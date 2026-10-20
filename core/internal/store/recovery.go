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

// S3-D recovery bounds. The claim is a cleanup lease, not a Run lease; it
// fences concurrent recovery Workers but never changes Run/Attempt/
// generation authority. Retries are bounded and backed off so a transient
// daemon outage does not turn into an unbounded loop.
const (
	RecoveryClaimTTL     = 5 * time.Minute
	RecoveryMaxAttempts  = 8
	RecoveryAbsenceQuiet = 10 * time.Second
)

// recoveryColumns lists paseo_recovery columns in scan order (excluding the
// key/run/generation columns, which come from the binding).
const recoveryColumns = `r.status, r.claim_owner, r.claim_token, r.claim_expires_at,
	r.attempts, r.next_attempt_at, r.last_observed_at, r.session_id,
	r.workspace_id, r.session_observed, r.workspace_observed, r.last_outcome,
	r.reason, r.created_at, r.updated_at`

func scanRecovery(row interface {
	Scan(dest ...any) error
}, attemptID, runID string, generation int64,
) (domain.PaseoRecovery, error) {
	var (
		status, claimOwner, claimToken, claimExpires, nextAttempt, lastObserved string
		sessionID, workspaceID, sessionObs, workspaceObs, outcome               string
		reason, created, updated                                                string
		attempts                                                                int
	)
	if err := row.Scan(&status, &claimOwner, &claimToken, &claimExpires, &attempts, &nextAttempt,
		&lastObserved, &sessionID, &workspaceID, &sessionObs, &workspaceObs,
		&outcome, &reason, &created, &updated); err != nil {
		return domain.PaseoRecovery{}, err
	}
	r := domain.PaseoRecovery{
		AttemptID:         attemptID,
		RunID:             runID,
		LeaseGeneration:   generation,
		Status:            domain.PaseoRecoveryStatus(status),
		ClaimOwner:        claimOwner,
		ClaimToken:        claimToken,
		Attempts:          attempts,
		SessionID:         sessionID,
		WorkspaceID:       workspaceID,
		SessionObserved:   domain.PaseoObservation(sessionObs),
		WorkspaceObserved: domain.PaseoObservation(workspaceObs),
		LastOutcome:       domain.PaseoRecoveryOutcome(outcome),
		Reason:            reason,
	}
	r.ClaimExpiresAt = parseStamp(claimExpires)
	r.NextAttemptAt = parseStamp(nextAttempt)
	r.LastObservedAt = parseStamp(lastObserved)
	r.CreatedAt = parseStamp(created)
	r.UpdatedAt = parseStamp(updated)
	return r, nil
}

func parseStamp(raw string) time.Time {
	if raw == "" {
		return time.Time{}
	}
	t, _ := time.Parse(time.RFC3339Nano, raw)
	return t
}

func stampValue(t time.Time) string {
	if t.IsZero() {
		return ""
	}
	return stamp(t)
}

// ensureRecoveryTx loads or creates the recovery row for one orphaned
// binding inside the caller's transaction.
func ensureRecoveryTx(ctx context.Context, tx *sql.Tx, binding domain.PaseoBinding, now time.Time) (domain.PaseoRecovery, error) {
	r, err := scanRecovery(tx.QueryRowContext(ctx,
		`SELECT `+recoveryColumns+` FROM paseo_recovery r WHERE r.attempt_id=?`,
		binding.AttemptID), binding.AttemptID, binding.RunID, binding.LeaseGeneration)
	if err == nil {
		return r, nil
	}
	if err != sql.ErrNoRows {
		return domain.PaseoRecovery{}, err
	}
	r = domain.PaseoRecovery{
		AttemptID:       binding.AttemptID,
		RunID:           binding.RunID,
		LeaseGeneration: binding.LeaseGeneration,
		Status:          domain.RecoveryPending,
		SessionID:       binding.SessionID,
		WorkspaceID:     binding.WorkspaceID,
		CreatedAt:       now,
		UpdatedAt:       now,
	}
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO paseo_recovery(attempt_id, run_id, lease_generation, status,
		 claim_owner, claim_token, claim_expires_at, attempts, next_attempt_at,
		 last_observed_at, session_id, workspace_id, session_observed,
		 workspace_observed, last_outcome, reason, created_at, updated_at)
		 VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`,
		r.AttemptID, r.RunID, r.LeaseGeneration, string(r.Status),
		"", "", "", 0, "", "", r.SessionID, r.WorkspaceID, "", "", "", "",
		stamp(now), stamp(now),
	); err != nil {
		return domain.PaseoRecovery{}, fmt.Errorf("insert paseo recovery: %w", err)
	}
	return r, nil
}

// ClaimPaseoRecovery grants one orphaned binding to a registered Worker.
// The transaction is the single arbiter: two Workers can never receive the
// same live claim, and terminal receipts are never re-run. found=false is a
// normal empty queue, not an error.
func (db *DB) ClaimPaseoRecovery(ctx context.Context, workerID string) (domain.PaseoRecoveryClaim, bool, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.PaseoRecoveryClaim{}, false, err
	}
	defer func() { _ = tx.Rollback() }()

	if err := ensureWorkerTx(ctx, tx, workerID, now); err != nil {
		return domain.PaseoRecoveryClaim{}, false, err
	}

	binding, err := scanPaseoBinding(tx.QueryRowContext(ctx,
		`SELECT `+bindingColumnsB+`
		 FROM paseo_bindings b
		 LEFT JOIN paseo_recovery r ON r.attempt_id=b.attempt_id
		 WHERE (b.status=?
		        OR (b.status=? AND r.attempt_id IS NOT NULL))
		   AND (r.attempt_id IS NULL
		        OR (r.status IN (?,?) AND (r.next_attempt_at='' OR r.next_attempt_at<=?))
		        OR (r.status=? AND r.claim_expires_at<>'' AND r.claim_expires_at<=?))
		 ORDER BY b.updated_at ASC, b.attempt_id ASC LIMIT 1`,
		string(domain.PaseoOrphaned), string(domain.PaseoSucceeded),
		string(domain.RecoveryPending), string(domain.RecoveryUnconfirmed), stamp(now),
		string(domain.RecoveryClaimed), stamp(now)))
	if err == sql.ErrNoRows {
		if cerr := tx.Commit(); cerr != nil {
			return domain.PaseoRecoveryClaim{}, false, cerr
		}
		return domain.PaseoRecoveryClaim{}, false, nil
	}
	if err != nil {
		return domain.PaseoRecoveryClaim{}, false, err
	}
	recovery, err := ensureRecoveryTx(ctx, tx, binding, now)
	if err != nil {
		return domain.PaseoRecoveryClaim{}, false, err
	}
	// Re-check eligibility after materialization. A terminal receipt must
	// win; a live claim or a not-yet-due retry must not be double-granted.
	if recovery.Status.Terminal() {
		if cerr := tx.Commit(); cerr != nil {
			return domain.PaseoRecoveryClaim{}, false, cerr
		}
		return domain.PaseoRecoveryClaim{}, false, nil
	}
	if recovery.Status == domain.RecoveryClaimed && recovery.ClaimExpiresAt.After(now) {
		if cerr := tx.Commit(); cerr != nil {
			return domain.PaseoRecoveryClaim{}, false, cerr
		}
		return domain.PaseoRecoveryClaim{}, false, nil
	}
	if recovery.Status == domain.RecoveryUnconfirmed && recovery.NextAttemptAt.After(now) {
		if cerr := tx.Commit(); cerr != nil {
			return domain.PaseoRecoveryClaim{}, false, cerr
		}
		return domain.PaseoRecoveryClaim{}, false, nil
	}
	// A Worker that dies before completing must not create an unbounded
	// sequence of cleanup claims. The retry budget is enforced here as well
	// as on unconfirmed completion.
	if recovery.Attempts >= RecoveryMaxAttempts {
		if err := blockExhaustedRecoveryTx(ctx, tx, binding, recovery, now); err != nil {
			return domain.PaseoRecoveryClaim{}, false, err
		}
		if cerr := tx.Commit(); cerr != nil {
			return domain.PaseoRecoveryClaim{}, false, cerr
		}
		return domain.PaseoRecoveryClaim{}, false, nil
	}

	token := newID()
	expires := now.Add(RecoveryClaimTTL)
	if _, err := tx.ExecContext(ctx,
		`UPDATE paseo_recovery SET status=?, claim_owner=?, claim_token=?,
		 claim_expires_at=?, attempts=attempts+1, next_attempt_at='',
		 last_outcome='', updated_at=?
		 WHERE attempt_id=?`,
		string(domain.RecoveryClaimed), workerID, token, stamp(expires),
		stamp(now), binding.AttemptID,
	); err != nil {
		return domain.PaseoRecoveryClaim{}, false, err
	}
	if err := tx.Commit(); err != nil {
		return domain.PaseoRecoveryClaim{}, false, err
	}
	stored, err := db.GetPaseoRecovery(binding.AttemptID)
	if err != nil {
		return domain.PaseoRecoveryClaim{}, false, err
	}
	return domain.PaseoRecoveryClaim{
		Recovery:   stored,
		ClaimToken: token,
		Session:    binding,
	}, true, nil
}

// RecoveryCompletion is the validated completion input shared by the API
// and store. Session/workspace identities are optional for absence but must
// never contradict the binding.
type RecoveryCompletion struct {
	WorkerID          string
	AttemptID         string
	ClaimToken        string
	Outcome           domain.PaseoRecoveryOutcome
	SessionID         string
	WorkspaceID       string
	SessionObserved   domain.PaseoObservation
	WorkspaceObserved domain.PaseoObservation
	Reason            string
}

// CompletePaseoRecovery records one Worker observation. Identical
// redelivery replays without a second event; a stale claim token is
// rejected so a superseded Worker cannot write the receipt. Confirmed and
// absent outcomes require the observation evidence described by the
// contract; an unconfirmed result is retried with bounded backoff until it
// becomes blocked.
func (db *DB) CompletePaseoRecovery(ctx context.Context, input RecoveryCompletion, correlationID string) (domain.PaseoRecovery, bool, domain.Event, error) {
	reason := domain.SanitizeReason(telemetry.Redact(input.Reason))
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.PaseoRecovery{}, false, domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()

	if err := ensureWorkerTx(ctx, tx, input.WorkerID, now); err != nil {
		return domain.PaseoRecovery{}, false, domain.Event{}, err
	}
	binding, err := scanPaseoBinding(tx.QueryRowContext(ctx,
		`SELECT `+bindingColumns+` FROM paseo_bindings WHERE attempt_id=?`, input.AttemptID))
	if err == sql.ErrNoRows {
		return domain.PaseoRecovery{}, false, domain.Event{}, domain.ErrNotFound
	}
	if err != nil {
		return domain.PaseoRecovery{}, false, domain.Event{}, err
	}
	// Orphaned bindings are the normal target. A succeeded binding may also
	// carry a recovery row when its post-close cleanup was unconfirmed or the
	// Worker died before reporting; cleanup verification does not change the
	// authoritative succeeded result.
	if binding.Status != domain.PaseoOrphaned && binding.Status != domain.PaseoSucceeded {
		return domain.PaseoRecovery{}, false, domain.Event{}, domain.ErrStaleGeneration
	}
	// A completion is only meaningful for an existing recovery target. It
	// must not materialize a new one.
	if _, serr := scanRecovery(tx.QueryRowContext(ctx,
		`SELECT `+recoveryColumns+` FROM paseo_recovery r WHERE r.attempt_id=?`, binding.AttemptID),
		binding.AttemptID, binding.RunID, binding.LeaseGeneration); serr == sql.ErrNoRows {
		return domain.PaseoRecovery{}, false, domain.Event{}, domain.ErrNotFound
	} else if serr != nil {
		return domain.PaseoRecovery{}, false, domain.Event{}, serr
	}
	recovery, err := ensureRecoveryTx(ctx, tx, binding, now)
	if err != nil {
		return domain.PaseoRecovery{}, false, domain.Event{}, err
	}
	// Idempotent replay: the same claim token already recorded this exact
	// outcome. No second event and no status re-evaluation.
	if recovery.ClaimOwner == input.WorkerID && recovery.ClaimToken == input.ClaimToken &&
		recovery.LastOutcome == input.Outcome {
		if cerr := tx.Commit(); cerr != nil {
			return domain.PaseoRecovery{}, false, domain.Event{}, cerr
		}
		stored, gerr := db.GetPaseoRecovery(binding.AttemptID)
		if gerr != nil {
			return domain.PaseoRecovery{}, false, domain.Event{}, gerr
		}
		return stored, true, domain.Event{}, nil
	}
	if recovery.Status != domain.RecoveryClaimed || recovery.ClaimOwner != input.WorkerID ||
		recovery.ClaimToken != input.ClaimToken ||
		(!recovery.ClaimExpiresAt.IsZero() && !now.Before(recovery.ClaimExpiresAt)) {
		return domain.PaseoRecovery{}, false, domain.Event{}, domain.ErrStaleRecoveryClaim
	}

	// Identity checks: a completion may name a discovered resource, but it
	// may never contradict the frozen binding or adopt a resource owned by
	// another Attempt.
	if input.SessionID != "" && binding.SessionID != "" && input.SessionID != binding.SessionID {
		return domain.PaseoRecovery{}, false, domain.Event{}, domain.ErrRecoveryIdentityConflict
	}
	if input.WorkspaceID != "" && binding.WorkspaceID != "" && input.WorkspaceID != binding.WorkspaceID {
		return domain.PaseoRecovery{}, false, domain.Event{}, domain.ErrRecoveryIdentityConflict
	}
	if input.SessionID != "" {
		conflict, cerr := bindingOwnsTx(ctx, tx, "session_id", input.SessionID, binding.AttemptID)
		if cerr != nil {
			return domain.PaseoRecovery{}, false, domain.Event{}, cerr
		}
		if conflict {
			return domain.PaseoRecovery{}, false, domain.Event{}, domain.ErrRecoveryIdentityConflict
		}
	}
	if input.WorkspaceID != "" {
		conflict, cerr := bindingOwnsTx(ctx, tx, "workspace_id", input.WorkspaceID, binding.AttemptID)
		if cerr != nil {
			return domain.PaseoRecovery{}, false, domain.Event{}, cerr
		}
		if conflict {
			return domain.PaseoRecovery{}, false, domain.Event{}, domain.ErrRecoveryIdentityConflict
		}
	}

	status, next := recoveryTransition(recovery, input, now)
	if status == domain.RecoveryBlocked && input.Outcome == domain.RecoveryOutcomeUnconfirmed {
		if reason == "" {
			reason = "retry-exhausted: bounded retry budget exhausted"
		} else {
			reason = domain.SanitizeReason("retry-exhausted: " + reason)
		}
	}
	sessionID := firstNonEmpty(input.SessionID, recovery.SessionID, binding.SessionID)
	workspaceID := firstNonEmpty(input.WorkspaceID, recovery.WorkspaceID, binding.WorkspaceID)
	if _, err := tx.ExecContext(ctx,
		`UPDATE paseo_recovery SET status=?, claim_owner=?, claim_expires_at='',
		 next_attempt_at=?, last_observed_at=?, session_id=?, workspace_id=?,
		 session_observed=?, workspace_observed=?, last_outcome=?, reason=?, updated_at=?
		 WHERE attempt_id=?`,
		string(status), input.WorkerID, stampValue(next), stamp(now), sessionID, workspaceID,
		string(input.SessionObserved), string(input.WorkspaceObserved), string(input.Outcome),
		reason, stamp(now), binding.AttemptID,
	); err != nil {
		return domain.PaseoRecovery{}, false, domain.Event{}, err
	}
	event, err := recordRecoveryEvent(ctx, tx, binding, input, recovery.Attempts, reason, correlationID, now)
	if err != nil {
		return domain.PaseoRecovery{}, false, domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.PaseoRecovery{}, false, domain.Event{}, err
	}
	stored, err := db.GetPaseoRecovery(binding.AttemptID)
	if err != nil {
		return domain.PaseoRecovery{}, false, domain.Event{}, err
	}
	return stored, false, event, nil
}

// blockExhaustedRecoveryTx records the terminal retry-exhausted receipt when
// a cleanup claim budget is used up by Workers that never completed.
func blockExhaustedRecoveryTx(ctx context.Context, tx *sql.Tx, binding domain.PaseoBinding, recovery domain.PaseoRecovery, now time.Time) error {
	const reason = "retry-exhausted: cleanup claim budget exhausted before a verified completion"
	if _, err := tx.ExecContext(ctx,
		`UPDATE paseo_recovery SET status=?, claim_owner='', claim_token='',
		 claim_expires_at='', next_attempt_at='', last_outcome=?, reason=?, updated_at=?
		 WHERE attempt_id=?`,
		string(domain.RecoveryBlocked), string(domain.RecoveryOutcomeBlocked), reason,
		stamp(now), binding.AttemptID,
	); err != nil {
		return err
	}
	retryCount := recovery.Attempts - 1
	if retryCount < 0 {
		retryCount = 0
	}
	payload, _ := json.Marshal(map[string]any{
		"attemptId":         binding.AttemptID,
		"seq":               binding.Seq,
		"generation":        binding.LeaseGeneration,
		"workerId":          "",
		"outcome":           string(domain.RecoveryOutcomeBlocked),
		"sessionId":         firstNonEmpty(recovery.SessionID, binding.SessionID),
		"workspaceId":       firstNonEmpty(recovery.WorkspaceID, binding.WorkspaceID),
		"sessionObserved":   string(recovery.SessionObserved),
		"workspaceObserved": string(recovery.WorkspaceObserved),
		"retryCount":        retryCount,
		"reason":            reason,
	})
	_, err := appendWorkerRunEvent(ctx, tx, domain.EventPaseoSessionRecoveryRecorded, binding.RunID, payload, now)
	return err
}

func bindingOwnsTx(ctx context.Context, tx *sql.Tx, column, value, attemptID string) (bool, error) {
	// column is a private constant supplied by this file, never user input.
	var owner string
	err := tx.QueryRowContext(ctx,
		`SELECT attempt_id FROM paseo_bindings WHERE `+column+`=? AND attempt_id<>?`,
		value, attemptID).Scan(&owner)
	if err == sql.ErrNoRows {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	return true, nil
}

// recoveryTransition maps one observation to the next durable status.
// Absence is only terminal after two authoritative checks separated by the
// quiet period; a transient failure is retried until the bounded budget is
// exhausted and then becomes blocked.
func recoveryTransition(current domain.PaseoRecovery, input RecoveryCompletion, now time.Time) (domain.PaseoRecoveryStatus, time.Time) {
	switch input.Outcome {
	case domain.RecoveryOutcomeConfirmed:
		return domain.RecoveryConfirmed, time.Time{}
	case domain.RecoveryOutcomeBlocked:
		return domain.RecoveryBlocked, time.Time{}
	case domain.RecoveryOutcomeAbsent:
		if current.SessionObserved == domain.PaseoObservedAbsent &&
			current.WorkspaceObserved == domain.PaseoObservedAbsent &&
			!current.LastObservedAt.IsZero() &&
			now.Sub(current.LastObservedAt) >= RecoveryAbsenceQuiet {
			return domain.RecoveryAbsent, time.Time{}
		}
		return domain.RecoveryUnconfirmed, now.Add(RecoveryAbsenceQuiet)
	default:
		if current.Attempts >= RecoveryMaxAttempts {
			return domain.RecoveryBlocked, time.Time{}
		}
		return domain.RecoveryUnconfirmed, now.Add(recoveryBackoff(current.Attempts))
	}
}

func recoveryBackoff(attempts int) time.Duration {
	if attempts < 1 {
		attempts = 1
	}
	d := 2 * time.Second
	for i := 1; i < attempts; i++ {
		d *= 2
		if d >= 5*time.Minute {
			return 5 * time.Minute
		}
	}
	return d
}

func recordRecoveryEvent(ctx context.Context, tx *sql.Tx, binding domain.PaseoBinding, input RecoveryCompletion, attempts int, reason, correlationID string, now time.Time) (domain.Event, error) {
	retryCount := attempts - 1
	if retryCount < 0 {
		retryCount = 0
	}
	payload, _ := json.Marshal(map[string]any{
		"attemptId":         binding.AttemptID,
		"seq":               binding.Seq,
		"generation":        binding.LeaseGeneration,
		"workerId":          input.WorkerID,
		"outcome":           string(input.Outcome),
		"sessionId":         firstNonEmpty(input.SessionID, binding.SessionID),
		"workspaceId":       firstNonEmpty(input.WorkspaceID, binding.WorkspaceID),
		"sessionObserved":   string(input.SessionObserved),
		"workspaceObserved": string(input.WorkspaceObserved),
		"retryCount":        retryCount,
		"reason":            reason,
	})
	event, err := appendWorkerRunEvent(ctx, tx, domain.EventPaseoSessionRecoveryRecorded, binding.RunID, payload, now)
	if err != nil {
		return domain.Event{}, err
	}
	event.CorrelationID = correlationID
	if _, err := tx.ExecContext(ctx,
		`UPDATE events SET correlation_id=? WHERE event_id=?`, correlationID, event.EventID,
	); err != nil {
		return domain.Event{}, err
	}
	return event, nil
}

func firstNonEmpty(values ...string) string {
	for _, v := range values {
		if v != "" {
			return v
		}
	}
	return ""
}

// GetPaseoRecovery returns one recovery receipt (ErrNotFound when the
// binding exists but recovery has not been materialized).
func (db *DB) GetPaseoRecovery(attemptID string) (domain.PaseoRecovery, error) {
	binding, err := scanPaseoBinding(db.sql.QueryRow(
		`SELECT `+bindingColumns+` FROM paseo_bindings WHERE attempt_id=?`, attemptID))
	if err == sql.ErrNoRows {
		return domain.PaseoRecovery{}, domain.ErrNotFound
	}
	if err != nil {
		return domain.PaseoRecovery{}, err
	}
	r, err := scanRecovery(db.sql.QueryRow(
		`SELECT `+recoveryColumns+` FROM paseo_recovery r WHERE r.attempt_id=?`, attemptID),
		binding.AttemptID, binding.RunID, binding.LeaseGeneration)
	if err == sql.ErrNoRows {
		return domain.PaseoRecovery{}, domain.ErrNotFound
	}
	return r, err
}

// paseoRetryGateTx reports whether a queued paseo-task Run may be leased.
func paseoRetryGateTx(ctx context.Context, tx *sql.Tx, runID string) (allow bool, blockedReason string, err error) {
	prior, err := scanPaseoBinding(tx.QueryRowContext(ctx,
		`SELECT `+bindingColumns+` FROM paseo_bindings WHERE run_id=? ORDER BY seq DESC LIMIT 1`, runID))
	if err == sql.ErrNoRows {
		return true, "", nil
	}
	if err != nil {
		return false, "", err
	}
	if prior.Status == domain.PaseoFailed || prior.Status == domain.PaseoCancelled || prior.Status == domain.PaseoSucceeded {
		return true, "", nil
	}
	r, err := scanRecovery(tx.QueryRowContext(ctx,
		`SELECT `+recoveryColumns+` FROM paseo_recovery r WHERE r.attempt_id=?`, prior.AttemptID),
		prior.AttemptID, prior.RunID, prior.LeaseGeneration)
	if err == sql.ErrNoRows {
		return false, "", nil
	}
	if err != nil {
		return false, "", err
	}
	if r.Status.SafeForReplacement() {
		return true, "", nil
	}
	if r.Status == domain.RecoveryBlocked {
		reason := r.Reason
		if reason == "" {
			reason = "external session state unresolved"
		}
		return false, fmt.Sprintf("recovery-unresolved: attempt %s generation %d: %s", prior.AttemptID, prior.LeaseGeneration, reason), nil
	}
	return false, "", nil
}

// ResolveBlockedPaseoRetries fails closed only the Runs whose prior
// generation can no longer be reconciled automatically. Other Runs and
// projects continue. It is called by the Core sweeper so a queued Run does
// not wait forever after recovery is blocked.
func (db *DB) ResolveBlockedPaseoRetries(ctx context.Context) ([]domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return nil, err
	}
	defer func() { _ = tx.Rollback() }()

	// Filter terminal blocked recovery in SQL so pending/unconfirmed rows
	// cannot occupy a fixed sweep prefix and postpone an older failure
	// forever. Each sweep handles a bounded 200 blocked Runs; later sweeps
	// continue if more remain.
	rows, err := tx.QueryContext(ctx,
		`SELECT `+runColumnsR+` FROM runs r
		 WHERE r.status=? AND r.workload_kind=? AND r.attempt_count>0
		   AND EXISTS (
		     SELECT 1 FROM paseo_bindings b
		     JOIN paseo_recovery rec ON rec.attempt_id=b.attempt_id
		     WHERE b.run_id=r.id
		       AND b.seq=(SELECT MAX(b2.seq) FROM paseo_bindings b2 WHERE b2.run_id=r.id)
		       AND b.status=?
		       AND rec.status=?
		   )
		 ORDER BY r.created_at ASC, r.id ASC LIMIT 200`,
		string(domain.RunQueued), domain.PaseoTaskKind,
		string(domain.PaseoOrphaned), string(domain.RecoveryBlocked))
	if err != nil {
		return nil, err
	}
	var candidates []domain.Run
	for rows.Next() {
		r, serr := scanRun(rows)
		if serr != nil {
			_ = rows.Close()
			return nil, serr
		}
		candidates = append(candidates, r)
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return nil, err
	}

	var emitted []domain.Event
	for _, run := range candidates {
		allow, reason, gerr := paseoRetryGateTx(ctx, tx, run.ID)
		if gerr != nil {
			return nil, gerr
		}
		if allow || reason == "" {
			continue
		}
		prior, perr := scanPaseoBinding(tx.QueryRowContext(ctx,
			`SELECT `+bindingColumns+` FROM paseo_bindings WHERE run_id=? ORDER BY seq DESC LIMIT 1`, run.ID))
		if perr != nil && perr != sql.ErrNoRows {
			return nil, perr
		}
		if _, err := tx.ExecContext(ctx,
			`UPDATE runs SET status=?, lease_owner='', lease_expires_at='', error=?, updated_at=? WHERE id=?`,
			string(domain.RunFailed), reason, stamp(now), run.ID,
		); err != nil {
			return nil, err
		}
		payload, _ := json.Marshal(map[string]any{
			"attemptId":  prior.AttemptID,
			"seq":        prior.Seq,
			"generation": prior.LeaseGeneration,
			"workerId":   prior.WorkerID,
			"error":      reason,
			"willRetry":  false,
		})
		failed, err := appendWorkerRunEvent(ctx, tx, domain.EventRunFailed, run.ID, payload, now)
		if err != nil {
			return nil, err
		}
		emitted = append(emitted, failed)
	}
	if err := tx.Commit(); err != nil {
		return nil, err
	}
	return emitted, nil
}
