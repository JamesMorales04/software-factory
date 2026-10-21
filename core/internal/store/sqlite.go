// Package store owns durable state: Projects, Actions and the domain
// event ledger backed by SQLite in WAL mode. Business mutations and
// their events commit in a single transaction; the ledger is the
// replayable source of truth for the UI.
package store

import (
	"context"
	"crypto/rand"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	_ "modernc.org/sqlite"
)

// SQLite pragmas are fixed at open time: WAL for crash-safe concurrent
// readers, synchronous NORMAL (durable with WAL), foreign keys enforced
// and a bounded busy timeout instead of indefinite blocking.
const openParams = "?_pragma=journal_mode(WAL)&_pragma=synchronous(NORMAL)&_pragma=foreign_keys(ON)&_pragma=busy_timeout(5000)"

const schema = `
CREATE TABLE IF NOT EXISTS projects (
	id TEXT PRIMARY KEY,
	name TEXT NOT NULL,
	created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS actions (
	id TEXT PRIMARY KEY,
	project_id TEXT NOT NULL REFERENCES projects(id),
	title TEXT NOT NULL,
	description TEXT NOT NULL DEFAULT '',
	status TEXT NOT NULL,
	idempotency_key TEXT UNIQUE,
	created_at TEXT NOT NULL,
	updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS events (
	global_seq INTEGER PRIMARY KEY AUTOINCREMENT,
	event_id TEXT NOT NULL UNIQUE,
	event_type TEXT NOT NULL,
	schema_version TEXT NOT NULL,
	aggregate_type TEXT NOT NULL,
	aggregate_id TEXT NOT NULL,
	sequence INTEGER NOT NULL,
	occurred_at TEXT NOT NULL,
	recorded_at TEXT NOT NULL,
	correlation_id TEXT NOT NULL DEFAULT '',
	causation_id TEXT NOT NULL DEFAULT '',
	actor TEXT NOT NULL DEFAULT '',
	payload TEXT NOT NULL DEFAULT '{}',
	UNIQUE (aggregate_type, aggregate_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_events_global ON events(global_seq);
CREATE INDEX IF NOT EXISTS idx_actions_project ON actions(project_id, created_at);
CREATE TABLE IF NOT EXISTS runs (
	id TEXT PRIMARY KEY,
	project_id TEXT NOT NULL REFERENCES projects(id),
	action_id TEXT NOT NULL DEFAULT '',
	workload_kind TEXT NOT NULL,
	workload_input TEXT NOT NULL DEFAULT '{}',
	status TEXT NOT NULL,
	attempt_count INTEGER NOT NULL DEFAULT 0,
	max_attempts INTEGER NOT NULL DEFAULT 3,
	timeout_secs INTEGER NOT NULL DEFAULT 120,
	lease_owner TEXT NOT NULL DEFAULT '',
	lease_generation INTEGER NOT NULL DEFAULT 0,
	lease_expires_at TEXT NOT NULL DEFAULT '',
	result TEXT NOT NULL DEFAULT '{}',
	error TEXT NOT NULL DEFAULT '',
	execution_mode TEXT NOT NULL DEFAULT 'restricted',
	provider TEXT NOT NULL DEFAULT '',
	model TEXT NOT NULL DEFAULT '',
	paseo_mode TEXT NOT NULL DEFAULT '',
	spec_id TEXT NOT NULL DEFAULT '',
	created_at TEXT NOT NULL,
	updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS attempts (
	id TEXT PRIMARY KEY,
	run_id TEXT NOT NULL REFERENCES runs(id),
	seq INTEGER NOT NULL,
	worker_id TEXT NOT NULL,
	lease_generation INTEGER NOT NULL,
	status TEXT NOT NULL,
	heartbeats INTEGER NOT NULL DEFAULT 0,
	result TEXT NOT NULL DEFAULT '{}',
	error TEXT NOT NULL DEFAULT '',
	created_at TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	UNIQUE (run_id, seq)
);
CREATE TABLE IF NOT EXISTS workers (
	id TEXT PRIMARY KEY,
	version TEXT NOT NULL DEFAULT '',
	status TEXT NOT NULL,
	last_heartbeat TEXT NOT NULL DEFAULT '',
	last_run_id TEXT NOT NULL DEFAULT '',
	created_at TEXT NOT NULL,
	updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_runs_status ON runs(status, created_at);
CREATE INDEX IF NOT EXISTS idx_attempts_run ON attempts(run_id, seq);
-- S3-A: durable 1:1 link between one Attempt generation and one real Paseo
-- session. The row is intended before any non-idempotent Paseo call so a
-- crash between session creation and identity persistence is detectable.
CREATE TABLE IF NOT EXISTS paseo_bindings (
	attempt_id TEXT PRIMARY KEY,
	run_id TEXT NOT NULL REFERENCES runs(id),
	seq INTEGER NOT NULL,
	worker_id TEXT NOT NULL,
	lease_generation INTEGER NOT NULL,
	provider TEXT NOT NULL,
	model TEXT NOT NULL,
	mode TEXT NOT NULL,
	exec_mode TEXT NOT NULL,
	workspace_id TEXT NOT NULL DEFAULT '',
	session_id TEXT NOT NULL DEFAULT '',
	correlation_key TEXT NOT NULL DEFAULT '',
	status TEXT NOT NULL,
	error TEXT NOT NULL DEFAULT '',
	created_at TEXT NOT NULL,
	updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_paseo_bindings_run ON paseo_bindings(run_id);
CREATE INDEX IF NOT EXISTS idx_paseo_bindings_session ON paseo_bindings(session_id);
CREATE INDEX IF NOT EXISTS idx_paseo_bindings_status ON paseo_bindings(status);
-- S3-D: durable cleanup/reconciliation receipt for an orphaned binding.
-- It is an operational record, not a second source of truth for the
-- immutable binding or the Run.
CREATE TABLE IF NOT EXISTS paseo_recovery (
	attempt_id TEXT PRIMARY KEY REFERENCES paseo_bindings(attempt_id),
	run_id TEXT NOT NULL REFERENCES runs(id),
	lease_generation INTEGER NOT NULL,
	status TEXT NOT NULL,
	claim_owner TEXT NOT NULL DEFAULT '',
	claim_token TEXT NOT NULL DEFAULT '',
	claim_expires_at TEXT NOT NULL DEFAULT '',
	attempts INTEGER NOT NULL DEFAULT 0,
	next_attempt_at TEXT NOT NULL DEFAULT '',
	last_observed_at TEXT NOT NULL DEFAULT '',
	session_id TEXT NOT NULL DEFAULT '',
	workspace_id TEXT NOT NULL DEFAULT '',
	session_observed TEXT NOT NULL DEFAULT '',
	workspace_observed TEXT NOT NULL DEFAULT '',
	last_outcome TEXT NOT NULL DEFAULT '',
	reason TEXT NOT NULL DEFAULT '',
	created_at TEXT NOT NULL,
	updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_paseo_recovery_status ON paseo_recovery(status, next_attempt_at);
-- S4-A: frozen engineering specification bound to one Action.
CREATE TABLE IF NOT EXISTS action_specs (
	id TEXT PRIMARY KEY,
	project_id TEXT NOT NULL REFERENCES projects(id),
	action_id TEXT NOT NULL REFERENCES actions(id),
	version INTEGER NOT NULL DEFAULT 1,
	content_hash TEXT NOT NULL UNIQUE,
	policy_revision TEXT NOT NULL,
	goal TEXT NOT NULL,
	repository TEXT NOT NULL,
	base_revision TEXT NOT NULL,
	allowed_paths TEXT NOT NULL,
	acceptance_criteria TEXT NOT NULL,
	negative_cases TEXT NOT NULL,
	risk_class TEXT NOT NULL,
	quality_profile TEXT NOT NULL,
	budget TEXT NOT NULL,
	execution_mode TEXT NOT NULL,
	provider TEXT NOT NULL,
	model TEXT NOT NULL,
	paseo_mode TEXT NOT NULL,
	created_at TEXT NOT NULL,
	frozen_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_action_specs_action ON action_specs(action_id, version DESC);
-- S4-A: immutable candidate identity. Frozen rows are never updated except
-- the explicit superseded state/timestamp.
CREATE TABLE IF NOT EXISTS candidates (
	id TEXT PRIMARY KEY,
	project_id TEXT NOT NULL REFERENCES projects(id),
	action_id TEXT NOT NULL REFERENCES actions(id),
	spec_id TEXT NOT NULL REFERENCES action_specs(id),
	spec_hash TEXT NOT NULL,
	policy_revision TEXT NOT NULL,
	base_revision TEXT NOT NULL,
	commit_sha TEXT NOT NULL,
	tree_sha TEXT NOT NULL,
	content_digest TEXT NOT NULL,
	candidate_ref TEXT NOT NULL,
	parent_candidate_id TEXT NOT NULL DEFAULT '',
	repository TEXT NOT NULL,
	implementer_run_id TEXT NOT NULL REFERENCES runs(id),
	implementer_attempt_id TEXT NOT NULL REFERENCES attempts(id),
	implementer_generation INTEGER NOT NULL,
	implementer_worker_id TEXT NOT NULL,
	execution_mode TEXT NOT NULL,
	provider TEXT NOT NULL,
	model TEXT NOT NULL,
	paseo_mode TEXT NOT NULL,
	changed_paths TEXT NOT NULL,
	risk_class TEXT NOT NULL,
	state TEXT NOT NULL,
	created_at TEXT NOT NULL,
	frozen_at TEXT NOT NULL,
	superseded_at TEXT NOT NULL DEFAULT '',
	UNIQUE (implementer_attempt_id),
	UNIQUE (implementer_run_id, implementer_generation)
);
CREATE INDEX IF NOT EXISTS idx_candidates_action ON candidates(action_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_candidates_run ON candidates(implementer_run_id);
CREATE INDEX IF NOT EXISTS idx_candidates_state ON candidates(state, frozen_at);
-- S4-A: append-only independent verifier evidence.
CREATE TABLE IF NOT EXISTS candidate_verifications (
	id TEXT PRIMARY KEY,
	candidate_id TEXT NOT NULL REFERENCES candidates(id),
	candidate_sha TEXT NOT NULL,
	spec_id TEXT NOT NULL,
	spec_hash TEXT NOT NULL,
	policy_revision TEXT NOT NULL,
	authority TEXT NOT NULL,
	actor TEXT NOT NULL,
	independent INTEGER NOT NULL,
	clean_resource TEXT NOT NULL,
	oracle_digest TEXT NOT NULL,
	status TEXT NOT NULL,
	checks TEXT NOT NULL,
	findings TEXT NOT NULL,
	idempotency_key TEXT NOT NULL,
	created_at TEXT NOT NULL,
	UNIQUE (candidate_id, idempotency_key)
);
CREATE INDEX IF NOT EXISTS idx_candidate_verifications_candidate ON candidate_verifications(candidate_id, created_at);
-- S4-A: append-only read-only reviewer verdicts.
CREATE TABLE IF NOT EXISTS candidate_reviews (
	id TEXT PRIMARY KEY,
	candidate_id TEXT NOT NULL REFERENCES candidates(id),
	candidate_sha TEXT NOT NULL,
	spec_id TEXT NOT NULL,
	spec_hash TEXT NOT NULL,
	policy_revision TEXT NOT NULL,
	authority TEXT NOT NULL,
	actor TEXT NOT NULL,
	verdict TEXT NOT NULL,
	findings TEXT NOT NULL,
	diff_stat TEXT NOT NULL,
	diff_bytes INTEGER NOT NULL,
	idempotency_key TEXT NOT NULL,
	created_at TEXT NOT NULL,
	UNIQUE (candidate_id, idempotency_key)
);
CREATE INDEX IF NOT EXISTS idx_candidate_reviews_candidate ON candidate_reviews(candidate_id, created_at);
-- S4-A: exactly one authoritative Quality Gate decision per candidate.
CREATE TABLE IF NOT EXISTS gate_decisions (
	id TEXT PRIMARY KEY,
	candidate_id TEXT NOT NULL UNIQUE REFERENCES candidates(id),
	candidate_sha TEXT NOT NULL,
	spec_id TEXT NOT NULL,
	spec_hash TEXT NOT NULL,
	policy_revision TEXT NOT NULL,
	status TEXT NOT NULL,
	evidence_digest TEXT NOT NULL DEFAULT '',
	reasons TEXT NOT NULL,
	checks TEXT NOT NULL,
	actor TEXT NOT NULL,
	created_at TEXT NOT NULL
);
-- S4-B: one Core-owned campaign binds a frozen spec to an immutable task
-- plan. Campaign state is authoritative; task rows carry scheduling state.
CREATE TABLE IF NOT EXISTS campaigns (
	id TEXT PRIMARY KEY,
	project_id TEXT NOT NULL REFERENCES projects(id),
	action_id TEXT NOT NULL REFERENCES actions(id),
	spec_id TEXT NOT NULL REFERENCES action_specs(id),
	spec_hash TEXT NOT NULL,
	policy_revision TEXT NOT NULL,
	plan_hash TEXT NOT NULL,
	plan TEXT NOT NULL,
	status TEXT NOT NULL,
	terminal_reason TEXT NOT NULL DEFAULT '',
	max_repair_cycles INTEGER NOT NULL,
	repair_cycles INTEGER NOT NULL DEFAULT 0,
	max_attempts INTEGER NOT NULL DEFAULT 1,
	attempt_budget_version INTEGER NOT NULL DEFAULT 1,
	max_wall_clock_secs INTEGER NOT NULL,
	max_parallel INTEGER NOT NULL,
	idempotency_key TEXT NOT NULL DEFAULT '',
	correlation_id TEXT NOT NULL DEFAULT '',
	created_at TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	started_at TEXT NOT NULL DEFAULT '',
	completed_at TEXT NOT NULL DEFAULT '',
	UNIQUE (action_id, spec_id),
	UNIQUE (spec_id)
);
CREATE INDEX IF NOT EXISTS idx_campaigns_status ON campaigns(status, created_at);
CREATE INDEX IF NOT EXISTS idx_campaigns_action ON campaigns(action_id, created_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_campaigns_idempotency ON campaigns(idempotency_key) WHERE idempotency_key<>'';
CREATE TABLE IF NOT EXISTS campaign_tasks (
	id TEXT PRIMARY KEY,
	campaign_id TEXT NOT NULL REFERENCES campaigns(id),
	task_key TEXT NOT NULL,
	role TEXT NOT NULL,
	cycle INTEGER NOT NULL,
	status TEXT NOT NULL,
	depends_on TEXT NOT NULL DEFAULT '[]',
	participant_id TEXT NOT NULL DEFAULT '',
	candidate_id TEXT NOT NULL DEFAULT '',
	run_id TEXT NOT NULL DEFAULT '',
	outcome TEXT NOT NULL DEFAULT '',
	error TEXT NOT NULL DEFAULT '',
	execution_count INTEGER NOT NULL DEFAULT 0,
	max_executions INTEGER NOT NULL,
	idempotency_key TEXT NOT NULL DEFAULT '',
	dispatch_owner TEXT NOT NULL DEFAULT '',
	dispatch_token TEXT NOT NULL DEFAULT '',
	dispatch_expires_at TEXT NOT NULL DEFAULT '',
	created_at TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	started_at TEXT NOT NULL DEFAULT '',
	completed_at TEXT NOT NULL DEFAULT '',
	UNIQUE (campaign_id, task_key)
);
CREATE INDEX IF NOT EXISTS idx_campaign_tasks_campaign ON campaign_tasks(campaign_id, created_at);
CREATE INDEX IF NOT EXISTS idx_campaign_tasks_status ON campaign_tasks(status, campaign_id);
`

// DB is the S2 persistence adapter (S1: actions/events; S2 adds
// runs/attempts/workers). A single logical writer owns transactions;
// readers observe WAL snapshots.
type DB struct {
	sql   *sql.DB
	hooks testHooks
}

// S2 test hooks. Production code always uses the defaults installed by
// Open; tests override them for deterministic lease/expiry behavior.
type testHooks struct {
	clock        func() time.Time
	leaseTTL     time.Duration
	offlineAfter time.Duration
}

// Open creates the schema, enforces WAL mode and seeds the default project.
func Open(path string) (*DB, error) {
	sqldb, err := sql.Open("sqlite", path+openParams)
	if err != nil {
		return nil, fmt.Errorf("open sqlite: %w", err)
	}
	sqldb.SetMaxOpenConns(1)
	if _, err := sqldb.Exec(schema); err != nil {
		_ = sqldb.Close()
		return nil, fmt.Errorf("apply schema: %w", err)
	}
	db := &DB{sql: sqldb}
	if err := db.migrateS3A(); err != nil {
		_ = sqldb.Close()
		return nil, fmt.Errorf("s3-a migration: %w", err)
	}
	if err := db.migrateS3D(); err != nil {
		_ = sqldb.Close()
		return nil, fmt.Errorf("s3-d migration: %w", err)
	}
	if err := db.migrateS4A(); err != nil {
		_ = sqldb.Close()
		return nil, fmt.Errorf("s4-a migration: %w", err)
	}
	if err := db.migrateS4B(); err != nil {
		_ = sqldb.Close()
		return nil, fmt.Errorf("s4-b migration: %w", err)
	}
	db.hooks.clock = utcNow
	db.hooks.leaseTTL = domain.DefaultLeaseTTL
	db.hooks.offlineAfter = domain.WorkerOfflineAfter
	if err := db.assertWAL(); err != nil {
		_ = sqldb.Close()
		return nil, err
	}
	if err := db.seedDefaultProject(); err != nil {
		_ = sqldb.Close()
		return nil, err
	}
	return db, nil
}

// now reports the store clock (overridable in tests for deterministic
// lease/expiry behavior; production always uses UTC wall time).
func (db *DB) now() time.Time {
	if db.hooks.clock != nil {
		return db.hooks.clock()
	}
	return utcNow()
}

// SetClock overrides the store clock. Test-only hook for deterministic
// lease-expiry and reconciliation tests.
func (db *DB) SetClock(clock func() time.Time) { db.hooks.clock = clock }

// SetLeaseTTL overrides the granted lease duration. Test-only hook so
// expiry tests run in milliseconds instead of seconds.
func (db *DB) SetLeaseTTL(ttl time.Duration) { db.hooks.leaseTTL = ttl }

// SetWorkerOfflineAfter overrides the heartbeat-overdue threshold.
// Test-only hook for deterministic liveness tests.
func (db *DB) SetWorkerOfflineAfter(d time.Duration) { db.hooks.offlineAfter = d }

// Close releases the database handle.
func (db *DB) Close() error { return db.sql.Close() }

// assertWAL verifies the durability assumption S1 depends on instead of
// assuming the pragma took effect.
func (db *DB) assertWAL() error {
	var mode string
	if err := db.sql.QueryRow(`PRAGMA journal_mode`).Scan(&mode); err != nil {
		return fmt.Errorf("read journal_mode: %w", err)
	}
	if !strings.EqualFold(mode, "wal") {
		return fmt.Errorf("journal_mode is %q, S1 requires WAL", mode)
	}
	return nil
}

// JournalMode reports the active journal mode (used by acceptance tests).
func (db *DB) JournalMode() (string, error) {
	var mode string
	if err := db.sql.QueryRow(`PRAGMA journal_mode`).Scan(&mode); err != nil {
		return "", err
	}
	return mode, nil
}

// migrateS3A adds the S3-A runs columns to databases created before the
// session-identity slice. Fresh databases already carry them via the schema
// above; the ALTERs below only fire when the column is absent, so the
// migration is idempotent and never rewrites existing rows.
func (db *DB) migrateS3A() error {
	rows, err := db.sql.Query(`SELECT name FROM pragma_table_info('runs')`)
	if err != nil {
		return err
	}
	present := map[string]bool{}
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			_ = rows.Close()
			return err
		}
		present[name] = true
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return err
	}
	for _, col := range []string{"execution_mode", "provider", "model", "paseo_mode"} {
		if present[col] {
			continue
		}
		def := `TEXT NOT NULL DEFAULT ''`
		if col == "execution_mode" {
			def = `TEXT NOT NULL DEFAULT 'restricted'`
		}
		if _, err := db.sql.Exec(`ALTER TABLE runs ADD COLUMN ` + col + ` ` + def); err != nil {
			return fmt.Errorf("add runs.%s: %w", col, err)
		}
	}
	return nil
}

// migrateS3D adds the S3-D correlation column to databases created before
// this slice. The recovery table and its index are created idempotently by
// the schema block above. Existing bindings keep an empty correlation key:
// they are legacy rows and must never be matched by the new exact labels.
func (db *DB) migrateS3D() error {
	rows, err := db.sql.Query(`SELECT name FROM pragma_table_info('paseo_bindings')`)
	if err != nil {
		return err
	}
	present := false
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			_ = rows.Close()
			return err
		}
		if name == "correlation_key" {
			present = true
		}
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return err
	}
	if !present {
		if _, err := db.sql.Exec(`ALTER TABLE paseo_bindings ADD COLUMN correlation_key TEXT NOT NULL DEFAULT ''`); err != nil {
			return fmt.Errorf("add paseo_bindings.correlation_key: %w", err)
		}
	}
	// The recovery table is new in S3-D, but a mid-development database may
	// already carry it without the last_outcome column.
	rows, err = db.sql.Query(`SELECT name FROM pragma_table_info('paseo_recovery')`)
	if err != nil {
		return err
	}
	hasOutcome := false
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			_ = rows.Close()
			return err
		}
		if name == "last_outcome" {
			hasOutcome = true
		}
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return err
	}
	if !hasOutcome {
		if _, err := db.sql.Exec(`ALTER TABLE paseo_recovery ADD COLUMN last_outcome TEXT NOT NULL DEFAULT ''`); err != nil {
			return fmt.Errorf("add paseo_recovery.last_outcome: %w", err)
		}
	}
	return nil
}

// migrateS4A adds the S4-A runs.spec_id column to databases created before
// the quality pipeline. The spec/candidate/evidence tables are created
// idempotently by the schema block above.
func (db *DB) migrateS4A() error {
	rows, err := db.sql.Query(`SELECT name FROM pragma_table_info('runs')`)
	if err != nil {
		return err
	}
	present := false
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			_ = rows.Close()
			return err
		}
		if name == "spec_id" {
			present = true
		}
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return err
	}
	if !present {
		if _, err := db.sql.Exec(`ALTER TABLE runs ADD COLUMN spec_id TEXT NOT NULL DEFAULT ''`); err != nil {
			return fmt.Errorf("add runs.spec_id: %w", err)
		}
	}
	rows, err = db.sql.Query(`SELECT name FROM pragma_table_info('gate_decisions')`)
	if err != nil {
		return err
	}
	hasDigest := false
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			_ = rows.Close()
			return err
		}
		if name == "evidence_digest" {
			hasDigest = true
		}
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return err
	}
	if !hasDigest {
		if _, err := db.sql.Exec(`ALTER TABLE gate_decisions ADD COLUMN evidence_digest TEXT NOT NULL DEFAULT ''`); err != nil {
			return fmt.Errorf("add gate_decisions.evidence_digest: %w", err)
		}
	}
	return nil
}

// migrateS4B upgrades databases created before the repaired Campaign
// invariants. Fresh databases already carry attempt_budget_version=1 and the
// partial unique index; older rows are backfilled once, duplicate active
// campaigns are blocked instead of breaking startup, and only then is the
// uniqueness index created.
func (db *DB) migrateS4B() error {
	rows, err := db.sql.Query(`SELECT name FROM pragma_table_info('campaigns')`)
	if err != nil {
		return err
	}
	hasBudgetVersion := false
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			_ = rows.Close()
			return err
		}
		if name == "attempt_budget_version" {
			hasBudgetVersion = true
		}
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return err
	}
	if !hasBudgetVersion {
		if _, err := db.sql.Exec(`ALTER TABLE campaigns ADD COLUMN attempt_budget_version INTEGER NOT NULL DEFAULT 0`); err != nil {
			return fmt.Errorf("add campaigns.attempt_budget_version: %w", err)
		}
	}
	if err := db.backfillCampaignBudgets(); err != nil {
		return err
	}
	if err := db.blockDuplicateActiveCampaigns(); err != nil {
		return err
	}
	if _, err := db.sql.Exec(
		`CREATE UNIQUE INDEX IF NOT EXISTS idx_campaigns_active_action
		 ON campaigns(action_id) WHERE status IN ('pending','running')`); err != nil {
		return fmt.Errorf("create active campaign uniqueness index: %w", err)
	}
	return nil
}

// backfillCampaignBudgets recomputes legacy per-Run attempt limits from the
// frozen spec. Recomputation is idempotent and distinguishes a b71-era total
// budget from a true pre-total row without multiplying twice.
func (db *DB) backfillCampaignBudgets() error {
	rows, err := db.sql.Query(
		`SELECT id, spec_id, max_repair_cycles FROM campaigns WHERE attempt_budget_version=0`)
	if err != nil {
		return err
	}
	type legacy struct {
		id, specID string
		repairs    int
	}
	var pending []legacy
	for rows.Next() {
		var item legacy
		if err := rows.Scan(&item.id, &item.specID, &item.repairs); err != nil {
			_ = rows.Close()
			return err
		}
		pending = append(pending, item)
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return err
	}
	for _, item := range pending {
		var budgetJSON string
		if err := db.sql.QueryRow(`SELECT budget FROM action_specs WHERE id=?`, item.specID).Scan(&budgetJSON); err != nil {
			if err == sql.ErrNoRows {
				continue
			}
			return err
		}
		var budget domain.SpecBudget
		if err := json.Unmarshal([]byte(budgetJSON), &budget); err != nil {
			return fmt.Errorf("decode legacy spec budget: %w", err)
		}
		if budget.MaxAttempts < 1 {
			continue
		}
		total := budget.MaxAttempts * (item.repairs + 1)
		if _, err := db.sql.Exec(
			`UPDATE campaigns SET max_attempts=?, attempt_budget_version=1 WHERE id=?`,
			total, item.id); err != nil {
			return fmt.Errorf("backfill campaign attempt budget: %w", err)
		}
	}
	return nil
}

// blockDuplicateActiveCampaigns resolves pre-index data through the normal
// terminal transition: Run cancellation, task cancellation and durable
// events are recorded rather than rewritten behind the ledger.
func (db *DB) blockDuplicateActiveCampaigns() error {
	rows, err := db.sql.Query(
		`SELECT id, action_id FROM campaigns WHERE status IN (?,?)
		 ORDER BY action_id ASC, created_at ASC, id ASC`,
		string(domain.CampaignPending), string(domain.CampaignRunning))
	if err != nil {
		return err
	}
	seen := map[string]bool{}
	var duplicates []string
	for rows.Next() {
		var id, actionID string
		if err := rows.Scan(&id, &actionID); err != nil {
			_ = rows.Close()
			return err
		}
		if seen[actionID] {
			duplicates = append(duplicates, id)
			continue
		}
		seen[actionID] = true
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return err
	}
	for _, id := range duplicates {
		if err := db.blockCampaignOnMigration(id); err != nil {
			return err
		}
	}
	return nil
}

func (db *DB) blockCampaignOnMigration(campaignID string) error {
	ctx := context.Background()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback() }()
	campaign, err := scanCampaign(tx.QueryRowContext(ctx,
		`SELECT `+campaignColumns+` FROM campaigns WHERE id=?`, campaignID))
	if err != nil {
		if err == sql.ErrNoRows {
			return nil
		}
		return err
	}
	if campaign.Status != domain.CampaignPending && campaign.Status != domain.CampaignRunning {
		return tx.Commit()
	}
	if _, _, err := finishCampaignTx(ctx, tx, campaignID, domain.CampaignBlocked,
		"migration: duplicate active campaign", domain.EventCampaignFinished,
		"s4b-migration", utcNow()); err != nil {
		return err
	}
	return tx.Commit()
}

func newID() string {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		panic("crypto/rand unavailable: " + err.Error())
	}
	return hex.EncodeToString(b[:])
}

func utcNow() time.Time { return time.Now().UTC().Truncate(time.Millisecond) }

func (db *DB) seedDefaultProject() error {
	_, err := db.sql.Exec(
		`INSERT INTO projects(id, name, created_at) VALUES(?,?,?)
		 ON CONFLICT(id) DO NOTHING`,
		domain.DefaultProjectID, "Default Project", utcNow().Format(time.RFC3339Nano),
	)
	return err
}
