package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	"github.com/JamesMorales04/software-factory/core/internal/telemetry"
)

// bindingColumns lists paseo_bindings columns in scan order.
const bindingColumns = `attempt_id, run_id, seq, worker_id, lease_generation,
	provider, model, mode, exec_mode, workspace_id, session_id, correlation_key,
	status, error, created_at, updated_at`

// bindingColumnsB is the alias-qualified form for joins against recovery.
const bindingColumnsB = `b.attempt_id, b.run_id, b.seq, b.worker_id, b.lease_generation,
	b.provider, b.model, b.mode, b.exec_mode, b.workspace_id, b.session_id, b.correlation_key,
	b.status, b.error, b.created_at, b.updated_at`

func scanPaseoBinding(row interface {
	Scan(dest ...any) error
}, extra ...any,
) (domain.PaseoBinding, error) {
	var b domain.PaseoBinding
	var status, created, updated string
	dest := []any{
		&b.AttemptID, &b.RunID, &b.Seq, &b.WorkerID, &b.LeaseGeneration,
		&b.Provider, &b.Model, &b.Mode, &b.ExecMode, &b.WorkspaceID, &b.SessionID, &b.CorrelationKey,
		&status, &b.Error, &created, &updated,
	}
	dest = append(dest, extra...)
	if err := row.Scan(dest...); err != nil {
		return domain.PaseoBinding{}, err
	}
	b.Status = domain.PaseoSessionStatus(status)
	b.CreatedAt, _ = time.Parse(time.RFC3339Nano, created)
	b.UpdatedAt, _ = time.Parse(time.RFC3339Nano, updated)
	return b, nil
}

// fenceBinding enforces the lease fencing invariants shared by the session
// routes, then loads the binding row (if any). Cancellation is checked
// inside fence before Attempt existence, so any Worker message for a
// cancelled Run is rejected as cancelled even with an unknown Attempt.
func fenceBinding(tx *sql.Tx, ctx context.Context, workerID, runID, attemptID string, generation int64) (domain.Run, domain.Attempt, domain.PaseoBinding, bool, error) {
	run, attempt, err := fence(tx, ctx, workerID, runID, attemptID, generation)
	if err != nil {
		return domain.Run{}, domain.Attempt{}, domain.PaseoBinding{}, false, err
	}
	binding, err := scanPaseoBinding(tx.QueryRowContext(ctx,
		`SELECT `+bindingColumns+` FROM paseo_bindings WHERE attempt_id=?`, attemptID))
	if err == sql.ErrNoRows {
		return run, attempt, domain.PaseoBinding{}, false, nil
	}
	if err != nil {
		return domain.Run{}, domain.Attempt{}, domain.PaseoBinding{}, false, err
	}
	if binding.RunID != runID || binding.LeaseGeneration != generation {
		return domain.Run{}, domain.Attempt{}, domain.PaseoBinding{}, false, domain.ErrStaleGeneration
	}
	return run, attempt, binding, true, nil
}

// requirePaseoRun rejects session operations for non-Paseo runs. S2
// synthetic runs never carry a session binding.
func requirePaseoRun(run domain.Run) error {
	if run.WorkloadKind != domain.PaseoTaskKind || run.ExecutionMode != string(domain.ExecTrustedLocal) {
		return fmt.Errorf("%w: paseo session binding requires a paseo-task trusted-local run",
			domain.ErrInvalidInput)
	}
	return nil
}

// IntendPaseoSession persists the intent to create a real Paseo session for
// one Attempt generation before any non-idempotent external call. The row
// freezes the Run's requested routing for this generation. A duplicate
// intend with the same identity is an idempotent replay: no new event, no
// state change.
func (db *DB) IntendPaseoSession(ctx context.Context, workerID, runID, attemptID string, generation int64, correlationID string) (domain.PaseoBinding, bool, domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()

	if err := ensureWorkerTx(ctx, tx, workerID, now); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	run, attempt, binding, hasBinding, err := fenceBinding(tx, ctx, workerID, runID, attemptID, generation)
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	if leaseExpired(run, now) {
		return domain.PaseoBinding{}, false, domain.Event{}, domain.ErrLeaseExpired
	}
	if err := requirePaseoRun(run); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	if hasBinding {
		if binding.Status.Terminal() {
			return domain.PaseoBinding{}, false, domain.Event{}, domain.ErrStaleGeneration
		}
		// The intent for this generation is already recorded: replay.
		if cerr := tx.Commit(); cerr != nil {
			return domain.PaseoBinding{}, false, domain.Event{}, cerr
		}
		stored, gerr := db.GetPaseoBinding(attemptID)
		if gerr != nil {
			return domain.PaseoBinding{}, false, domain.Event{}, gerr
		}
		return stored, true, domain.Event{}, nil
	}

	binding = domain.PaseoBinding{
		AttemptID:       attempt.ID,
		RunID:           run.ID,
		Seq:             attempt.Seq,
		WorkerID:        workerID,
		LeaseGeneration: generation,
		Provider:        run.Provider,
		Model:           run.Model,
		Mode:            run.PaseoMode,
		ExecMode:        run.ExecutionMode,
		CorrelationKey:  domain.PaseoCorrelationKey(run.ID, generation),
		Status:          domain.PaseoIntended,
		CreatedAt:       now,
		UpdatedAt:       now,
	}
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO paseo_bindings(attempt_id, run_id, seq, worker_id, lease_generation,
		 provider, model, mode, exec_mode, workspace_id, session_id, correlation_key,
		 status, error, created_at, updated_at)
		 VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`,
		binding.AttemptID, binding.RunID, binding.Seq, binding.WorkerID, binding.LeaseGeneration,
		binding.Provider, binding.Model, binding.Mode, binding.ExecMode, "", "", binding.CorrelationKey,
		string(binding.Status), "", stamp(now), stamp(now),
	); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, fmt.Errorf("insert paseo binding: %w", err)
	}
	payload, _ := json.Marshal(map[string]any{
		"attemptId":  binding.AttemptID,
		"seq":        binding.Seq,
		"generation": generation,
		"workerId":   workerID,
		"provider":   binding.Provider,
		"model":      binding.Model,
		"mode":       binding.Mode,
		"execMode":   binding.ExecMode,
	})
	event, err := appendRunEvent(ctx, tx, domain.EventPaseoSessionIntended, run.ID, workerID, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	if err := touchWorkerTx(ctx, tx, workerID, run.ID, now); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	stored, err := db.GetPaseoBinding(attemptID)
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	return stored, false, event, nil
}

// BindPaseoSession records the proven Paseo workspace/session identity for
// an intended generation. The identity is validated but never trusted over
// the Run's requested routing: the binding froze that routing at intend
// time, and a mismatch is a hard failure, never a substitution. Binding
// requires the Attempt to have started (leased → running via heartbeat),
// preserving the S2 lifecycle.
func (db *DB) BindPaseoSession(ctx context.Context, workerID, runID, attemptID string, generation int64, workspaceID, sessionID, correlationID string) (domain.PaseoBinding, bool, domain.Event, error) {
	if err := domain.ValidatePaseoIdentity("workspaceId", workspaceID); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, fmt.Errorf("%w: %v", domain.ErrInvalidInput, err)
	}
	if err := domain.ValidatePaseoIdentity("sessionId", sessionID); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, fmt.Errorf("%w: %v", domain.ErrInvalidInput, err)
	}
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()

	if err := ensureWorkerTx(ctx, tx, workerID, now); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	run, attempt, binding, hasBinding, err := fenceBinding(tx, ctx, workerID, runID, attemptID, generation)
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	if leaseExpired(run, now) {
		return domain.PaseoBinding{}, false, domain.Event{}, domain.ErrLeaseExpired
	}
	if err := requirePaseoRun(run); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	if !hasBinding {
		return domain.PaseoBinding{}, false, domain.Event{}, fmt.Errorf("%w: no intended session for this attempt; intend before binding",
			domain.ErrInvalidInput)
	}
	if binding.Status.Terminal() {
		return domain.PaseoBinding{}, false, domain.Event{}, domain.ErrStaleGeneration
	}
	if binding.Status == domain.PaseoActive {
		// Identical re-binding replays; a different session identity for
		// the same generation is a fencing conflict: abandon the Attempt.
		if binding.WorkspaceID == workspaceID && binding.SessionID == sessionID {
			if cerr := tx.Commit(); cerr != nil {
				return domain.PaseoBinding{}, false, domain.Event{}, cerr
			}
			stored, gerr := db.GetPaseoBinding(attemptID)
			if gerr != nil {
				return domain.PaseoBinding{}, false, domain.Event{}, gerr
			}
			return stored, true, domain.Event{}, nil
		}
		return domain.PaseoBinding{}, false, domain.Event{}, domain.ErrStaleGeneration
	}
	// One session belongs to exactly one Attempt generation: a sessionId
	// already bound elsewhere — even to a terminal binding — must never be
	// adopted by this generation, or S3-B reconnect could re-attach the
	// wrong generation to a live session. The single-writer store checks
	// inside this transaction, so the check and the bind are atomic.
	var owner string
	if err := tx.QueryRowContext(ctx,
		`SELECT attempt_id FROM paseo_bindings WHERE session_id=? AND attempt_id<>?`,
		sessionID, attempt.ID).Scan(&owner); err != nil && err != sql.ErrNoRows {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	} else if err == nil {
		return domain.PaseoBinding{}, false, domain.Event{}, domain.ErrStaleGeneration
	}
	if attempt.Status != domain.AttemptRunning {
		return domain.PaseoBinding{}, false, domain.Event{}, domain.ErrAttemptNotActive
	}

	if _, err := tx.ExecContext(ctx,
		`UPDATE paseo_bindings SET workspace_id=?, session_id=?, status=?, updated_at=? WHERE attempt_id=?`,
		workspaceID, sessionID, string(domain.PaseoActive), stamp(now), attempt.ID,
	); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	payload, _ := json.Marshal(map[string]any{
		"attemptId":   binding.AttemptID,
		"seq":         binding.Seq,
		"generation":  generation,
		"workerId":    workerID,
		"workspaceId": workspaceID,
		"sessionId":   sessionID,
		"provider":    binding.Provider,
		"model":       binding.Model,
		"mode":        binding.Mode,
	})
	event, err := appendRunEvent(ctx, tx, domain.EventPaseoSessionActive, run.ID, workerID, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	if err := touchWorkerTx(ctx, tx, workerID, run.ID, now); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	stored, err := db.GetPaseoBinding(attemptID)
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	return stored, false, event, nil
}

// ParseSessionOutcome maps a Worker's declared session result to a binding
// status. Succeeding or failing a session that was never bound is invalid;
// cancelling an intended session (before any Paseo call completed) is valid
// and records the intent as cancelled.
type sessionOutcome = domain.PaseoSessionStatus

func parseSessionOutcome(s string) (sessionOutcome, error) {
	switch sessionOutcome(s) {
	case domain.PaseoSucceeded, domain.PaseoFailed, domain.PaseoCancelled:
		return sessionOutcome(s), nil
	default:
		return "", fmt.Errorf("%w: unknown session outcome %q (want succeeded|failed|cancelled)",
			domain.ErrInvalidInput, s)
	}
}

// ClosePaseoSession records the terminal observation for one session
// binding. Identical redelivery of an already-recorded outcome is an
// idempotent replay; anything else for a terminal binding is rejected as
// stale so a superseded observation can never rewrite history.
func (db *DB) ClosePaseoSession(ctx context.Context, workerID, runID, attemptID string, generation int64, outcome, errMsg, correlationID string) (domain.PaseoBinding, bool, domain.Event, error) {
	target, err := parseSessionOutcome(outcome)
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	// Reasons may carry Worker-observed text (in S3-B, Paseo output), so
	// secret shapes are masked at this persistence boundary — the same
	// shape redaction the log boundary applies — before the length bound.
	// S2 report errors keep their frozen semantics; this rule covers only
	// the new S3-A session paths.
	reason := domain.SanitizeReason(telemetry.Redact(errMsg))
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()

	if err := ensureWorkerTx(ctx, tx, workerID, now); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	run, _, binding, hasBinding, ferr := fenceBinding(tx, ctx, workerID, runID, attemptID, generation)
	if ferr != nil {
		if ferr == domain.ErrStaleGeneration {
			// Idempotent replay: the exact outcome for this generation was
			// already recorded and must not create a second event.
			if replay, rerr := replaySessionIfRecorded(ctx, tx, attemptID, generation, target); rerr == nil && replay {
				if cerr := tx.Commit(); cerr != nil {
					return domain.PaseoBinding{}, false, domain.Event{}, cerr
				}
				stored, gerr := db.GetPaseoBinding(attemptID)
				if gerr != nil {
					return domain.PaseoBinding{}, false, domain.Event{}, gerr
				}
				return stored, true, domain.Event{}, nil
			} else if rerr != nil {
				return domain.PaseoBinding{}, false, domain.Event{}, rerr
			}
		}
		return domain.PaseoBinding{}, false, domain.Event{}, ferr
	}
	if leaseExpired(run, now) {
		return domain.PaseoBinding{}, false, domain.Event{}, domain.ErrLeaseExpired
	}
	if err := requirePaseoRun(run); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	if !hasBinding {
		return domain.PaseoBinding{}, false, domain.Event{}, fmt.Errorf("%w: no intended session for this attempt; intend before closing",
			domain.ErrInvalidInput)
	}
	if binding.Status.Terminal() {
		if binding.Status == target {
			if cerr := tx.Commit(); cerr != nil {
				return domain.PaseoBinding{}, false, domain.Event{}, cerr
			}
			stored, gerr := db.GetPaseoBinding(attemptID)
			if gerr != nil {
				return domain.PaseoBinding{}, false, domain.Event{}, gerr
			}
			return stored, true, domain.Event{}, nil
		}
		return domain.PaseoBinding{}, false, domain.Event{}, domain.ErrStaleGeneration
	}
	switch {
	case binding.Status == domain.PaseoActive:
		// Any terminal outcome is valid for a running session.
	case binding.Status == domain.PaseoIntended && target == domain.PaseoCancelled:
		// Cancel before any Paseo call completed: record the intent.
		// Only the owning Worker may take this path — it knows no
		// external call was made.
		event, err := cancelBindingTx(ctx, tx, binding, reason, workerID, correlationID, now)
		if err != nil {
			return domain.PaseoBinding{}, false, domain.Event{}, err
		}
		if err := touchWorkerTx(ctx, tx, workerID, run.ID, now); err != nil {
			return domain.PaseoBinding{}, false, domain.Event{}, err
		}
		if err := tx.Commit(); err != nil {
			return domain.PaseoBinding{}, false, domain.Event{}, err
		}
		stored, err := db.GetPaseoBinding(attemptID)
		if err != nil {
			return domain.PaseoBinding{}, false, domain.Event{}, err
		}
		return stored, false, event, nil
	case binding.Status == domain.PaseoIntended && target == domain.PaseoFailed:
		// Startup failure before binding (e.g. provider unavailable):
		// record the failure honestly with its reason.
	default:
		return domain.PaseoBinding{}, false, domain.Event{}, fmt.Errorf("%w: cannot mark an intended session %q",
			domain.ErrInvalidInput, outcome)
	}

	if _, err := tx.ExecContext(ctx,
		`UPDATE paseo_bindings SET status=?, error=?, updated_at=? WHERE attempt_id=?`,
		string(target), reason, stamp(now), binding.AttemptID,
	); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	var eventType string
	switch target {
	case domain.PaseoSucceeded:
		eventType = domain.EventPaseoSessionSucceeded
	case domain.PaseoFailed:
		eventType = domain.EventPaseoSessionFailed
	default:
		eventType = domain.EventPaseoSessionCancelled
	}
	payload, _ := json.Marshal(map[string]any{
		"attemptId":  binding.AttemptID,
		"seq":        binding.Seq,
		"generation": generation,
		"workerId":   workerID,
		"sessionId":  binding.SessionID,
		"outcome":    string(target),
		"error":      reason,
	})
	event, err := appendRunEvent(ctx, tx, eventType, run.ID, workerID, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	if err := touchWorkerTx(ctx, tx, workerID, run.ID, now); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	stored, err := db.GetPaseoBinding(attemptID)
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	return stored, false, event, nil
}

// MarkWorkerSessionOrphaned records that a Worker discovered a live Paseo
// session it cannot prove stopped (lost connection, fenced generation,
// failed stop). The binding becomes orphaned: kept for cleanup, never
// authoritative.
func (db *DB) MarkWorkerSessionOrphaned(ctx context.Context, workerID, runID, attemptID string, generation int64, reason, correlationID string) (domain.PaseoBinding, bool, domain.Event, error) {
	sanitized := domain.SanitizeReason(telemetry.Redact(reason))
	if sanitized == "" {
		sanitized = "session state unprovable"
	}
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()

	if err := ensureWorkerTx(ctx, tx, workerID, now); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	run, _, binding, hasBinding, err := fenceBinding(tx, ctx, workerID, runID, attemptID, generation)
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	if leaseExpired(run, now) {
		return domain.PaseoBinding{}, false, domain.Event{}, domain.ErrLeaseExpired
	}
	if err := requirePaseoRun(run); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	if !hasBinding {
		return domain.PaseoBinding{}, false, domain.Event{}, fmt.Errorf("%w: no session binding for this attempt",
			domain.ErrInvalidInput)
	}
	if binding.Status.Terminal() {
		if binding.Status == domain.PaseoOrphaned {
			if cerr := tx.Commit(); cerr != nil {
				return domain.PaseoBinding{}, false, domain.Event{}, cerr
			}
			stored, gerr := db.GetPaseoBinding(attemptID)
			if gerr != nil {
				return domain.PaseoBinding{}, false, domain.Event{}, gerr
			}
			return stored, true, domain.Event{}, nil
		}
		return domain.PaseoBinding{}, false, domain.Event{}, domain.ErrStaleGeneration
	}
	event, err := orphanBindingTx(ctx, tx, binding, "worker:"+workerID, sanitized, now)
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	event.CorrelationID = correlationID
	if _, err := tx.ExecContext(ctx,
		`UPDATE events SET correlation_id=? WHERE event_id=?`, correlationID, event.EventID,
	); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	if err := touchWorkerTx(ctx, tx, workerID, run.ID, now); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	stored, err := db.GetPaseoBinding(attemptID)
	if err != nil {
		return domain.PaseoBinding{}, false, domain.Event{}, err
	}
	return stored, false, event, nil
}

// replaySessionIfRecorded reports whether the exact session outcome for an
// Attempt generation is already recorded, enabling idempotent redelivery.
func replaySessionIfRecorded(ctx context.Context, tx *sql.Tx, attemptID string, generation int64, outcome sessionOutcome) (bool, error) {
	binding, err := scanPaseoBinding(tx.QueryRowContext(ctx,
		`SELECT `+bindingColumns+` FROM paseo_bindings WHERE attempt_id=?`, attemptID))
	if err == sql.ErrNoRows {
		return false, domain.ErrNotFound
	}
	if err != nil {
		return false, err
	}
	if binding.LeaseGeneration != generation {
		return false, nil
	}
	return binding.Status == domain.PaseoSessionStatus(outcome), nil
}

// orphanBindingTx marks one non-terminal binding orphaned inside the
// caller's transaction (expiry, restart reconciliation, cancellation) and
// returns the committed event for hub publication.
func orphanBindingTx(ctx context.Context, tx *sql.Tx, binding domain.PaseoBinding, actor, reason string, now time.Time) (domain.Event, error) {
	// Materialize the pending cleanup target in the same transaction as the
	// orphan transition, so the Office projection can show "cleanup pending"
	// before any Worker claims it.
	if _, err := ensureRecoveryTx(ctx, tx, binding, now); err != nil {
		return domain.Event{}, err
	}
	if _, err := tx.ExecContext(ctx,
		`UPDATE paseo_bindings SET status=?, error=?, updated_at=? WHERE attempt_id=?`,
		string(domain.PaseoOrphaned), reason, stamp(now), binding.AttemptID,
	); err != nil {
		return domain.Event{}, err
	}
	payload, _ := json.Marshal(map[string]any{
		"attemptId":  binding.AttemptID,
		"seq":        binding.Seq,
		"generation": binding.LeaseGeneration,
		"workerId":   binding.WorkerID,
		"sessionId":  binding.SessionID,
		"reason":     reason,
	})
	return appendWorkerRunEvent(ctx, tx, domain.EventPaseoSessionOrphaned, binding.RunID, payload, now)
}

// cancelBindingTx cancels a never-acted-on intent inside the caller's
// transaction. Only the owning Worker may use it, via an explicit session
// close for an intent it never acted on; Core-side paths (CancelRun,
// failed reports) orphan instead, because Core cannot prove external state.
func cancelBindingTx(ctx context.Context, tx *sql.Tx, binding domain.PaseoBinding, reason, actor, correlationID string, now time.Time) (domain.Event, error) {
	if _, err := tx.ExecContext(ctx,
		`UPDATE paseo_bindings SET status=?, error=?, updated_at=? WHERE attempt_id=?`,
		string(domain.PaseoCancelled), reason, stamp(now), binding.AttemptID,
	); err != nil {
		return domain.Event{}, err
	}
	payload, _ := json.Marshal(map[string]any{
		"attemptId":  binding.AttemptID,
		"seq":        binding.Seq,
		"generation": binding.LeaseGeneration,
		"workerId":   binding.WorkerID,
		"sessionId":  binding.SessionID,
		"outcome":    string(domain.PaseoCancelled),
		"error":      reason,
	})
	event := domain.Event{
		EventID:       newID(),
		EventType:     domain.EventPaseoSessionCancelled,
		SchemaVersion: domain.EventSchemaVersion,
		AggregateType: domain.AggregateRun,
		AggregateID:   binding.RunID,
		OccurredAt:    now,
		RecordedAt:    now,
		CorrelationID: correlationID,
		Actor:         actor,
		Payload:       mustDecode(payload),
	}
	seq, err := nextSequence(ctx, tx, domain.AggregateRun, binding.RunID)
	if err != nil {
		return domain.Event{}, err
	}
	event.Sequence = seq
	if err := insertEvent(ctx, tx, event); err != nil {
		return domain.Event{}, err
	}
	return event, nil
}

// GetPaseoBinding returns one session binding by Attempt id.
func (db *DB) GetPaseoBinding(attemptID string) (domain.PaseoBinding, error) {
	binding, err := scanPaseoBinding(db.sql.QueryRow(
		`SELECT `+bindingColumns+` FROM paseo_bindings WHERE attempt_id=?`, attemptID))
	if err == sql.ErrNoRows {
		return domain.PaseoBinding{}, domain.ErrNotFound
	}
	return binding, err
}

// GetPaseoBindingBySession returns the binding holding a Paseo session id,
// or ErrNotFound when no Attempt generation ever bound it. S3-B uses this
// to re-attach after a Worker or daemon restart.
func (db *DB) GetPaseoBindingBySession(sessionID string) (domain.PaseoBinding, error) {
	binding, err := scanPaseoBinding(db.sql.QueryRow(
		`SELECT `+bindingColumns+` FROM paseo_bindings WHERE session_id=? AND session_id<>'' ORDER BY created_at DESC LIMIT 1`,
		sessionID))
	if err == sql.ErrNoRows {
		return domain.PaseoBinding{}, domain.ErrNotFound
	}
	return binding, err
}

// ListPaseoBindings returns one Run's session bindings in seq order.
func (db *DB) ListPaseoBindings(runID string) ([]domain.PaseoBinding, error) {
	if _, err := db.GetRun(runID); err != nil {
		return nil, err
	}
	rows, err := db.sql.Query(
		`SELECT `+bindingColumns+` FROM paseo_bindings WHERE run_id=? ORDER BY seq ASC`, runID)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.PaseoBinding
	for rows.Next() {
		b, err := scanPaseoBinding(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, b)
	}
	return out, rows.Err()
}

// ListPaseoSessionSummaries is the read-only cross-Run projection for the
// Office Agents view. It joins each binding to its owning Run status in a
// correlated subquery so a 100-row page never becomes 100 extra queries.
// Live bindings (intended/active) sort first, then the most recently
// updated history. Filters are exact and parameterized; the handler owns
// input-shape validation.
func (db *DB) ListPaseoSessionSummaries(status domain.PaseoSessionStatus, provider string, limit int) ([]domain.PaseoSessionSummary, error) {
	if limit <= 0 || limit > 200 {
		limit = 100
	}
	q := `SELECT ` + bindingColumnsB + `,
		(SELECT status FROM runs WHERE runs.id = b.run_id),
		r.attempt_id, ` + recoveryColumns + `
		FROM paseo_bindings b
		LEFT JOIN paseo_recovery r ON r.attempt_id=b.attempt_id
		WHERE 1=1`
	var args []any
	if status != "" {
		q += ` AND b.status=?`
		args = append(args, string(status))
	}
	if provider != "" {
		q += ` AND b.provider=?`
		args = append(args, provider)
	}
	q += ` ORDER BY (b.status=? OR b.status=?) DESC, b.updated_at DESC, b.attempt_id ASC LIMIT ?`
	args = append(args, string(domain.PaseoIntended), string(domain.PaseoActive), limit)
	rows, err := db.sql.Query(q, args...)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.PaseoSessionSummary
	for rows.Next() {
		s, err := scanSessionSummary(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, s)
	}
	return out, rows.Err()
}

// scanSessionSummary reads one binding, its Run status and the optional
// recovery receipt from a single joined row.
func scanSessionSummary(row interface {
	Scan(dest ...any) error
},
) (domain.PaseoSessionSummary, error) {
	var b domain.PaseoBinding
	var status, created, updated, runStatus string
	var present sql.NullString
	var (
		rStatus, claimOwner, claimToken, claimExpires, nextAttempt, lastObserved sql.NullString
		sessionID, workspaceID, sessionObs, workspaceObs, outcome                sql.NullString
		reason, rCreated, rUpdated                                               sql.NullString
		attempts                                                                 sql.NullInt64
	)
	if err := row.Scan(
		&b.AttemptID, &b.RunID, &b.Seq, &b.WorkerID, &b.LeaseGeneration,
		&b.Provider, &b.Model, &b.Mode, &b.ExecMode, &b.WorkspaceID, &b.SessionID,
		&b.CorrelationKey, &status, &b.Error, &created, &updated, &runStatus,
		&present, &rStatus, &claimOwner, &claimToken, &claimExpires, &attempts, &nextAttempt,
		&lastObserved, &sessionID, &workspaceID, &sessionObs, &workspaceObs,
		&outcome, &reason, &rCreated, &rUpdated,
	); err != nil {
		return domain.PaseoSessionSummary{}, err
	}
	b.Status = domain.PaseoSessionStatus(status)
	b.CreatedAt, _ = time.Parse(time.RFC3339Nano, created)
	b.UpdatedAt, _ = time.Parse(time.RFC3339Nano, updated)
	summary := domain.PaseoSessionSummary{PaseoBinding: b, RunStatus: domain.RunStatus(runStatus)}
	if !present.Valid {
		return summary, nil
	}
	r := &domain.PaseoRecovery{
		AttemptID:         b.AttemptID,
		RunID:             b.RunID,
		LeaseGeneration:   b.LeaseGeneration,
		Status:            domain.PaseoRecoveryStatus(rStatus.String),
		ClaimOwner:        claimOwner.String,
		ClaimToken:        claimToken.String,
		Attempts:          int(attempts.Int64),
		SessionID:         sessionID.String,
		WorkspaceID:       workspaceID.String,
		SessionObserved:   domain.PaseoObservation(sessionObs.String),
		WorkspaceObserved: domain.PaseoObservation(workspaceObs.String),
		LastOutcome:       domain.PaseoRecoveryOutcome(outcome.String),
		Reason:            reason.String,
	}
	r.ClaimExpiresAt = parseStamp(claimExpires.String)
	r.NextAttemptAt = parseStamp(nextAttempt.String)
	r.LastObservedAt = parseStamp(lastObserved.String)
	r.CreatedAt = parseStamp(rCreated.String)
	r.UpdatedAt = parseStamp(rUpdated.String)
	summary.Recovery = r
	return summary, nil
}

// ListOrphanedBindings reports non-terminal bindings whose sessions may
// still exist externally. Bounded for reconciliation probes.
func (db *DB) ListOrphanedBindings(limit int) ([]domain.PaseoBinding, error) {
	if limit <= 0 || limit > 200 {
		limit = 100
	}
	rows, err := db.sql.Query(
		`SELECT `+bindingColumns+` FROM paseo_bindings WHERE status=? ORDER BY updated_at ASC LIMIT ?`,
		string(domain.PaseoOrphaned), limit)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.PaseoBinding
	for rows.Next() {
		b, err := scanPaseoBinding(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, b)
	}
	return out, rows.Err()
}
