package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

// runColumns lists runs columns in scan order shared by all readers.
const runColumns = `id, project_id, action_id, workload_kind, workload_input,
	status, attempt_count, max_attempts, timeout_secs, lease_owner,
	lease_generation, lease_expires_at, result, error, created_at, updated_at`

const attemptColumns = `id, run_id, seq, worker_id, lease_generation,
	status, heartbeats, result, error, created_at, updated_at`

const workerColumns = `id, version, status, last_heartbeat, last_run_id,
	created_at, updated_at`

func scanRun(row interface {
	Scan(dest ...any) error
},
) (domain.Run, error) {
	var r domain.Run
	var status, workloadInput, result, leaseExpires, created, updated string
	var actionID sql.NullString
	err := row.Scan(
		&r.ID, &r.ProjectID, &actionID, &r.WorkloadKind, &workloadInput,
		&status, &r.AttemptCount, &r.MaxAttempts, &r.TimeoutSecs, &r.LeaseOwner,
		&r.LeaseGeneration, &leaseExpires, &result, &r.Error, &created, &updated,
	)
	if err != nil {
		return domain.Run{}, err
	}
	r.ActionID = actionID.String
	r.Status = domain.RunStatus(status)
	r.WorkloadInput = mustDecode([]byte(workloadInput))
	r.Result = mustDecode([]byte(result))
	if leaseExpires != "" {
		r.LeaseExpiresAt, _ = time.Parse(time.RFC3339Nano, leaseExpires)
	}
	r.CreatedAt, _ = time.Parse(time.RFC3339Nano, created)
	r.UpdatedAt, _ = time.Parse(time.RFC3339Nano, updated)
	return r, nil
}

func scanAttempt(row interface {
	Scan(dest ...any) error
},
) (domain.Attempt, error) {
	var a domain.Attempt
	var status, result, created, updated string
	err := row.Scan(
		&a.ID, &a.RunID, &a.Seq, &a.WorkerID, &a.LeaseGeneration,
		&status, &a.Heartbeats, &result, &a.Error, &created, &updated,
	)
	if err != nil {
		return domain.Attempt{}, err
	}
	a.Status = domain.AttemptStatus(status)
	a.Result = mustDecode([]byte(result))
	a.CreatedAt, _ = time.Parse(time.RFC3339Nano, created)
	a.UpdatedAt, _ = time.Parse(time.RFC3339Nano, updated)
	return a, nil
}

func scanWorker(row interface {
	Scan(dest ...any) error
},
) (domain.Worker, error) {
	var w domain.Worker
	var status, lastHB, created, updated string
	err := row.Scan(
		&w.ID, &w.Version, &status, &lastHB, &w.LastRunID, &created, &updated,
	)
	if err != nil {
		return domain.Worker{}, err
	}
	w.Status = domain.WorkerStatus(status)
	if lastHB != "" {
		w.LastHeartbeat, _ = time.Parse(time.RFC3339Nano, lastHB)
	}
	w.CreatedAt, _ = time.Parse(time.RFC3339Nano, created)
	w.UpdatedAt, _ = time.Parse(time.RFC3339Nano, updated)
	return w, nil
}

func encodeMap(v map[string]any) string {
	if v == nil {
		return "{}"
	}
	raw, err := json.Marshal(v)
	if err != nil {
		return "{}"
	}
	return string(raw)
}

// CreateRun persists a queued Run and its RunQueued event atomically.
func (db *DB) CreateRun(ctx context.Context, params domain.NewRunParams, correlationID string) (domain.Run, domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Run{}, domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()

	var projectExists bool
	if err := tx.QueryRowContext(ctx, `SELECT TRUE FROM projects WHERE id=?`, params.ProjectID).Scan(&projectExists); err != nil {
		if err == sql.ErrNoRows {
			return domain.Run{}, domain.Event{}, fmt.Errorf("%w: unknown projectId %q", domain.ErrInvalidInput, params.ProjectID)
		}
		return domain.Run{}, domain.Event{}, err
	}
	if params.ActionID != "" {
		var actionExists bool
		if err := tx.QueryRowContext(ctx, `SELECT TRUE FROM actions WHERE id=?`, params.ActionID).Scan(&actionExists); err != nil {
			if err == sql.ErrNoRows {
				return domain.Run{}, domain.Event{}, fmt.Errorf("%w: unknown actionId %q", domain.ErrInvalidInput, params.ActionID)
			}
			return domain.Run{}, domain.Event{}, err
		}
	}

	run := domain.Run{
		ID:            newID(),
		ProjectID:     params.ProjectID,
		ActionID:      params.ActionID,
		WorkloadKind:  params.WorkloadKind,
		WorkloadInput: params.WorkloadInput,
		Status:        domain.RunQueued,
		MaxAttempts:   params.MaxAttempts,
		TimeoutSecs:   params.TimeoutSecs,
		CreatedAt:     now,
		UpdatedAt:     now,
	}
	var actionID any
	if run.ActionID != "" {
		actionID = run.ActionID
	} else {
		actionID = ""
	}
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO runs(id, project_id, action_id, workload_kind, workload_input,
		 status, attempt_count, max_attempts, timeout_secs, lease_owner,
		 lease_generation, lease_expires_at, result, error, created_at, updated_at)
		 VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`,
		run.ID, run.ProjectID, actionID, run.WorkloadKind, encodeMap(run.WorkloadInput),
		string(run.Status), 0, run.MaxAttempts, run.TimeoutSecs, "", 0, "",
		"{}", "", stamp(now), stamp(now),
	); err != nil {
		return domain.Run{}, domain.Event{}, fmt.Errorf("insert run: %w", err)
	}
	payload, _ := json.Marshal(map[string]any{
		"projectId":    run.ProjectID,
		"actionId":     run.ActionID,
		"workloadKind": run.WorkloadKind,
		"maxAttempts":  run.MaxAttempts,
		"timeoutSecs":  run.TimeoutSecs,
	})
	event := domain.Event{
		EventID:       newID(),
		EventType:     domain.EventRunQueued,
		SchemaVersion: domain.EventSchemaVersion,
		AggregateType: domain.AggregateRun,
		AggregateID:   run.ID,
		OccurredAt:    now,
		RecordedAt:    now,
		CorrelationID: correlationID,
		Actor:         ActorLocalOperator,
		Payload:       mustDecode(payload),
	}
	event.Sequence, err = nextSequence(ctx, tx, domain.AggregateRun, run.ID)
	if err != nil {
		return domain.Run{}, domain.Event{}, err
	}
	if err := insertEvent(ctx, tx, event); err != nil {
		return domain.Run{}, domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.Run{}, domain.Event{}, err
	}
	stored, err := db.GetRun(run.ID)
	if err != nil {
		return domain.Run{}, domain.Event{}, err
	}
	return stored, event, nil
}

// GetRun returns one Run by id or domain.ErrNotFound.
func (db *DB) GetRun(id string) (domain.Run, error) {
	run, err := scanRun(db.sql.QueryRow(`SELECT `+runColumns+` FROM runs WHERE id=?`, id))
	if err == sql.ErrNoRows {
		return domain.Run{}, domain.ErrNotFound
	}
	return run, err
}

// ListRuns returns Runs newest first, optionally filtered by status.
func (db *DB) ListRuns(status domain.RunStatus, limit int) ([]domain.Run, error) {
	if limit <= 0 || limit > 200 {
		limit = 100
	}
	q := `SELECT ` + runColumns + ` FROM runs WHERE 1=1`
	var args []any
	if status != "" {
		q += ` AND status=?`
		args = append(args, string(status))
	}
	q += ` ORDER BY created_at DESC, id DESC LIMIT ?`
	args = append(args, limit)
	rows, err := db.sql.Query(q, args...)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.Run
	for rows.Next() {
		r, err := scanRun(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// ListAttempts returns one Run's Attempt history in seq order.
func (db *DB) ListAttempts(runID string) ([]domain.Attempt, error) {
	if _, err := db.GetRun(runID); err != nil {
		return nil, err
	}
	rows, err := db.sql.Query(
		`SELECT `+attemptColumns+` FROM attempts WHERE run_id=? ORDER BY seq ASC`, runID)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.Attempt
	for rows.Next() {
		a, err := scanAttempt(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, a)
	}
	return out, rows.Err()
}

// CountRuns reports how many Runs exist (acceptance oracle).
func (db *DB) CountRuns() (int, error) {
	var n int
	return n, db.sql.QueryRow(`SELECT COUNT(*) FROM runs`).Scan(&n)
}

// ClaimNextRun grants the oldest queued Run to a known Worker. It creates a
// new Attempt, advances the fencing generation and sets the lease atomically.
// found=false means no queued work; it is not an error.
func (db *DB) ClaimNextRun(ctx context.Context, workerID, correlationID string) (domain.Run, domain.Attempt, bool, domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Run{}, domain.Attempt{}, false, domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()

	var registered bool
	if err := tx.QueryRowContext(ctx, `SELECT TRUE FROM workers WHERE id=?`, workerID).Scan(&registered); err != nil {
		if err == sql.ErrNoRows {
			return domain.Run{}, domain.Attempt{}, false, domain.Event{}, domain.ErrNotFound
		}
		return domain.Run{}, domain.Attempt{}, false, domain.Event{}, err
	}

	var run domain.Run
	err = func() error {
		r, err := scanRun(tx.QueryRowContext(ctx,
			`SELECT `+runColumns+` FROM runs WHERE status=? ORDER BY created_at ASC, id ASC LIMIT 1`,
			string(domain.RunQueued)))
		if err != nil {
			return err
		}
		run = r
		return nil
	}()
	if err == sql.ErrNoRows {
		if cerr := tx.Commit(); cerr != nil {
			return domain.Run{}, domain.Attempt{}, false, domain.Event{}, cerr
		}
		return domain.Run{}, domain.Attempt{}, false, domain.Event{}, nil
	}
	if err != nil {
		return domain.Run{}, domain.Attempt{}, false, domain.Event{}, err
	}

	generation := run.LeaseGeneration + 1
	attempt := domain.Attempt{
		ID:              newID(),
		RunID:           run.ID,
		Seq:             run.AttemptCount + 1,
		WorkerID:        workerID,
		LeaseGeneration: generation,
		Status:          domain.AttemptLeased,
		CreatedAt:       now,
		UpdatedAt:       now,
	}
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO attempts(id, run_id, seq, worker_id, lease_generation,
		 status, heartbeats, result, error, created_at, updated_at)
		 VALUES(?,?,?,?,?,?,?,?,?,?,?)`,
		attempt.ID, attempt.RunID, attempt.Seq, attempt.WorkerID, attempt.LeaseGeneration,
		string(attempt.Status), 0, "{}", "", stamp(now), stamp(now),
	); err != nil {
		return domain.Run{}, domain.Attempt{}, false, domain.Event{}, fmt.Errorf("insert attempt: %w", err)
	}
	expires := now.Add(db.hooks.leaseTTL)
	if _, err := tx.ExecContext(ctx,
		`UPDATE runs SET status=?, attempt_count=?, lease_owner=?,
		 lease_generation=?, lease_expires_at=?, updated_at=? WHERE id=?`,
		string(domain.RunLeased), attempt.Seq, workerID,
		generation, stamp(expires), stamp(now), run.ID,
	); err != nil {
		return domain.Run{}, domain.Attempt{}, false, domain.Event{}, err
	}
	payload, _ := json.Marshal(map[string]any{
		"attemptId":  attempt.ID,
		"seq":        attempt.Seq,
		"generation": generation,
		"workerId":   workerID,
		"expiresAt":  stamp(expires),
	})
	event := domain.Event{
		EventID:       newID(),
		EventType:     domain.EventRunLeased,
		SchemaVersion: domain.EventSchemaVersion,
		AggregateType: domain.AggregateRun,
		AggregateID:   run.ID,
		OccurredAt:    now,
		RecordedAt:    now,
		CorrelationID: correlationID,
		Actor:         workerID,
		Payload:       mustDecode(payload),
	}
	event.Sequence, err = nextSequence(ctx, tx, domain.AggregateRun, run.ID)
	if err != nil {
		return domain.Run{}, domain.Attempt{}, false, domain.Event{}, err
	}
	if err := insertEvent(ctx, tx, event); err != nil {
		return domain.Run{}, domain.Attempt{}, false, domain.Event{}, err
	}
	if err := touchWorkerTx(ctx, tx, workerID, run.ID, now); err != nil {
		return domain.Run{}, domain.Attempt{}, false, domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.Run{}, domain.Attempt{}, false, domain.Event{}, err
	}
	updated, err := db.GetRun(run.ID)
	if err != nil {
		return domain.Run{}, domain.Attempt{}, false, domain.Event{}, err
	}
	return updated, attempt, true, event, nil
}

// fence loads the Run and Attempt and enforces the fencing invariants
// shared by heartbeat and report: known identities, current generation,
// non-terminal and non-cancelled Run, non-terminal Attempt. It returns the
// rows for the caller to transition.
func fence(tx *sql.Tx, ctx context.Context, workerID, runID, attemptID string, generation int64) (domain.Run, domain.Attempt, error) {
	run, err := scanRun(tx.QueryRowContext(ctx, `SELECT `+runColumns+` FROM runs WHERE id=?`, runID))
	if err == sql.ErrNoRows {
		return domain.Run{}, domain.Attempt{}, domain.ErrNotFound
	}
	if err != nil {
		return domain.Run{}, domain.Attempt{}, err
	}
	attempt, err := scanAttempt(tx.QueryRowContext(ctx,
		`SELECT `+attemptColumns+` FROM attempts WHERE id=? AND run_id=?`, attemptID, runID))
	if err == sql.ErrNoRows {
		return domain.Run{}, domain.Attempt{}, domain.ErrNotFound
	}
	if err != nil {
		return domain.Run{}, domain.Attempt{}, err
	}
	if run.Status == domain.RunCancelled || attempt.Status == domain.AttemptCancelled {
		return domain.Run{}, domain.Attempt{}, domain.ErrRunCancelled
	}
	if attempt.Status.Terminal() || run.Status.Terminal() {
		// A terminal Attempt/Run never accepts further Worker messages for
		// that generation; equal-generation redelivery is handled by the
		// idempotent replay path in ReportAttempt, never here.
		return domain.Run{}, domain.Attempt{}, domain.ErrStaleGeneration
	}
	if attempt.WorkerID != workerID || run.LeaseOwner != workerID {
		return domain.Run{}, domain.Attempt{}, domain.ErrStaleGeneration
	}
	if attempt.LeaseGeneration != generation || run.LeaseGeneration != generation {
		return domain.Run{}, domain.Attempt{}, domain.ErrStaleGeneration
	}
	return run, attempt, nil
}

// leaseExpired reports whether the Run's lease has lapsed at now.
func leaseExpired(run domain.Run, now time.Time) bool {
	if run.LeaseExpiresAt.IsZero() {
		return false
	}
	return !now.Before(run.LeaseExpiresAt)
}

// HeartbeatLease renews a Worker's lease. The first heartbeat for an
// Attempt moves it (and its Run) to running and emits RunStarted;
// renewals extend the lease snapshot without a new event. It returns the
// updated Run and a non-nil event only when a transition was recorded.
func (db *DB) HeartbeatLease(ctx context.Context, workerID, runID, attemptID string, generation int64, correlationID string) (domain.Run, *domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Run{}, nil, err
	}
	defer func() { _ = tx.Rollback() }()

	if err := ensureWorkerTx(ctx, tx, workerID, now); err != nil {
		return domain.Run{}, nil, err
	}
	run, attempt, err := fence(tx, ctx, workerID, runID, attemptID, generation)
	if err != nil {
		return domain.Run{}, nil, err
	}
	if leaseExpired(run, now) {
		return domain.Run{}, nil, domain.ErrLeaseExpired
	}
	var emitted *domain.Event
	if attempt.Status == domain.AttemptLeased {
		if _, err := tx.ExecContext(ctx,
			`UPDATE attempts SET status=?, heartbeats=heartbeats+1, updated_at=? WHERE id=?`,
			string(domain.AttemptRunning), stamp(now), attempt.ID,
		); err != nil {
			return domain.Run{}, nil, err
		}
		if _, err := tx.ExecContext(ctx,
			`UPDATE runs SET status=?, lease_expires_at=?, updated_at=? WHERE id=?`,
			string(domain.RunRunning), stamp(now.Add(db.hooks.leaseTTL)), stamp(now), run.ID,
		); err != nil {
			return domain.Run{}, nil, err
		}
		payload, _ := json.Marshal(map[string]any{
			"attemptId":  attempt.ID,
			"seq":        attempt.Seq,
			"generation": generation,
			"workerId":   workerID,
		})
		e := domain.Event{
			EventID:       newID(),
			EventType:     domain.EventRunStarted,
			SchemaVersion: domain.EventSchemaVersion,
			AggregateType: domain.AggregateRun,
			AggregateID:   run.ID,
			OccurredAt:    now,
			RecordedAt:    now,
			CorrelationID: correlationID,
			Actor:         workerID,
			Payload:       mustDecode(payload),
		}
		seq, err := nextSequence(ctx, tx, domain.AggregateRun, run.ID)
		if err != nil {
			return domain.Run{}, nil, err
		}
		e.Sequence = seq
		if err := insertEvent(ctx, tx, e); err != nil {
			return domain.Run{}, nil, err
		}
		emitted = &e
	} else {
		if _, err := tx.ExecContext(ctx,
			`UPDATE attempts SET heartbeats=heartbeats+1, updated_at=? WHERE id=?`,
			stamp(now), attempt.ID,
		); err != nil {
			return domain.Run{}, nil, err
		}
		if _, err := tx.ExecContext(ctx,
			`UPDATE runs SET lease_expires_at=?, updated_at=? WHERE id=?`,
			stamp(now.Add(db.hooks.leaseTTL)), stamp(now), run.ID,
		); err != nil {
			return domain.Run{}, nil, err
		}
	}
	if err := touchWorkerTx(ctx, tx, workerID, run.ID, now); err != nil {
		return domain.Run{}, nil, err
	}
	if err := tx.Commit(); err != nil {
		return domain.Run{}, nil, err
	}
	updated, err := db.GetRun(run.ID)
	if err != nil {
		return domain.Run{}, nil, err
	}
	return updated, emitted, nil
}

// ReportAttempt records a Worker's terminal observation for one Attempt.
// Identical redelivery of an already-recorded outcome is an idempotent
// replay (replayed=true, no new event, no state change). Anything else for
// a superseded generation is rejected as stale so an old Attempt can never
// overwrite the authoritative result of a newer one.
func (db *DB) ReportAttempt(ctx context.Context, workerID, runID, attemptID string, generation int64, outcome domain.ReportOutcome, output map[string]any, errMsg, correlationID string) (domain.Run, bool, domain.Event, error) {
	sanitized, err := domain.SanitizeResult(output)
	if err != nil {
		return domain.Run{}, false, domain.Event{}, fmt.Errorf("%w: %v", domain.ErrInvalidInput, err)
	}
	reason := domain.SanitizeReason(errMsg)
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Run{}, false, domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()

	if err := ensureWorkerTx(ctx, tx, workerID, now); err != nil {
		return domain.Run{}, false, domain.Event{}, err
	}
	run, attempt, ferr := fence(tx, ctx, workerID, runID, attemptID, generation)
	if ferr != nil {
		if ferr == domain.ErrStaleGeneration {
			// Idempotent replay: the exact outcome for this generation was
			// already recorded and must not create a second event/effect.
			if replay, rerr := replayIfRecorded(ctx, tx, runID, attemptID, generation, outcome); rerr == nil && replay {
				if cerr := tx.Commit(); cerr != nil {
					return domain.Run{}, false, domain.Event{}, cerr
				}
				updated, gerr := db.GetRun(runID)
				if gerr != nil {
					return domain.Run{}, false, domain.Event{}, gerr
				}
				return updated, true, domain.Event{}, nil
			} else if rerr != nil {
				return domain.Run{}, false, domain.Event{}, rerr
			}
		}
		return domain.Run{}, false, domain.Event{}, ferr
	}
	if leaseExpired(run, now) {
		return domain.Run{}, false, domain.Event{}, domain.ErrLeaseExpired
	}

	switch outcome {
	case domain.OutcomeCompleted:
		if _, err := tx.ExecContext(ctx,
			`UPDATE attempts SET status=?, result=?, error=?, updated_at=? WHERE id=?`,
			string(domain.AttemptSucceeded), encodeMap(sanitized), "", stamp(now), attempt.ID,
		); err != nil {
			return domain.Run{}, false, domain.Event{}, err
		}
		if _, err := tx.ExecContext(ctx,
			`UPDATE runs SET status=?, result=?, error=?, lease_expires_at='', updated_at=? WHERE id=?`,
			string(domain.RunSucceeded), encodeMap(sanitized), "", stamp(now), run.ID,
		); err != nil {
			return domain.Run{}, false, domain.Event{}, err
		}
		payload, _ := json.Marshal(map[string]any{
			"attemptId":  attempt.ID,
			"seq":        attempt.Seq,
			"generation": generation,
			"workerId":   workerID,
			"output":     sanitized,
		})
		reported, err := appendRunEvent(ctx, tx, domain.EventRunCompleted, run.ID, workerID, correlationID, now, mustDecode(payload))
		if err != nil {
			return domain.Run{}, false, domain.Event{}, err
		}
		if err := touchWorkerTx(ctx, tx, workerID, run.ID, now); err != nil {
			return domain.Run{}, false, domain.Event{}, err
		}
		if err := tx.Commit(); err != nil {
			return domain.Run{}, false, domain.Event{}, err
		}
		updated, err := db.GetRun(run.ID)
		if err != nil {
			return domain.Run{}, false, domain.Event{}, err
		}
		return updated, false, reported, nil
	case domain.OutcomeFailed:
		if _, err := tx.ExecContext(ctx,
			`UPDATE attempts SET status=?, error=?, updated_at=? WHERE id=?`,
			string(domain.AttemptFailed), reason, stamp(now), attempt.ID,
		); err != nil {
			return domain.Run{}, false, domain.Event{}, err
		}
		willRetry := run.AttemptCount < run.MaxAttempts
		if willRetry {
			if _, err := tx.ExecContext(ctx,
				`UPDATE runs SET status=?, lease_owner='', lease_expires_at='', error=?, updated_at=? WHERE id=?`,
				string(domain.RunQueued), reason, stamp(now), run.ID,
			); err != nil {
				return domain.Run{}, false, domain.Event{}, err
			}
		} else {
			if _, err := tx.ExecContext(ctx,
				`UPDATE runs SET status=?, lease_expires_at='', error=?, updated_at=? WHERE id=?`,
				string(domain.RunFailed), reason, stamp(now), run.ID,
			); err != nil {
				return domain.Run{}, false, domain.Event{}, err
			}
		}
		payload, _ := json.Marshal(map[string]any{
			"attemptId":  attempt.ID,
			"seq":        attempt.Seq,
			"generation": generation,
			"workerId":   workerID,
			"error":      reason,
			"willRetry":  willRetry,
		})
		failedEvent, err := appendRunEvent(ctx, tx, domain.EventRunFailed, run.ID, workerID, correlationID, now, mustDecode(payload))
		if err != nil {
			return domain.Run{}, false, domain.Event{}, err
		}
		if err := touchWorkerTx(ctx, tx, workerID, run.ID, now); err != nil {
			return domain.Run{}, false, domain.Event{}, err
		}
		if err := tx.Commit(); err != nil {
			return domain.Run{}, false, domain.Event{}, err
		}
		updated, err := db.GetRun(run.ID)
		if err != nil {
			return domain.Run{}, false, domain.Event{}, err
		}
		return updated, false, failedEvent, nil
	default:
		return domain.Run{}, false, domain.Event{}, fmt.Errorf("%w: unknown outcome %q", domain.ErrInvalidInput, outcome)
	}
}

// replayIfRecorded reports whether the exact outcome for an Attempt
// generation is already recorded, enabling idempotent redelivery without
// a second event or state change.
func replayIfRecorded(ctx context.Context, tx *sql.Tx, runID, attemptID string, generation int64, outcome domain.ReportOutcome) (bool, error) {
	attempt, err := scanAttempt(tx.QueryRowContext(ctx,
		`SELECT `+attemptColumns+` FROM attempts WHERE id=? AND run_id=?`, attemptID, runID))
	if err == sql.ErrNoRows {
		return false, domain.ErrNotFound
	}
	if err != nil {
		return false, err
	}
	if attempt.LeaseGeneration != generation {
		return false, nil
	}
	switch outcome {
	case domain.OutcomeCompleted:
		return attempt.Status == domain.AttemptSucceeded, nil
	case domain.OutcomeFailed:
		return attempt.Status == domain.AttemptFailed, nil
	default:
		return false, nil
	}
}

// CancelRun moves a non-terminal Run (and its live Attempt, if any) to
// cancelled. Terminal Runs reject cancellation as a conflict.
func (db *DB) CancelRun(ctx context.Context, runID, correlationID string) (domain.Run, domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Run{}, domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()

	run, err := scanRun(tx.QueryRowContext(ctx, `SELECT `+runColumns+` FROM runs WHERE id=?`, runID))
	if err == sql.ErrNoRows {
		return domain.Run{}, domain.Event{}, domain.ErrNotFound
	}
	if err != nil {
		return domain.Run{}, domain.Event{}, err
	}
	if run.Status.Terminal() {
		return domain.Run{}, domain.Event{}, fmt.Errorf("%w: run is already %s", domain.ErrInvalidTransition, run.Status)
	}
	attemptID := ""
	if run.LeaseOwner != "" {
		var a domain.Attempt
		a, err = scanAttempt(tx.QueryRowContext(ctx,
			`SELECT `+attemptColumns+` FROM attempts WHERE run_id=? AND lease_generation=?`,
			runID, run.LeaseGeneration))
		if err != nil && err != sql.ErrNoRows {
			return domain.Run{}, domain.Event{}, err
		}
		if err == nil && !a.Status.Terminal() {
			attemptID = a.ID
			if _, err := tx.ExecContext(ctx,
				`UPDATE attempts SET status=?, updated_at=? WHERE id=?`,
				string(domain.AttemptCancelled), stamp(now), a.ID,
			); err != nil {
				return domain.Run{}, domain.Event{}, err
			}
		}
	}
	if _, err := tx.ExecContext(ctx,
		`UPDATE runs SET status=?, lease_owner='', lease_expires_at='', updated_at=? WHERE id=?`,
		string(domain.RunCancelled), stamp(now), runID,
	); err != nil {
		return domain.Run{}, domain.Event{}, err
	}
	payload, _ := json.Marshal(map[string]any{
		"attemptId":  attemptID,
		"generation": run.LeaseGeneration,
		"workerId":   run.LeaseOwner,
	})
	cancelledEvent, err := appendRunEvent(ctx, tx, domain.EventRunCancelled, runID, ActorLocalOperator, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.Run{}, domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.Run{}, domain.Event{}, err
	}
	stored, err := db.GetRun(runID)
	if err != nil {
		return domain.Run{}, domain.Event{}, err
	}
	return stored, cancelledEvent, nil
}

// ExpireLeases sweeps timed-out leases and overdue Workers. Expired
// Attempts become expired and their Runs requeue (retries remain) or fail
// (exhausted). Attempts running past timeoutSecs fail as timeouts. Workers
// overdue for a heartbeat become offline. All transitions and their events
// commit atomically; the returned events are published to the hub.
func (db *DB) ExpireLeases(ctx context.Context) ([]domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return nil, err
	}
	defer func() { _ = tx.Rollback() }()

	var emitted []domain.Event
	rows, err := tx.QueryContext(ctx,
		`SELECT `+runColumns+` FROM runs WHERE status IN(?,?)`,
		string(domain.RunLeased), string(domain.RunRunning))
	if err != nil {
		return nil, err
	}
	var leased []domain.Run
	for rows.Next() {
		r, err := scanRun(rows)
		if err != nil {
			_ = rows.Close()
			return nil, err
		}
		leased = append(leased, r)
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return nil, err
	}

	for _, run := range leased {
		attempt, err := scanAttempt(tx.QueryRowContext(ctx,
			`SELECT `+attemptColumns+` FROM attempts WHERE run_id=? AND lease_generation=?`,
			run.ID, run.LeaseGeneration))
		if err == sql.ErrNoRows {
			continue
		}
		if err != nil {
			return nil, err
		}
		if attempt.Status.Terminal() {
			continue
		}
		switch {
		case leaseExpired(run, now):
			e, err := expireAttemptTx(ctx, tx, run, attempt, "lease-expired", now)
			if err != nil {
				return nil, err
			}
			emitted = append(emitted, e)
		case run.Status == domain.RunRunning && now.Sub(attempt.CreatedAt) > time.Duration(run.TimeoutSecs)*time.Second:
			e, err := timeoutAttemptTx(ctx, tx, run, attempt, now)
			if err != nil {
				return nil, err
			}
			emitted = append(emitted, e)
		}
	}

	workers, err := listWorkersTx(ctx, tx)
	if err != nil {
		return nil, err
	}
	for _, w := range workers {
		if w.Status != domain.WorkerOnline {
			continue
		}
		overdue := w.LastHeartbeat.IsZero() || now.Sub(w.LastHeartbeat) > db.hooks.offlineAfter
		// A freshly registered Worker that never heartbeated is overdue
		// only after offlineAfter since registration.
		if w.LastHeartbeat.IsZero() && now.Sub(w.CreatedAt) <= db.hooks.offlineAfter {
			continue
		}
		if !overdue {
			continue
		}
		if _, err := tx.ExecContext(ctx,
			`UPDATE workers SET status=?, updated_at=? WHERE id=?`,
			string(domain.WorkerOffline), stamp(now), w.ID,
		); err != nil {
			return nil, err
		}
		payload, _ := json.Marshal(map[string]any{"reason": "heartbeat-overdue"})
		e, err := appendWorkerEvent(ctx, tx, domain.EventWorkerOffline, w.ID, "core", "", now, mustDecode(payload))
		if err != nil {
			return nil, err
		}
		emitted = append(emitted, e)
	}

	if err := tx.Commit(); err != nil {
		return nil, err
	}
	return emitted, nil
}

// expireAttemptTx records one lease expiry and requeues or fails the Run.
func expireAttemptTx(ctx context.Context, tx *sql.Tx, run domain.Run, attempt domain.Attempt, reason string, now time.Time) (domain.Event, error) {
	if _, err := tx.ExecContext(ctx,
		`UPDATE attempts SET status=?, error=?, updated_at=? WHERE id=?`,
		string(domain.AttemptExpired), reason, stamp(now), attempt.ID,
	); err != nil {
		return domain.Event{}, err
	}
	willRetry := run.AttemptCount < run.MaxAttempts
	if willRetry {
		if _, err := tx.ExecContext(ctx,
			`UPDATE runs SET status=?, lease_owner='', lease_expires_at='', error=?, updated_at=? WHERE id=?`,
			string(domain.RunQueued), reason, stamp(now), run.ID,
		); err != nil {
			return domain.Event{}, err
		}
	} else {
		if _, err := tx.ExecContext(ctx,
			`UPDATE runs SET status=?, lease_expires_at='', error=?, updated_at=? WHERE id=?`,
			string(domain.RunFailed), "retry-exhausted: "+reason, stamp(now), run.ID,
		); err != nil {
			return domain.Event{}, err
		}
	}
	payload, _ := json.Marshal(map[string]any{
		"attemptId":  attempt.ID,
		"seq":        attempt.Seq,
		"generation": attempt.LeaseGeneration,
		"workerId":   attempt.WorkerID,
		"reason":     reason,
		"willRetry":  willRetry,
	})
	return appendWorkerRunEvent(ctx, tx, domain.EventRunExpired, run.ID, payload, now)
}

// timeoutAttemptTx records one execution timeout as a failed Attempt and
// requeues or fails the Run.
func timeoutAttemptTx(ctx context.Context, tx *sql.Tx, run domain.Run, attempt domain.Attempt, now time.Time) (domain.Event, error) {
	const reason = "timeout"
	if _, err := tx.ExecContext(ctx,
		`UPDATE attempts SET status=?, error=?, updated_at=? WHERE id=?`,
		string(domain.AttemptFailed), reason, stamp(now), attempt.ID,
	); err != nil {
		return domain.Event{}, err
	}
	willRetry := run.AttemptCount < run.MaxAttempts
	if willRetry {
		if _, err := tx.ExecContext(ctx,
			`UPDATE runs SET status=?, lease_owner='', lease_expires_at='', error=?, updated_at=? WHERE id=?`,
			string(domain.RunQueued), reason, stamp(now), run.ID,
		); err != nil {
			return domain.Event{}, err
		}
	} else {
		if _, err := tx.ExecContext(ctx,
			`UPDATE runs SET status=?, lease_expires_at='', error=?, updated_at=? WHERE id=?`,
			string(domain.RunFailed), "retry-exhausted: "+reason, stamp(now), run.ID,
		); err != nil {
			return domain.Event{}, err
		}
	}
	payload, _ := json.Marshal(map[string]any{
		"attemptId":  attempt.ID,
		"seq":        attempt.Seq,
		"generation": attempt.LeaseGeneration,
		"workerId":   attempt.WorkerID,
		"error":      reason,
		"willRetry":  willRetry,
	})
	return appendWorkerRunEvent(ctx, tx, domain.EventRunFailed, run.ID, payload, now)
}

// ReconcileOnBoot recovers authoritative state after a Core restart:
// every non-terminal leased/running Run expires its live Attempt
// (reason core-restart) and requeues or fails; every online Worker becomes
// offline until it re-registers. Nothing unfinished disappears silently.
func (db *DB) ReconcileOnBoot(ctx context.Context, correlationID string) ([]domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return nil, err
	}
	defer func() { _ = tx.Rollback() }()

	var emitted []domain.Event
	rows, err := tx.QueryContext(ctx,
		`SELECT `+runColumns+` FROM runs WHERE status IN(?,?)`,
		string(domain.RunLeased), string(domain.RunRunning))
	if err != nil {
		return nil, err
	}
	var live []domain.Run
	for rows.Next() {
		r, err := scanRun(rows)
		if err != nil {
			_ = rows.Close()
			return nil, err
		}
		live = append(live, r)
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return nil, err
	}
	for _, run := range live {
		attempt, err := scanAttempt(tx.QueryRowContext(ctx,
			`SELECT `+attemptColumns+` FROM attempts WHERE run_id=? AND lease_generation=?`,
			run.ID, run.LeaseGeneration))
		if err == sql.ErrNoRows {
			continue
		}
		if err != nil {
			return nil, err
		}
		if attempt.Status.Terminal() {
			continue
		}
		e, err := expireAttemptTx(ctx, tx, run, attempt, "core-restart", now)
		if err != nil {
			return nil, err
		}
		e.CorrelationID = correlationID
		emitted = append(emitted, e)
		// Fix the persisted correlation on the committed event row.
		if _, err := tx.ExecContext(ctx,
			`UPDATE events SET correlation_id=? WHERE event_id=?`, correlationID, e.EventID,
		); err != nil {
			return nil, err
		}
	}

	workers, err := listWorkersTx(ctx, tx)
	if err != nil {
		return nil, err
	}
	for _, w := range workers {
		if w.Status != domain.WorkerOnline {
			continue
		}
		if _, err := tx.ExecContext(ctx,
			`UPDATE workers SET status=?, updated_at=? WHERE id=?`,
			string(domain.WorkerOffline), stamp(now), w.ID,
		); err != nil {
			return nil, err
		}
		payload, _ := json.Marshal(map[string]any{"reason": "core-restart"})
		e, err := appendWorkerEvent(ctx, tx, domain.EventWorkerOffline, w.ID, "core", correlationID, now, mustDecode(payload))
		if err != nil {
			return nil, err
		}
		emitted = append(emitted, e)
	}
	if err := tx.Commit(); err != nil {
		return nil, err
	}
	return emitted, nil
}

// RegisterWorker records a Worker registration idempotently by workerId.
// The first registration emits WorkerRegistered; an offline Worker that
// re-registers emits WorkerRegistered with reconnect=true and returns
// replayed=false; an online re-registration is a replay with no new event.
func (db *DB) RegisterWorker(ctx context.Context, workerID, version, correlationID string) (domain.Worker, *domain.Event, error) {
	if err := domain.ValidateWorkerID(workerID); err != nil {
		return domain.Worker{}, nil, fmt.Errorf("%w: %v", domain.ErrInvalidInput, err)
	}
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Worker{}, nil, err
	}
	defer func() { _ = tx.Rollback() }()

	existing, err := scanWorker(tx.QueryRowContext(ctx,
		`SELECT `+workerColumns+` FROM workers WHERE id=?`, workerID))
	if err != nil && err != sql.ErrNoRows {
		return domain.Worker{}, nil, err
	}
	if err == sql.ErrNoRows {
		if _, err := tx.ExecContext(ctx,
			`INSERT INTO workers(id, version, status, last_heartbeat, last_run_id, created_at, updated_at)
			 VALUES(?,?,?,?,?,?,?)`,
			workerID, version, string(domain.WorkerOnline), stamp(now), "", stamp(now), stamp(now),
		); err != nil {
			return domain.Worker{}, nil, err
		}
		payload, _ := json.Marshal(map[string]any{"version": version})
		registered, err := appendWorkerEvent(ctx, tx, domain.EventWorkerRegistered, workerID, workerID, correlationID, now, mustDecode(payload))
		if err != nil {
			return domain.Worker{}, nil, err
		}
		if err := tx.Commit(); err != nil {
			return domain.Worker{}, nil, err
		}
		w, err := db.GetWorker(workerID)
		if err != nil {
			return domain.Worker{}, nil, err
		}
		return w, &registered, nil
	}

	reconnect := existing.Status != domain.WorkerOnline
	if _, err := tx.ExecContext(ctx,
		`UPDATE workers SET version=?, status=?, last_heartbeat=?, updated_at=? WHERE id=?`,
		version, string(domain.WorkerOnline), stamp(now), stamp(now), workerID,
	); err != nil {
		return domain.Worker{}, nil, err
	}
	if reconnect {
		payload, _ := json.Marshal(map[string]any{"version": version, "reconnect": true})
		reconnected, err := appendWorkerEvent(ctx, tx, domain.EventWorkerRegistered, workerID, workerID, correlationID, now, mustDecode(payload))
		if err != nil {
			return domain.Worker{}, nil, err
		}
		if err := tx.Commit(); err != nil {
			return domain.Worker{}, nil, err
		}
		w, err := db.GetWorker(workerID)
		if err != nil {
			return domain.Worker{}, nil, err
		}
		return w, &reconnected, nil
	}
	if err := tx.Commit(); err != nil {
		return domain.Worker{}, nil, err
	}
	w, err := db.GetWorker(workerID)
	if err != nil {
		return domain.Worker{}, nil, err
	}
	return w, nil, nil
}

// GetWorker returns one Worker or domain.ErrNotFound.
func (db *DB) GetWorker(id string) (domain.Worker, error) {
	w, err := scanWorker(db.sql.QueryRow(`SELECT `+workerColumns+` FROM workers WHERE id=?`, id))
	if err == sql.ErrNoRows {
		return domain.Worker{}, domain.ErrNotFound
	}
	return w, err
}

// ListWorkers returns Workers ordered by id.
func (db *DB) ListWorkers() ([]domain.Worker, error) {
	rows, err := db.sql.Query(`SELECT ` + workerColumns + ` FROM workers ORDER BY id`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.Worker
	for rows.Next() {
		w, err := scanWorker(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, w)
	}
	return out, rows.Err()
}

func listWorkersTx(ctx context.Context, tx *sql.Tx) ([]domain.Worker, error) {
	rows, err := tx.QueryContext(ctx, `SELECT `+workerColumns+` FROM workers ORDER BY id`)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.Worker
	for rows.Next() {
		w, err := scanWorker(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, w)
	}
	return out, rows.Err()
}

// ensureWorkerTx rejects messages from unknown Workers without creating
// phantom registrations: only explicit register creates a Worker row.
func ensureWorkerTx(ctx context.Context, tx *sql.Tx, workerID string, now time.Time) error {
	var exists bool
	if err := tx.QueryRowContext(ctx, `SELECT TRUE FROM workers WHERE id=?`, workerID).Scan(&exists); err != nil {
		if err == sql.ErrNoRows {
			return domain.ErrNotFound
		}
		return err
	}
	return nil
}

// touchWorkerTx records liveness and last assignment for a known Worker.
func touchWorkerTx(ctx context.Context, tx *sql.Tx, workerID, runID string, now time.Time) error {
	_, err := tx.ExecContext(ctx,
		`UPDATE workers SET status=?, last_heartbeat=?, last_run_id=?, updated_at=? WHERE id=?`,
		string(domain.WorkerOnline), stamp(now), runID, stamp(now), workerID,
	)
	return err
}

// appendRunEvent records one event on a run aggregate and returns it.
func appendRunEvent(ctx context.Context, tx *sql.Tx, eventType, runID, actor, correlationID string, now time.Time, payload map[string]any) (domain.Event, error) {
	event := domain.Event{
		EventID:       newID(),
		EventType:     eventType,
		SchemaVersion: domain.EventSchemaVersion,
		AggregateType: domain.AggregateRun,
		AggregateID:   runID,
		OccurredAt:    now,
		RecordedAt:    now,
		CorrelationID: correlationID,
		Actor:         actor,
		Payload:       payload,
	}
	seq, err := nextSequence(ctx, tx, domain.AggregateRun, runID)
	if err != nil {
		return domain.Event{}, err
	}
	event.Sequence = seq
	if err := insertEvent(ctx, tx, event); err != nil {
		return domain.Event{}, err
	}
	return event, nil
}

// appendWorkerRunEvent is expire/timeout's variant: it must return the
// committed event for hub publication.
func appendWorkerRunEvent(ctx context.Context, tx *sql.Tx, eventType, runID string, payload []byte, now time.Time) (domain.Event, error) {
	event := domain.Event{
		EventID:       newID(),
		EventType:     eventType,
		SchemaVersion: domain.EventSchemaVersion,
		AggregateType: domain.AggregateRun,
		AggregateID:   runID,
		OccurredAt:    now,
		RecordedAt:    now,
		Actor:         "core",
		Payload:       mustDecode(payload),
	}
	seq, err := nextSequence(ctx, tx, domain.AggregateRun, runID)
	if err != nil {
		return domain.Event{}, err
	}
	event.Sequence = seq
	if err := insertEvent(ctx, tx, event); err != nil {
		return domain.Event{}, err
	}
	return event, nil
}

// appendWorkerEvent records one event on a worker aggregate and returns it.
func appendWorkerEvent(ctx context.Context, tx *sql.Tx, eventType, workerID, actor, correlationID string, now time.Time, payload map[string]any) (domain.Event, error) {
	event := domain.Event{
		EventID:       newID(),
		EventType:     eventType,
		SchemaVersion: domain.EventSchemaVersion,
		AggregateType: domain.AggregateWorker,
		AggregateID:   workerID,
		OccurredAt:    now,
		RecordedAt:    now,
		CorrelationID: correlationID,
		Actor:         actor,
		Payload:       payload,
	}
	seq, err := nextSequence(ctx, tx, domain.AggregateWorker, workerID)
	if err != nil {
		return domain.Event{}, err
	}
	event.Sequence = seq
	if err := insertEvent(ctx, tx, event); err != nil {
		return domain.Event{}, err
	}
	return event, nil
}
