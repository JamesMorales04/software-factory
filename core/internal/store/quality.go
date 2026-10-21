package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/quality"
)

// S4-A bounds for durable evidence lists.
const (
	maxVerificationChecks = 32
	maxReviewFindings     = domain.MaxFindings
	maxEvidenceFindings   = 32
)

const specColumns = `id, project_id, action_id, version, content_hash, policy_revision,
	goal, repository, base_revision, allowed_paths, acceptance_criteria,
	negative_cases, risk_class, quality_profile, budget, execution_mode,
	provider, model, paseo_mode, project_config_revision, repository_identity,
	capabilities, browser_endpoints, browser_test_server, binding_hash,
	created_at, frozen_at`

const candidateColumns = `id, project_id, action_id, spec_id, spec_hash, policy_revision,
	base_revision, commit_sha, tree_sha, content_digest, candidate_ref,
	parent_candidate_id, repository, implementer_run_id, implementer_attempt_id,
	implementer_generation, implementer_worker_id, execution_mode, provider,
	model, paseo_mode, changed_paths, risk_class, spec_binding_hash, state,
	created_at, frozen_at, superseded_at`

const verificationColumns = `id, candidate_id, candidate_sha, spec_id, spec_hash,
	policy_revision, authority, actor, independent, clean_resource, oracle_digest,
	status, checks, findings, idempotency_key, created_at`

const reviewColumns = `id, candidate_id, candidate_sha, spec_id, spec_hash,
	policy_revision, authority, actor, verdict, findings, diff_stat, diff_bytes,
	idempotency_key, created_at`

const decisionColumns = `id, candidate_id, candidate_sha, spec_id, spec_hash,
	policy_revision, status, evidence_digest, reasons, checks, actor, created_at`

// CreateActionSpec freezes one ActionSpec. Repeated delivery of identical
// normative content is an idempotent replay: no second row or event.
func (db *DB) CreateActionSpec(ctx context.Context, spec domain.ActionSpec, correlationID string) (domain.ActionSpec, bool, domain.Event, error) {
	if err := spec.Validate(); err != nil {
		return domain.ActionSpec{}, false, domain.Event{}, fmt.Errorf("%w: %v", domain.ErrInvalidInput, err)
	}
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.ActionSpec{}, false, domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()

	var projectID string
	if err := tx.QueryRowContext(ctx,
		`SELECT project_id FROM actions WHERE id=?`, spec.ActionID).Scan(&projectID); err != nil {
		if err == sql.ErrNoRows {
			return domain.ActionSpec{}, false, domain.Event{}, fmt.Errorf("%w: unknown actionId %q",
				domain.ErrInvalidInput, spec.ActionID)
		}
		return domain.ActionSpec{}, false, domain.Event{}, err
	}
	if projectID != spec.ProjectID {
		return domain.ActionSpec{}, false, domain.Event{}, fmt.Errorf("%w: action %q belongs to project %q",
			domain.ErrUnauthorized, spec.ActionID, projectID)
	}
	project, err := scanProject(tx.QueryRowContext(ctx,
		`SELECT `+projectColumns+` FROM projects WHERE id=?`, spec.ProjectID))
	if err != nil {
		if err == sql.ErrNoRows {
			return domain.ActionSpec{}, false, domain.Event{}, fmt.Errorf("%w: unknown projectId %q",
				domain.ErrInvalidInput, spec.ProjectID)
		}
		return domain.ActionSpec{}, false, domain.Event{}, err
	}
	if err := spec.ValidateProjectBinding(project); err != nil {
		return domain.ActionSpec{}, false, domain.Event{}, fmt.Errorf("%w: %v", domain.ErrUnauthorized, err)
	}

	if existing, gerr := scanSpec(tx.QueryRowContext(ctx,
		`SELECT `+specColumns+` FROM action_specs WHERE content_hash=?`, spec.ContentHash)); gerr == nil {
		if cerr := tx.Commit(); cerr != nil {
			return domain.ActionSpec{}, false, domain.Event{}, cerr
		}
		return existing, true, domain.Event{}, nil
	} else if gerr != sql.ErrNoRows {
		return domain.ActionSpec{}, false, domain.Event{}, gerr
	}

	spec.ID = newID()
	spec.Version = 1
	spec.CreatedAt = now
	spec.FrozenAt = now
	allowed, err := json.Marshal(spec.AllowedPaths)
	if err != nil {
		return domain.ActionSpec{}, false, domain.Event{}, err
	}
	acceptance, err := json.Marshal(spec.AcceptanceCriteria)
	if err != nil {
		return domain.ActionSpec{}, false, domain.Event{}, err
	}
	negative, err := json.Marshal(spec.NegativeCases)
	if err != nil {
		return domain.ActionSpec{}, false, domain.Event{}, err
	}
	budget, err := json.Marshal(spec.Budget)
	if err != nil {
		return domain.ActionSpec{}, false, domain.Event{}, err
	}
	capabilities, err := json.Marshal(spec.Capabilities)
	if err != nil {
		return domain.ActionSpec{}, false, domain.Event{}, err
	}
	endpoints, err := json.Marshal(spec.BrowserEndpoints)
	if err != nil {
		return domain.ActionSpec{}, false, domain.Event{}, err
	}
	testServer := encodeBrowserTestServer(spec.BrowserTestServer)
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO action_specs(id, project_id, action_id, version, content_hash,
		 policy_revision, goal, repository, base_revision, allowed_paths,
		 acceptance_criteria, negative_cases, risk_class, quality_profile,
		 budget, execution_mode, provider, model, paseo_mode,
		 project_config_revision, repository_identity, capabilities,
		 browser_endpoints, browser_test_server, binding_hash, created_at, frozen_at)
		 VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`,
		spec.ID, spec.ProjectID, spec.ActionID, spec.Version, spec.ContentHash,
		spec.PolicyRevision, spec.Goal, spec.Repository, spec.BaseRevision, string(allowed),
		string(acceptance), string(negative), string(spec.RiskClass), string(spec.QualityProfile),
		string(budget), spec.ExecutionMode, spec.Provider, spec.Model, spec.PaseoMode,
		spec.ProjectConfigRevision, spec.RepositoryIdentity, string(capabilities),
		string(endpoints), testServer, spec.BindingHash, stamp(now), stamp(now),
	); err != nil {
		return domain.ActionSpec{}, false, domain.Event{}, fmt.Errorf("insert action spec: %w", err)
	}
	payload, _ := json.Marshal(map[string]any{
		"specId":                spec.ID,
		"projectId":             spec.ProjectID,
		"actionId":              spec.ActionID,
		"version":               spec.Version,
		"contentHash":           spec.ContentHash,
		"policyRevision":        spec.PolicyRevision,
		"riskClass":             spec.RiskClass,
		"qualityProfile":        spec.QualityProfile,
		"baseRevision":          spec.BaseRevision,
		"repositoryIdentity":    spec.RepositoryIdentity,
		"capabilities":          spec.Capabilities,
		"bindingHash":           spec.BindingHash,
		"projectConfigRevision": spec.ProjectConfigRevision,
	})
	event := domain.Event{
		EventID:       newID(),
		EventType:     domain.EventActionSpecFrozen,
		SchemaVersion: domain.EventSchemaVersion,
		AggregateType: domain.AggregateSpec,
		AggregateID:   spec.ID,
		OccurredAt:    now,
		RecordedAt:    now,
		CorrelationID: correlationID,
		Actor:         ActorLocalOperator,
		Payload:       mustDecode(payload),
	}
	if event.Sequence, err = nextSequence(ctx, tx, domain.AggregateSpec, spec.ID); err != nil {
		return domain.ActionSpec{}, false, domain.Event{}, err
	}
	if err := insertEvent(ctx, tx, event); err != nil {
		return domain.ActionSpec{}, false, domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.ActionSpec{}, false, domain.Event{}, err
	}
	stored, err := db.GetActionSpec(spec.ID)
	if err != nil {
		return domain.ActionSpec{}, false, domain.Event{}, err
	}
	return stored, false, event, nil
}

func scanSpec(row interface {
	Scan(dest ...any) error
},
) (domain.ActionSpec, error) {
	var s domain.ActionSpec
	var allowed, acceptance, negative, budget string
	var risk, profile, capabilities, endpoints, testServer, created, frozen string
	if err := row.Scan(
		&s.ID, &s.ProjectID, &s.ActionID, &s.Version, &s.ContentHash,
		&s.PolicyRevision, &s.Goal, &s.Repository, &s.BaseRevision, &allowed,
		&acceptance, &negative, &risk, &profile, &budget, &s.ExecutionMode,
		&s.Provider, &s.Model, &s.PaseoMode, &s.ProjectConfigRevision,
		&s.RepositoryIdentity, &capabilities, &endpoints, &testServer,
		&s.BindingHash, &created, &frozen,
	); err != nil {
		return domain.ActionSpec{}, err
	}
	if err := json.Unmarshal([]byte(allowed), &s.AllowedPaths); err != nil {
		return domain.ActionSpec{}, err
	}
	if err := json.Unmarshal([]byte(acceptance), &s.AcceptanceCriteria); err != nil {
		return domain.ActionSpec{}, err
	}
	if err := json.Unmarshal([]byte(negative), &s.NegativeCases); err != nil {
		return domain.ActionSpec{}, err
	}
	if err := json.Unmarshal([]byte(budget), &s.Budget); err != nil {
		return domain.ActionSpec{}, err
	}
	var capabilityValues []string
	if err := json.Unmarshal([]byte(capabilities), &capabilityValues); err != nil {
		return domain.ActionSpec{}, err
	}
	s.Capabilities = make([]domain.Capability, 0, len(capabilityValues))
	for _, value := range capabilityValues {
		s.Capabilities = append(s.Capabilities, domain.Capability(value))
	}
	if err := json.Unmarshal([]byte(endpoints), &s.BrowserEndpoints); err != nil {
		return domain.ActionSpec{}, err
	}
	if testServer != "" && testServer != "null" {
		var server domain.BrowserTestServer
		if err := json.Unmarshal([]byte(testServer), &server); err != nil {
			return domain.ActionSpec{}, err
		}
		s.BrowserTestServer = &server
	}
	s.RiskClass = domain.RiskClass(risk)
	s.QualityProfile = domain.QualityProfile(profile)
	s.CreatedAt, _ = time.Parse(time.RFC3339Nano, created)
	s.FrozenAt, _ = time.Parse(time.RFC3339Nano, frozen)
	return s, nil
}

// GetActionSpec returns one frozen spec or ErrNotFound.
func (db *DB) GetActionSpec(id string) (domain.ActionSpec, error) {
	spec, err := scanSpec(db.sql.QueryRow(
		`SELECT `+specColumns+` FROM action_specs WHERE id=?`, id))
	if err == sql.ErrNoRows {
		return domain.ActionSpec{}, domain.ErrNotFound
	}
	return spec, err
}

// ListActionSpecs returns an Action's frozen specs newest first.
func (db *DB) ListActionSpecs(actionID string) ([]domain.ActionSpec, error) {
	rows, err := db.sql.Query(
		`SELECT `+specColumns+` FROM action_specs WHERE action_id=? ORDER BY created_at DESC, id DESC`,
		actionID)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.ActionSpec
	for rows.Next() {
		spec, err := scanSpec(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, spec)
	}
	return out, rows.Err()
}

// CreateCandidate atomically freezes a Core-validated candidate, supersedes
// prior frozen candidates for the same Action, and records the lineage
// events. The caller supplies only independently computed git evidence.
func (db *DB) CreateCandidate(ctx context.Context, candidate domain.CandidateRevision, correlationID string) (domain.CandidateRevision, []domain.Event, error) {
	now := db.now()
	if candidate.SpecBindingHash == "" {
		// Caller-built candidates may omit the binding hash; fill it from
		// the durable spec so evidence remains bound to the exact config.
		var binding string
		if err := db.sql.QueryRowContext(ctx, `SELECT binding_hash FROM action_specs WHERE id=?`, candidate.SpecID).Scan(&binding); err == nil {
			candidate.SpecBindingHash = binding
		}
	}
	candidate.ID = newID()
	candidate.State = domain.CandidateFrozen
	candidate.CreatedAt = now
	candidate.FrozenAt = now
	candidate.SupersededAt = time.Time{}
	if err := candidate.Validate(); err != nil {
		return domain.CandidateRevision{}, nil, fmt.Errorf("%w: %v", domain.ErrInvalidInput, err)
	}
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.CandidateRevision{}, nil, err
	}
	defer func() { _ = tx.Rollback() }()

	var runStatus, attemptStatus, runWorker, runSpec string
	var runGeneration int64
	if err := tx.QueryRowContext(ctx,
		`SELECT status, lease_owner, spec_id, lease_generation FROM runs WHERE id=?`,
		candidate.ImplementerRunID).Scan(&runStatus, &runWorker, &runSpec, &runGeneration); err != nil {
		if err == sql.ErrNoRows {
			return domain.CandidateRevision{}, nil, domain.ErrNotFound
		}
		return domain.CandidateRevision{}, nil, err
	}
	if runStatus != string(domain.RunRunning) {
		return domain.CandidateRevision{}, nil, fmt.Errorf("%w: candidate requires a running implementer Run (is %s)",
			domain.ErrInvalidTransition, runStatus)
	}
	if runSpec != candidate.SpecID || runGeneration != candidate.ImplementerGeneration {
		return domain.CandidateRevision{}, nil, fmt.Errorf("%w: candidate does not match the Run's frozen spec/generation",
			domain.ErrStaleGeneration)
	}
	if runWorker != candidate.ImplementerWorkerID {
		return domain.CandidateRevision{}, nil, fmt.Errorf("%w: candidate worker does not own the active lease",
			domain.ErrStaleGeneration)
	}
	if err := tx.QueryRowContext(ctx,
		`SELECT status, worker_id FROM attempts WHERE id=? AND run_id=? AND lease_generation=?`,
		candidate.ImplementerAttemptID, candidate.ImplementerRunID,
		candidate.ImplementerGeneration).Scan(&attemptStatus, &runWorker); err != nil {
		if err == sql.ErrNoRows {
			return domain.CandidateRevision{}, nil, domain.ErrNotFound
		}
		return domain.CandidateRevision{}, nil, err
	}
	if attemptStatus != string(domain.AttemptRunning) {
		return domain.CandidateRevision{}, nil, fmt.Errorf("%w: candidate requires an active Implementer Attempt (is %s)",
			domain.ErrInvalidTransition, attemptStatus)
	}

	spec, err := scanSpec(tx.QueryRowContext(ctx,
		`SELECT `+specColumns+` FROM action_specs WHERE id=?`, candidate.SpecID))
	if err != nil {
		return domain.CandidateRevision{}, nil, err
	}
	if spec.ContentHash != candidate.SpecHash || spec.PolicyRevision != candidate.PolicyRevision ||
		spec.ActionID != candidate.ActionID || spec.ProjectID != candidate.ProjectID ||
		spec.Repository != candidate.Repository || spec.BindingHash != candidate.SpecBindingHash {
		return domain.CandidateRevision{}, nil, fmt.Errorf("%w: candidate provenance does not match the frozen spec",
			domain.ErrUnauthorized)
	}
	project, err := scanProject(tx.QueryRowContext(ctx,
		`SELECT `+projectColumns+` FROM projects WHERE id=?`, spec.ProjectID))
	if err != nil {
		return domain.CandidateRevision{}, nil, fmt.Errorf("%w: candidate project is not registered", domain.ErrUnauthorized)
	}
	if err := spec.ValidateProjectBinding(project); err != nil {
		return domain.CandidateRevision{}, nil, fmt.Errorf("%w: %v", domain.ErrUnauthorized, err)
	}

	var priorID string
	err = tx.QueryRowContext(ctx,
		`SELECT c.id FROM candidates c JOIN runs r ON r.id=c.implementer_run_id
		 WHERE c.action_id=? AND c.state IN (?,?) AND r.status=?
		 ORDER BY c.frozen_at DESC, c.id DESC LIMIT 1`,
		candidate.ActionID, string(domain.CandidateFrozen), string(domain.CandidateSuperseded),
		string(domain.RunSucceeded)).Scan(&priorID)
	if err != nil && err != sql.ErrNoRows {
		return domain.CandidateRevision{}, nil, err
	}
	if err == sql.ErrNoRows {
		priorID = ""
	}
	candidate.ParentCandidateID = priorID
	changed, err := json.Marshal(candidate.ChangedPaths)
	if err != nil {
		return domain.CandidateRevision{}, nil, err
	}
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO candidates(id, project_id, action_id, spec_id, spec_hash,
		 policy_revision, base_revision, commit_sha, tree_sha, content_digest,
		 candidate_ref, parent_candidate_id, repository, implementer_run_id,
		 implementer_attempt_id, implementer_generation, implementer_worker_id,
		 execution_mode, provider, model, paseo_mode, changed_paths, risk_class,
		 spec_binding_hash, state, created_at, frozen_at, superseded_at)
		 VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`,
		candidate.ID, candidate.ProjectID, candidate.ActionID, candidate.SpecID,
		candidate.SpecHash, candidate.PolicyRevision, candidate.BaseRevision,
		candidate.CommitSHA, candidate.TreeSHA, candidate.ContentDigest,
		candidate.CandidateRef, candidate.ParentCandidateID, candidate.Repository,
		candidate.ImplementerRunID, candidate.ImplementerAttemptID,
		candidate.ImplementerGeneration, candidate.ImplementerWorkerID,
		candidate.ExecutionMode, candidate.Provider, candidate.Model,
		candidate.PaseoMode, string(changed), string(candidate.RiskClass),
		candidate.SpecBindingHash, string(candidate.State), stamp(now), stamp(now), "",
	); err != nil {
		return domain.CandidateRevision{}, nil, fmt.Errorf("insert candidate: %w", err)
	}

	payload, _ := json.Marshal(map[string]any{
		"candidateId":       candidate.ID,
		"actionId":          candidate.ActionID,
		"specId":            candidate.SpecID,
		"specHash":          candidate.SpecHash,
		"specBindingHash":   candidate.SpecBindingHash,
		"commitSha":         candidate.CommitSHA,
		"treeSha":           candidate.TreeSHA,
		"contentDigest":     candidate.ContentDigest,
		"baseRevision":      candidate.BaseRevision,
		"runId":             candidate.ImplementerRunID,
		"attemptId":         candidate.ImplementerAttemptID,
		"generation":        candidate.ImplementerGeneration,
		"workerId":          candidate.ImplementerWorkerID,
		"riskClass":         candidate.RiskClass,
		"changedPaths":      candidate.ChangedPaths,
		"parentCandidateId": candidate.ParentCandidateID,
	})
	frozenEvent, err := appendCandidateEvent(ctx, tx, domain.EventCandidateFrozen, candidate.ID, candidate.ImplementerWorkerID, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.CandidateRevision{}, nil, err
	}
	events := []domain.Event{frozenEvent}
	if err := tx.Commit(); err != nil {
		return domain.CandidateRevision{}, nil, err
	}
	stored, err := db.GetCandidate(candidate.ID)
	if err != nil {
		return domain.CandidateRevision{}, nil, err
	}
	return stored, events, nil
}

func appendCandidateEvent(ctx context.Context, tx *sql.Tx, eventType, candidateID, actor, correlationID string, now time.Time, payload map[string]any) (domain.Event, error) {
	event := domain.Event{
		EventID:       newID(),
		EventType:     eventType,
		SchemaVersion: domain.EventSchemaVersion,
		AggregateType: domain.AggregateCandidate,
		AggregateID:   candidateID,
		OccurredAt:    now,
		RecordedAt:    now,
		CorrelationID: correlationID,
		Actor:         actor,
		Payload:       payload,
	}
	seq, err := nextSequence(ctx, tx, domain.AggregateCandidate, candidateID)
	if err != nil {
		return domain.Event{}, err
	}
	event.Sequence = seq
	if err := insertEvent(ctx, tx, event); err != nil {
		return domain.Event{}, err
	}
	return event, nil
}

func scanCandidate(row interface {
	Scan(dest ...any) error
},
) (domain.CandidateRevision, error) {
	var c domain.CandidateRevision
	var changed, risk, state, created, frozen, superseded string
	if err := row.Scan(
		&c.ID, &c.ProjectID, &c.ActionID, &c.SpecID, &c.SpecHash,
		&c.PolicyRevision, &c.BaseRevision, &c.CommitSHA, &c.TreeSHA,
		&c.ContentDigest, &c.CandidateRef, &c.ParentCandidateID, &c.Repository,
		&c.ImplementerRunID, &c.ImplementerAttemptID, &c.ImplementerGeneration,
		&c.ImplementerWorkerID, &c.ExecutionMode, &c.Provider, &c.Model,
		&c.PaseoMode, &changed, &risk, &c.SpecBindingHash, &state, &created, &frozen, &superseded,
	); err != nil {
		return domain.CandidateRevision{}, err
	}
	if err := json.Unmarshal([]byte(changed), &c.ChangedPaths); err != nil {
		return domain.CandidateRevision{}, err
	}
	c.RiskClass = domain.RiskClass(risk)
	c.State = domain.CandidateState(state)
	c.CreatedAt = parseStamp(created)
	c.FrozenAt = parseStamp(frozen)
	c.SupersededAt = parseStamp(superseded)
	return c, nil
}

// GetCandidate returns one candidate or ErrNotFound.
func (db *DB) GetCandidate(id string) (domain.CandidateRevision, error) {
	c, err := scanCandidate(db.sql.QueryRow(
		`SELECT `+candidateColumns+` FROM candidates WHERE id=?`, id))
	if err == sql.ErrNoRows {
		return domain.CandidateRevision{}, domain.ErrNotFound
	}
	return c, err
}

// ListCandidates returns candidates newest first, optionally filtered by
// Action, Run or state.
func (db *DB) ListCandidates(projectID, actionID, runID string, limit int) ([]domain.CandidateRevision, error) {
	if limit <= 0 || limit > 200 {
		limit = 100
	}
	q := `SELECT ` + candidateColumns + ` FROM candidates WHERE 1=1`
	var args []any
	if projectID != "" {
		q += ` AND project_id=?`
		args = append(args, projectID)
	}
	if actionID != "" {
		q += ` AND action_id=?`
		args = append(args, actionID)
	}
	if runID != "" {
		q += ` AND implementer_run_id=?`
		args = append(args, runID)
	}
	q += ` ORDER BY created_at DESC, id DESC LIMIT ?`
	args = append(args, limit)
	rows, err := db.sql.Query(q, args...)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.CandidateRevision
	for rows.Next() {
		c, err := scanCandidate(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, c)
	}
	return out, rows.Err()
}

// GetCandidateByAttempt returns the frozen candidate for one Attempt.
func (db *DB) GetCandidateByAttempt(attemptID string) (domain.CandidateRevision, error) {
	c, err := scanCandidate(db.sql.QueryRow(
		`SELECT `+candidateColumns+` FROM candidates WHERE implementer_attempt_id=?`, attemptID))
	if err == sql.ErrNoRows {
		return domain.CandidateRevision{}, domain.ErrNotFound
	}
	return c, err
}

// LatestRunCandidate returns the newest candidate for one Implementer Run.
func (db *DB) LatestRunCandidate(runID string) (domain.CandidateRevision, error) {
	c, err := scanCandidate(db.sql.QueryRow(
		`SELECT `+candidateColumns+` FROM candidates WHERE implementer_run_id=? ORDER BY created_at DESC LIMIT 1`, runID))
	if err == sql.ErrNoRows {
		return domain.CandidateRevision{}, domain.ErrNotFound
	}
	return c, err
}

// GetAttempt returns one Attempt by Run/Attempt identity.
func (db *DB) GetAttempt(runID, attemptID string) (domain.Attempt, error) {
	a, err := scanAttempt(db.sql.QueryRow(
		`SELECT `+attemptColumns+` FROM attempts WHERE id=? AND run_id=?`, attemptID, runID))
	if err == sql.ErrNoRows {
		return domain.Attempt{}, domain.ErrNotFound
	}
	return a, err
}

// RecordVerification durably appends one independent verifier bundle.
// Identity, oracle, authority and actor are server-authoritative. An exact
// idempotent redelivery returns the stored bundle without a second event.
func (db *DB) RecordVerification(ctx context.Context, evidence domain.VerificationEvidence, correlationID string) (domain.VerificationEvidence, bool, domain.Event, error) {
	if evidence.Authority != "verifier" {
		return domain.VerificationEvidence{}, false, domain.Event{}, fmt.Errorf("%w: verification authority must be server-assigned verifier",
			domain.ErrUnauthorized)
	}
	if err := validateEvidenceIdentity(evidence.CandidateID, evidence.CandidateSHA, evidence.SpecID,
		evidence.SpecHash, evidence.PolicyRevision, evidence.Actor, evidence.IdempotencyKey); err != nil {
		return domain.VerificationEvidence{}, false, domain.Event{}, err
	}
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.VerificationEvidence{}, false, domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()

	candidate, spec, err := loadCandidateAndSpecTx(ctx, tx, evidence.CandidateID)
	if err != nil {
		return domain.VerificationEvidence{}, false, domain.Event{}, err
	}
	if candidate.State != domain.CandidateFrozen {
		return domain.VerificationEvidence{}, false, domain.Event{}, fmt.Errorf("%w: candidate is %s",
			domain.ErrInvalidTransition, candidate.State)
	}
	if evidence.CandidateSHA != candidate.CommitSHA || evidence.SpecID != candidate.SpecID ||
		evidence.SpecHash != candidate.SpecHash || evidence.PolicyRevision != candidate.PolicyRevision {
		return domain.VerificationEvidence{}, false, domain.Event{}, fmt.Errorf("%w: evidence identity does not match the frozen candidate",
			domain.ErrUnauthorized)
	}
	if evidence.Actor == candidate.ImplementerWorkerID {
		return domain.VerificationEvidence{}, false, domain.Event{}, fmt.Errorf("%w: verifier identity equals the implementer",
			domain.ErrUnauthorized)
	}
	if evidence.OracleDigest != spec.OracleDigest() {
		return domain.VerificationEvidence{}, false, domain.Event{}, fmt.Errorf("%w: evidence oracle does not match the frozen spec",
			domain.ErrUnauthorized)
	}
	if err := enforceCampaignParticipantTx(ctx, tx, evidence.CandidateID, evidence.IdempotencyKey,
		evidence.Actor, domain.RoleVerifier); err != nil {
		return domain.VerificationEvidence{}, false, domain.Event{}, err
	}
	if !evidence.Independent || strings.TrimSpace(evidence.CleanResource) == "" {
		return domain.VerificationEvidence{}, false, domain.Event{}, fmt.Errorf("%w: independent clean-resource evidence is required",
			domain.ErrInvalidInput)
	}
	if len(evidence.CleanResource) > domain.MaxCleanResourceChars ||
		len(evidence.Findings) > maxEvidenceFindings {
		return domain.VerificationEvidence{}, false, domain.Event{}, fmt.Errorf("%w: evidence bounds exceeded",
			domain.ErrInvalidInput)
	}
	if len(evidence.Checks) > maxVerificationChecks {
		return domain.VerificationEvidence{}, false, domain.Event{}, fmt.Errorf("%w: too many check results",
			domain.ErrInvalidInput)
	}

	if existing, rerr := scanVerification(tx.QueryRowContext(ctx,
		`SELECT `+verificationColumns+` FROM candidate_verifications WHERE candidate_id=? AND idempotency_key=?`,
		evidence.CandidateID, evidence.IdempotencyKey)); rerr == nil {
		if cerr := tx.Commit(); cerr != nil {
			return domain.VerificationEvidence{}, false, domain.Event{}, cerr
		}
		return existing, true, domain.Event{}, nil
	} else if rerr != sql.ErrNoRows {
		return domain.VerificationEvidence{}, false, domain.Event{}, rerr
	}

	if hasDecision, derr := hasGateDecisionTx(ctx, tx, evidence.CandidateID); derr != nil {
		return domain.VerificationEvidence{}, false, domain.Event{}, derr
	} else if hasDecision {
		return domain.VerificationEvidence{}, false, domain.Event{}, fmt.Errorf("%w: candidate already has a final Quality Gate decision; create a new candidate",
			domain.ErrInvalidTransition)
	}
	normalized, err := normalizeVerifierChecks(evidence.Checks, spec)
	if err != nil {
		return domain.VerificationEvidence{}, false, domain.Event{}, err
	}
	evidence.Checks = normalized
	evidence.CreatedAt = now
	evidence.Status = aggregateVerificationStatus(normalized)
	evidence.OracleDigest = spec.OracleDigest()
	evidence.ID = newID()
	payloadChecks, err := json.Marshal(evidence.Checks)
	if err != nil {
		return domain.VerificationEvidence{}, false, domain.Event{}, err
	}
	payloadFindings, err := json.Marshal(evidence.Findings)
	if err != nil {
		return domain.VerificationEvidence{}, false, domain.Event{}, err
	}
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO candidate_verifications(id, candidate_id, candidate_sha,
		 spec_id, spec_hash, policy_revision, authority, actor, independent,
		 clean_resource, oracle_digest, status, checks, findings, idempotency_key, created_at)
		 VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`,
		evidence.ID, evidence.CandidateID, evidence.CandidateSHA, evidence.SpecID,
		evidence.SpecHash, evidence.PolicyRevision, evidence.Authority, evidence.Actor,
		boolInt(evidence.Independent), evidence.CleanResource, evidence.OracleDigest,
		string(evidence.Status), string(payloadChecks), string(payloadFindings),
		evidence.IdempotencyKey, stamp(evidence.CreatedAt),
	); err != nil {
		return domain.VerificationEvidence{}, false, domain.Event{}, fmt.Errorf("insert verification: %w", err)
	}
	payload, _ := json.Marshal(map[string]any{
		"evidenceId":   evidence.ID,
		"candidateId":  evidence.CandidateID,
		"candidateSha": evidence.CandidateSHA,
		"authority":    evidence.Authority,
		"actor":        evidence.Actor,
		"status":       evidence.Status,
		"checkCount":   len(evidence.Checks),
		"oracleDigest": evidence.OracleDigest,
	})
	event, err := appendCandidateEvent(ctx, tx, domain.EventVerificationRecorded, evidence.CandidateID, evidence.Actor, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.VerificationEvidence{}, false, domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.VerificationEvidence{}, false, domain.Event{}, err
	}
	stored, err := db.GetVerification(evidence.ID)
	if err != nil {
		return domain.VerificationEvidence{}, false, domain.Event{}, err
	}
	return stored, false, event, nil
}

func normalizeVerifierChecks(checks []domain.CheckResult, spec domain.ActionSpec) ([]domain.CheckResult, error) {
	if len(checks) == 0 {
		return nil, fmt.Errorf("%w: verifier did not submit any check result",
			domain.ErrInvalidInput)
	}
	seen := map[string]bool{}
	out := make([]domain.CheckResult, 0, len(checks))
	for _, result := range checks {
		expected, ok := spec.CheckByID(result.ID)
		if !ok {
			return nil, fmt.Errorf("%w: unknown check id %q", domain.ErrUnauthorized, result.ID)
		}
		if seen[result.ID] {
			return nil, fmt.Errorf("%w: duplicate check id %q", domain.ErrInvalidInput, result.ID)
		}
		seen[result.ID] = true
		if result.Kind != expected.Kind {
			return nil, fmt.Errorf("%w: check %q kind does not match the spec", domain.ErrUnauthorized, result.ID)
		}
		switch result.Status {
		case domain.CheckPass, domain.CheckFail, domain.CheckSkipped,
			domain.CheckNotApplicable, domain.CheckNotTested:
		default:
			return nil, fmt.Errorf("%w: unknown check status %q", domain.ErrInvalidInput, result.Status)
		}
		if result.DurationMs < 0 || result.DurationMs > 3_600_000 {
			return nil, fmt.Errorf("%w: check duration out of range", domain.ErrInvalidInput)
		}
		if len(result.Detail) > domain.MaxCheckDescription {
			return nil, fmt.Errorf("%w: check detail exceeds the bound", domain.ErrInvalidInput)
		}
		out = append(out, domain.CheckResult{
			ID: result.ID, Kind: expected.Kind, Status: result.Status,
			Command:  append([]string(nil), expected.Command...),
			ExitCode: result.ExitCode, DurationMs: result.DurationMs, Detail: result.Detail,
		})
	}
	sort.Slice(out, func(i, j int) bool { return out[i].ID < out[j].ID })
	return out, nil
}

// enforceCampaignParticipantTx binds evidence delivered with a campaign task
// idempotency key to the exact Factory-assigned participant identity. It
// preserves the S4-A standalone evidence path, where no campaign task exists.
func enforceCampaignParticipantTx(ctx context.Context, tx *sql.Tx, candidateID, key, actor string, role domain.TaskRole) error {
	var campaignBound int
	if err := tx.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM campaigns c
		 JOIN candidates cand ON cand.spec_id=c.spec_id
		 WHERE cand.id=?`, candidateID).Scan(&campaignBound); err != nil {
		return err
	}
	if campaignBound == 0 {
		// Standalone S4-A candidate: the scoped role credential remains the
		// authority and there is no campaign participant to bind.
		return nil
	}
	var participant string
	err := tx.QueryRowContext(ctx,
		`SELECT participant_id FROM campaign_tasks
		 WHERE candidate_id=? AND idempotency_key=? AND role=?`,
		candidateID, key, string(role)).Scan(&participant)
	if err == sql.ErrNoRows {
		return fmt.Errorf("%w: campaign evidence has no assigned, claimed task",
			domain.ErrUnauthorized)
	}
	if err != nil {
		return err
	}
	if participant == "" || participant != actor {
		return fmt.Errorf("%w: evidence actor does not match the assigned campaign participant",
			domain.ErrUnauthorized)
	}
	return nil
}

func aggregateVerificationStatus(checks []domain.CheckResult) domain.EvidenceStatus {
	status := domain.EvidencePassed
	for _, check := range checks {
		switch check.Status {
		case domain.CheckFail:
			return domain.EvidenceFailed
		case domain.CheckNotTested:
			status = domain.EvidenceNotTested
		case domain.CheckSkipped, domain.CheckNotApplicable:
			if status == domain.EvidencePassed {
				status = domain.EvidenceBlocked
			}
		}
	}
	if status == domain.EvidencePassed && len(checks) == 0 {
		return domain.EvidenceNotTested
	}
	return status
}

// RecordReview appends one read-only reviewer verdict. Reviewer evidence
// cannot name a different candidate/spec identity or the implementer.
func (db *DB) RecordReview(ctx context.Context, review domain.ReviewEvidence, correlationID string) (domain.ReviewEvidence, bool, domain.Event, error) {
	if review.Authority != "reviewer" {
		return domain.ReviewEvidence{}, false, domain.Event{}, fmt.Errorf("%w: review authority must be server-assigned reviewer",
			domain.ErrUnauthorized)
	}
	if err := validateEvidenceIdentity(review.CandidateID, review.CandidateSHA, review.SpecID,
		review.SpecHash, review.PolicyRevision, review.Actor, review.IdempotencyKey); err != nil {
		return domain.ReviewEvidence{}, false, domain.Event{}, err
	}
	switch review.Verdict {
	case domain.ReviewApprove, domain.ReviewRequestChanges, domain.ReviewBlocked:
	default:
		return domain.ReviewEvidence{}, false, domain.Event{}, fmt.Errorf("%w: unknown review verdict %q",
			domain.ErrInvalidInput, review.Verdict)
	}
	if len(review.Findings) > maxReviewFindings {
		return domain.ReviewEvidence{}, false, domain.Event{}, fmt.Errorf("%w: too many review findings",
			domain.ErrInvalidInput)
	}
	for i := range review.Findings {
		finding := &review.Findings[i]
		finding.ID = strings.TrimSpace(finding.ID)
		finding.Severity = strings.TrimSpace(finding.Severity)
		finding.Message = strings.TrimSpace(finding.Message)
		if finding.ID == "" || len(finding.ID) > 64 || finding.Message == "" ||
			len(finding.Message) > domain.MaxFindingChars {
			return domain.ReviewEvidence{}, false, domain.Event{}, fmt.Errorf("%w: review finding is empty or unbounded",
				domain.ErrInvalidInput)
		}
		switch finding.Severity {
		case "info", "warning", "blocking":
		default:
			return domain.ReviewEvidence{}, false, domain.Event{}, fmt.Errorf("%w: unknown finding severity %q",
				domain.ErrInvalidInput, finding.Severity)
		}
	}
	if review.DiffBytes < 0 || review.DiffBytes > 1<<20 || len(review.DiffStat) > 512 {
		return domain.ReviewEvidence{}, false, domain.Event{}, fmt.Errorf("%w: review diff evidence is out of bounds",
			domain.ErrInvalidInput)
	}
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.ReviewEvidence{}, false, domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()

	candidate, _, err := loadCandidateAndSpecTx(ctx, tx, review.CandidateID)
	if err != nil {
		return domain.ReviewEvidence{}, false, domain.Event{}, err
	}
	if candidate.State != domain.CandidateFrozen {
		return domain.ReviewEvidence{}, false, domain.Event{}, fmt.Errorf("%w: candidate is %s",
			domain.ErrInvalidTransition, candidate.State)
	}
	if review.CandidateSHA != candidate.CommitSHA || review.SpecID != candidate.SpecID ||
		review.SpecHash != candidate.SpecHash || review.PolicyRevision != candidate.PolicyRevision {
		return domain.ReviewEvidence{}, false, domain.Event{}, fmt.Errorf("%w: review identity does not match the frozen candidate",
			domain.ErrUnauthorized)
	}
	if review.Actor == candidate.ImplementerWorkerID {
		return domain.ReviewEvidence{}, false, domain.Event{}, fmt.Errorf("%w: reviewer identity equals the implementer",
			domain.ErrUnauthorized)
	}
	if err := enforceCampaignParticipantTx(ctx, tx, review.CandidateID, review.IdempotencyKey,
		review.Actor, domain.RoleReviewer); err != nil {
		return domain.ReviewEvidence{}, false, domain.Event{}, err
	}
	review.CreatedAt = now
	if existing, rerr := scanReview(tx.QueryRowContext(ctx,
		`SELECT `+reviewColumns+` FROM candidate_reviews WHERE candidate_id=? AND idempotency_key=?`,
		review.CandidateID, review.IdempotencyKey)); rerr == nil {
		if cerr := tx.Commit(); cerr != nil {
			return domain.ReviewEvidence{}, false, domain.Event{}, cerr
		}
		return existing, true, domain.Event{}, nil
	} else if rerr != sql.ErrNoRows {
		return domain.ReviewEvidence{}, false, domain.Event{}, rerr
	}
	if hasDecision, derr := hasGateDecisionTx(ctx, tx, review.CandidateID); derr != nil {
		return domain.ReviewEvidence{}, false, domain.Event{}, derr
	} else if hasDecision {
		return domain.ReviewEvidence{}, false, domain.Event{}, fmt.Errorf("%w: candidate already has a final Quality Gate decision; create a new candidate",
			domain.ErrInvalidTransition)
	}
	review.ID = newID()
	findings, err := json.Marshal(review.Findings)
	if err != nil {
		return domain.ReviewEvidence{}, false, domain.Event{}, err
	}
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO candidate_reviews(id, candidate_id, candidate_sha, spec_id,
		 spec_hash, policy_revision, authority, actor, verdict, findings,
		 diff_stat, diff_bytes, idempotency_key, created_at)
		 VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)`,
		review.ID, review.CandidateID, review.CandidateSHA, review.SpecID,
		review.SpecHash, review.PolicyRevision, review.Authority, review.Actor,
		string(review.Verdict), string(findings), review.DiffStat, review.DiffBytes,
		review.IdempotencyKey, stamp(review.CreatedAt),
	); err != nil {
		return domain.ReviewEvidence{}, false, domain.Event{}, fmt.Errorf("insert review: %w", err)
	}
	payload, _ := json.Marshal(map[string]any{
		"reviewId":     review.ID,
		"candidateId":  review.CandidateID,
		"candidateSha": review.CandidateSHA,
		"authority":    review.Authority,
		"actor":        review.Actor,
		"verdict":      review.Verdict,
		"findingCount": len(review.Findings),
	})
	event, err := appendCandidateEvent(ctx, tx, domain.EventReviewRecorded, review.CandidateID, review.Actor, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.ReviewEvidence{}, false, domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.ReviewEvidence{}, false, domain.Event{}, err
	}
	stored, err := db.GetReview(review.ID)
	if err != nil {
		return domain.ReviewEvidence{}, false, domain.Event{}, err
	}
	return stored, false, event, nil
}

// SaveGateDecision persists exactly one final decision per candidate. A
// repeated evaluation returns the stored decision and never fabricates a
// second authoritative result.
func (db *DB) SaveGateDecision(ctx context.Context, decision domain.GateDecision, correlationID string) (domain.GateDecision, bool, domain.Event, error) {
	if existing, err := db.GetGateDecision(decision.CandidateID); err == nil {
		return existing, true, domain.Event{}, nil
	} else if err != domain.ErrNotFound {
		return domain.GateDecision{}, false, domain.Event{}, err
	}
	if decision.Actor != "core-quality-gate" {
		return domain.GateDecision{}, false, domain.Event{}, fmt.Errorf("%w: gate actor must be Core", domain.ErrUnauthorized)
	}
	switch decision.Status {
	case domain.GateAccepted, domain.GateRepair, domain.GateBlocked,
		domain.GateHumanRequired, domain.GateNotTested:
	default:
		return domain.GateDecision{}, false, domain.Event{}, fmt.Errorf("%w: unknown gate status %q",
			domain.ErrInvalidInput, decision.Status)
	}
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.GateDecision{}, false, domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()

	candidate, err := scanCandidate(tx.QueryRowContext(ctx,
		`SELECT `+candidateColumns+` FROM candidates WHERE id=?`, decision.CandidateID))
	if err != nil {
		if err == sql.ErrNoRows {
			return domain.GateDecision{}, false, domain.Event{}, domain.ErrNotFound
		}
		return domain.GateDecision{}, false, domain.Event{}, err
	}
	if decision.CandidateSHA != candidate.CommitSHA || decision.SpecID != candidate.SpecID ||
		decision.SpecHash != candidate.SpecHash || decision.PolicyRevision != candidate.PolicyRevision {
		return domain.GateDecision{}, false, domain.Event{}, fmt.Errorf("%w: gate decision identity does not match the candidate",
			domain.ErrUnauthorized)
	}
	if existing, rerr := scanDecision(tx.QueryRowContext(ctx,
		`SELECT `+decisionColumns+` FROM gate_decisions WHERE candidate_id=?`,
		decision.CandidateID)); rerr == nil {
		if cerr := tx.Commit(); cerr != nil {
			return domain.GateDecision{}, false, domain.Event{}, cerr
		}
		return existing, true, domain.Event{}, nil
	} else if rerr != sql.ErrNoRows {
		return domain.GateDecision{}, false, domain.Event{}, rerr
	}
	verificationIDs, err := candidateEvidenceIDsTx(ctx, tx, "candidate_verifications", decision.CandidateID)
	if err != nil {
		return domain.GateDecision{}, false, domain.Event{}, err
	}
	reviewIDs, err := candidateEvidenceIDsTx(ctx, tx, "candidate_reviews", decision.CandidateID)
	if err != nil {
		return domain.GateDecision{}, false, domain.Event{}, err
	}
	if len(verificationIDs) == 0 || len(reviewIDs) == 0 {
		return domain.GateDecision{}, false, domain.Event{}, fmt.Errorf("%w: a final decision requires verifier and reviewer evidence",
			domain.ErrInvalidInput)
	}
	if got := quality.EvidenceIDDigest(verificationIDs, reviewIDs); got != decision.EvidenceDigest {
		return domain.GateDecision{}, false, domain.Event{}, fmt.Errorf("%w: evidence changed during gate evaluation; re-evaluate",
			domain.ErrConflict)
	}
	decision.ID = newID()
	decision.CreatedAt = now
	reasons, err := json.Marshal(decision.Reasons)
	if err != nil {
		return domain.GateDecision{}, false, domain.Event{}, err
	}
	checks, err := json.Marshal(decision.Checks)
	if err != nil {
		return domain.GateDecision{}, false, domain.Event{}, err
	}
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO gate_decisions(id, candidate_id, candidate_sha, spec_id,
		 spec_hash, policy_revision, status, evidence_digest, reasons, checks,
		 actor, created_at)
		 VALUES(?,?,?,?,?,?,?,?,?,?,?,?)`,
		decision.ID, decision.CandidateID, decision.CandidateSHA, decision.SpecID,
		decision.SpecHash, decision.PolicyRevision, string(decision.Status),
		decision.EvidenceDigest, string(reasons), string(checks), decision.Actor, stamp(now),
	); err != nil {
		return domain.GateDecision{}, false, domain.Event{}, fmt.Errorf("insert gate decision: %w", err)
	}
	payload, _ := json.Marshal(map[string]any{
		"decisionId":   decision.ID,
		"candidateId":  decision.CandidateID,
		"candidateSha": decision.CandidateSHA,
		"status":       decision.Status,
		"reasonCount":  len(decision.Reasons),
	})
	event, err := appendCandidateEvent(ctx, tx, domain.EventQualityGateDecided, decision.CandidateID, decision.Actor, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.GateDecision{}, false, domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.GateDecision{}, false, domain.Event{}, err
	}
	stored, err := db.GetGateDecision(decision.CandidateID)
	if err != nil {
		return domain.GateDecision{}, false, domain.Event{}, err
	}
	return stored, false, event, nil
}

// GetGateDecision returns the single decision for a candidate.
func (db *DB) GetGateDecision(candidateID string) (domain.GateDecision, error) {
	d, err := scanDecision(db.sql.QueryRow(
		`SELECT `+decisionColumns+` FROM gate_decisions WHERE candidate_id=?`, candidateID))
	if err == sql.ErrNoRows {
		return domain.GateDecision{}, domain.ErrNotFound
	}
	return d, err
}

// GetCandidateBundle returns the candidate, its frozen spec, every durable
// verifier/reviewer record and the optional final decision.
func (db *DB) GetCandidateBundle(candidateID string) (domain.CandidateBundle, error) {
	candidate, err := db.GetCandidate(candidateID)
	if err != nil {
		return domain.CandidateBundle{}, err
	}
	spec, err := db.GetActionSpec(candidate.SpecID)
	if err != nil {
		return domain.CandidateBundle{}, err
	}
	verifications, err := db.ListVerifications(candidateID)
	if err != nil {
		return domain.CandidateBundle{}, err
	}
	if verifications == nil {
		verifications = []domain.VerificationEvidence{}
	}
	reviews, err := db.ListReviews(candidateID)
	if err != nil {
		return domain.CandidateBundle{}, err
	}
	if reviews == nil {
		reviews = []domain.ReviewEvidence{}
	}
	bundle := domain.CandidateBundle{
		Candidate: candidate, Spec: spec, Verifications: verifications, Reviews: reviews,
	}
	if decision, derr := db.GetGateDecision(candidateID); derr == nil {
		bundle.Decision = &decision
	} else if derr != domain.ErrNotFound {
		return domain.CandidateBundle{}, derr
	}
	return bundle, nil
}

// ListVerifications returns one candidate's verifier bundles in append order.
func (db *DB) ListVerifications(candidateID string) ([]domain.VerificationEvidence, error) {
	rows, err := db.sql.Query(
		`SELECT `+verificationColumns+` FROM candidate_verifications WHERE candidate_id=? ORDER BY created_at ASC, id ASC`,
		candidateID)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.VerificationEvidence
	for rows.Next() {
		v, err := scanVerification(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, v)
	}
	return out, rows.Err()
}

// ListReviews returns one candidate's reviewer verdicts in append order.
func (db *DB) ListReviews(candidateID string) ([]domain.ReviewEvidence, error) {
	rows, err := db.sql.Query(
		`SELECT `+reviewColumns+` FROM candidate_reviews WHERE candidate_id=? ORDER BY created_at ASC, id ASC`,
		candidateID)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.ReviewEvidence
	for rows.Next() {
		r, err := scanReview(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// GetVerification returns one verification bundle.
func (db *DB) GetVerification(id string) (domain.VerificationEvidence, error) {
	v, err := scanVerification(db.sql.QueryRow(
		`SELECT `+verificationColumns+` FROM candidate_verifications WHERE id=?`, id))
	if err == sql.ErrNoRows {
		return domain.VerificationEvidence{}, domain.ErrNotFound
	}
	return v, err
}

// GetReview returns one review verdict.
func (db *DB) GetReview(id string) (domain.ReviewEvidence, error) {
	r, err := scanReview(db.sql.QueryRow(
		`SELECT `+reviewColumns+` FROM candidate_reviews WHERE id=?`, id))
	if err == sql.ErrNoRows {
		return domain.ReviewEvidence{}, domain.ErrNotFound
	}
	return r, err
}

func loadCandidateAndSpecTx(ctx context.Context, tx *sql.Tx, candidateID string) (domain.CandidateRevision, domain.ActionSpec, error) {
	candidate, err := scanCandidate(tx.QueryRowContext(ctx,
		`SELECT `+candidateColumns+` FROM candidates WHERE id=?`, candidateID))
	if err != nil {
		if err == sql.ErrNoRows {
			return domain.CandidateRevision{}, domain.ActionSpec{}, domain.ErrNotFound
		}
		return domain.CandidateRevision{}, domain.ActionSpec{}, err
	}
	spec, err := scanSpec(tx.QueryRowContext(ctx,
		`SELECT `+specColumns+` FROM action_specs WHERE id=?`, candidate.SpecID))
	if err != nil {
		return domain.CandidateRevision{}, domain.ActionSpec{}, err
	}
	return candidate, spec, nil
}

func validateEvidenceIdentity(candidateID, candidateSHA, specID, specHash, policyRevision, actor, idempotency string) error {
	if candidateID == "" || candidateSHA == "" || specID == "" || specHash == "" || policyRevision == "" {
		return fmt.Errorf("%w: candidate and spec identity are required", domain.ErrInvalidInput)
	}
	if actor == "" || len(actor) > domain.MaxEvidenceActorChars {
		return fmt.Errorf("%w: evidence actor is required and bounded", domain.ErrInvalidInput)
	}
	if idempotency == "" || len(idempotency) > domain.MaxIdempotencyKeyChars {
		return fmt.Errorf("%w: idempotencyKey is required and bounded", domain.ErrInvalidInput)
	}
	return nil
}

func scanVerification(row interface {
	Scan(dest ...any) error
},
) (domain.VerificationEvidence, error) {
	var v domain.VerificationEvidence
	var independent int
	var status, checks, findings, created string
	if err := row.Scan(
		&v.ID, &v.CandidateID, &v.CandidateSHA, &v.SpecID, &v.SpecHash,
		&v.PolicyRevision, &v.Authority, &v.Actor, &independent, &v.CleanResource,
		&v.OracleDigest, &status, &checks, &findings, &v.IdempotencyKey, &created,
	); err != nil {
		return domain.VerificationEvidence{}, err
	}
	if err := json.Unmarshal([]byte(checks), &v.Checks); err != nil {
		return domain.VerificationEvidence{}, err
	}
	if err := json.Unmarshal([]byte(findings), &v.Findings); err != nil {
		return domain.VerificationEvidence{}, err
	}
	v.Independent = independent != 0
	v.Status = domain.EvidenceStatus(status)
	v.CreatedAt = parseStamp(created)
	return v, nil
}

func scanReview(row interface {
	Scan(dest ...any) error
},
) (domain.ReviewEvidence, error) {
	var r domain.ReviewEvidence
	var verdict, findings, created string
	if err := row.Scan(
		&r.ID, &r.CandidateID, &r.CandidateSHA, &r.SpecID, &r.SpecHash,
		&r.PolicyRevision, &r.Authority, &r.Actor, &verdict, &findings,
		&r.DiffStat, &r.DiffBytes, &r.IdempotencyKey, &created,
	); err != nil {
		return domain.ReviewEvidence{}, err
	}
	if err := json.Unmarshal([]byte(findings), &r.Findings); err != nil {
		return domain.ReviewEvidence{}, err
	}
	r.Verdict = domain.ReviewVerdictValues(verdict)
	r.CreatedAt = parseStamp(created)
	return r, nil
}

func scanDecision(row interface {
	Scan(dest ...any) error
},
) (domain.GateDecision, error) {
	var d domain.GateDecision
	var status, reasons, checks, created string
	if err := row.Scan(
		&d.ID, &d.CandidateID, &d.CandidateSHA, &d.SpecID, &d.SpecHash,
		&d.PolicyRevision, &status, &d.EvidenceDigest, &reasons, &checks,
		&d.Actor, &created,
	); err != nil {
		return domain.GateDecision{}, err
	}
	if err := json.Unmarshal([]byte(reasons), &d.Reasons); err != nil {
		return domain.GateDecision{}, err
	}
	if err := json.Unmarshal([]byte(checks), &d.Checks); err != nil {
		return domain.GateDecision{}, err
	}
	d.Status = domain.GateStatus(status)
	d.CreatedAt = parseStamp(created)
	return d, nil
}

// candidateEvidenceIDsTx lists durable evidence IDs for one candidate. The
// table name is a private constant supplied by this file, never user input.
func candidateEvidenceIDsTx(ctx context.Context, tx *sql.Tx, table, candidateID string) ([]string, error) {
	rows, err := tx.QueryContext(ctx,
		`SELECT id FROM `+table+` WHERE candidate_id=? ORDER BY id`, candidateID)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var ids []string
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			return nil, err
		}
		ids = append(ids, id)
	}
	return ids, rows.Err()
}

func hasGateDecisionTx(ctx context.Context, tx *sql.Tx, candidateID string) (bool, error) {
	var exists bool
	err := tx.QueryRowContext(ctx,
		`SELECT TRUE FROM gate_decisions WHERE candidate_id=?`, candidateID).Scan(&exists)
	if err == sql.ErrNoRows {
		return false, nil
	}
	return err == nil, err
}

func boolInt(value bool) int {
	if value {
		return 1
	}
	return 0
}

// EvaluateGate is the single Core-owned gate path used by both the operator
// route and the campaign orchestrator. It re-reads the complete durable
// evidence set, retries once on a concurrent evidence commit, and persists
// exactly one final decision per candidate.
func (db *DB) EvaluateGate(ctx context.Context, candidateID, correlationID string) (domain.GateDecision, bool, bool, *domain.Event, error) {
	for attempt := 0; attempt < 3; attempt++ {
		if existing, err := db.GetGateDecision(candidateID); err == nil {
			return existing, true, true, nil, nil
		} else if err != domain.ErrNotFound {
			return domain.GateDecision{}, false, false, nil, err
		}
		bundle, err := db.GetCandidateBundle(candidateID)
		if err != nil {
			return domain.GateDecision{}, false, false, nil, err
		}
		run, err := db.GetRun(bundle.Candidate.ImplementerRunID)
		if err != nil {
			return domain.GateDecision{}, false, false, nil, err
		}
		attemptRecord, err := db.GetAttempt(bundle.Candidate.ImplementerRunID, bundle.Candidate.ImplementerAttemptID)
		if err != nil {
			return domain.GateDecision{}, false, false, nil, err
		}
		evaluation := quality.Evaluate(quality.Input{
			Candidate: bundle.Candidate, Spec: bundle.Spec, Run: run, Attempt: attemptRecord,
			Verifications: bundle.Verifications, Reviews: bundle.Reviews,
		})
		if !evaluation.Final {
			return evaluation.Decision, false, false, nil, nil
		}
		decision, replayed, event, err := db.SaveGateDecision(ctx, evaluation.Decision, correlationID)
		if errors.Is(err, domain.ErrConflict) {
			continue
		}
		if err != nil {
			return domain.GateDecision{}, false, false, nil, err
		}
		if replayed {
			return decision, true, true, nil, nil
		}
		return decision, true, false, &event, nil
	}
	return domain.GateDecision{}, false, false, nil,
		fmt.Errorf("%w: evidence kept changing during gate evaluation", domain.ErrConflict)
}
