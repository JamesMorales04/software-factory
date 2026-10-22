package store

import (
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

// LeadRequest is the durable reservation made before any model assessment is
// started. RequestJSON preserves the caller's complete request envelope;
// RequestHash also binds the project configuration and delegation revision.
type LeadRequest struct {
	ID                string    `json:"id"`
	ProjectID         string    `json:"projectId"`
	ActionID          string    `json:"actionId,omitempty"`
	Objective         string    `json:"objective"`
	RequestHash       string    `json:"requestHash"`
	RequestJSON       string    `json:"request"`
	IdempotencyKey    string    `json:"idempotencyKey"`
	Status            string    `json:"status"`
	DelegationID      string    `json:"delegationId"`
	ProjectConfigHash string    `json:"projectConfigHash"`
	AssessmentID      string    `json:"assessmentId,omitempty"`
	ProposalID        string    `json:"proposalId,omitempty"`
	FrozenPlanID      string    `json:"frozenPlanId,omitempty"`
	SpecID            string    `json:"specId,omitempty"`
	CampaignID        string    `json:"campaignId,omitempty"`
	FailureReason     string    `json:"failureReason,omitempty"`
	CreatedAt         time.Time `json:"createdAt"`
	UpdatedAt         time.Time `json:"updatedAt"`
}

type NewLeadRequest struct {
	ProjectID         string
	Objective         string
	RequestJSON       string
	IdempotencyKey    string
	DelegationID      string
	ProjectConfigHash string
}

type LeadAssessmentRecord struct {
	ID             string    `json:"id"`
	RequestID      string    `json:"requestId"`
	ProjectID      string    `json:"projectId"`
	ActionID       string    `json:"actionId"`
	Mechanism      string    `json:"mechanism"`
	AssessmentJSON string    `json:"assessment"`
	RouteJSON      string    `json:"modelRoute"`
	AssessmentHash string    `json:"assessmentHash"`
	IdempotencyKey string    `json:"idempotencyKey"`
	CreatedAt      time.Time `json:"createdAt"`
}

type LeadProposalRecord struct {
	ID             string                  `json:"id"`
	RequestID      string                  `json:"requestId"`
	ProjectID      string                  `json:"projectId"`
	AssessmentID   string                  `json:"assessmentId"`
	Proposal       domain.LeadPlanProposal `json:"proposal"`
	ContentHash    string                  `json:"contentHash"`
	IdempotencyKey string                  `json:"idempotencyKey"`
	CreatedAt      time.Time               `json:"createdAt"`
}

type DelegationUsage struct {
	DelegationID     string    `json:"delegationId"`
	ReservedRequests int       `json:"reservedRequests"`
	ReservedSessions int       `json:"reservedSessions"`
	ReservedAttempts int       `json:"reservedAttempts"`
	UpdatedAt        time.Time `json:"updatedAt"`
}

type FrozenLeadPlanRecord struct {
	ID                    string    `json:"id"`
	RequestID             string    `json:"requestId"`
	ProjectID             string    `json:"projectId"`
	ProposalID            string    `json:"proposalId"`
	ProposalHash          string    `json:"proposalHash"`
	DelegationID          string    `json:"delegationId"`
	DelegationHash        string    `json:"delegationHash"`
	ProjectConfigRevision int       `json:"projectConfigRevision"`
	ProjectConfigHash     string    `json:"projectConfigHash"`
	PlanHash              string    `json:"planHash"`
	IdempotencyKey        string    `json:"idempotencyKey"`
	CreatedAt             time.Time `json:"createdAt"`
}

// CreateLeadRequest durably reserves an idempotency key before model work.
// Replays with the same digest return the existing reservation; reusing a key
// for a different request is a conflict.
func (db *DB) CreateLeadRequest(ctx context.Context, input NewLeadRequest) (LeadRequest, bool, error) {
	input.ProjectID = strings.TrimSpace(input.ProjectID)
	input.Objective = strings.TrimSpace(input.Objective)
	input.IdempotencyKey = strings.TrimSpace(input.IdempotencyKey)
	input.DelegationID = strings.TrimSpace(input.DelegationID)
	input.ProjectConfigHash = strings.TrimSpace(input.ProjectConfigHash)
	if err := domain.ValidateProjectID(input.ProjectID); err != nil {
		return LeadRequest{}, false, fmt.Errorf("%w: %v", domain.ErrInvalidInput, err)
	}
	if input.Objective == "" || len(input.Objective) > domain.MaxSpecGoalChars {
		return LeadRequest{}, false, fmt.Errorf("%w: objective is required and bounded", domain.ErrInvalidInput)
	}
	if input.IdempotencyKey == "" || len(input.IdempotencyKey) > domain.MaxIdempotencyKeyChars {
		return LeadRequest{}, false, fmt.Errorf("%w: idempotency key is required and bounded", domain.ErrInvalidInput)
	}
	if !isLeadID(input.DelegationID) {
		return LeadRequest{}, false, fmt.Errorf("%w: delegationId must be a 32-character lowercase hex id", domain.ErrInvalidInput)
	}
	requestJSON, err := canonicalJSON(input.RequestJSON)
	if err != nil {
		return LeadRequest{}, false, fmt.Errorf("%w: request must be valid JSON: %v", domain.ErrInvalidInput, err)
	}
	input.RequestJSON = string(requestJSON)
	digest, err := hashJSON(struct {
		ProjectID, Objective, RequestJSON, DelegationID, ProjectConfigHash string
	}{input.ProjectID, input.Objective, input.RequestJSON, input.DelegationID, input.ProjectConfigHash})
	if err != nil {
		return LeadRequest{}, false, err
	}
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return LeadRequest{}, false, err
	}
	defer func() { _ = tx.Rollback() }()
	if existing, qerr := scanLeadRequest(tx.QueryRowContext(ctx,
		`SELECT `+leadRequestColumns+` FROM lead_requests WHERE idempotency_key=?`, input.IdempotencyKey)); qerr == nil {
		if existing.RequestHash != digest {
			return LeadRequest{}, false, fmt.Errorf("%w: idempotency key is bound to a different lead request", domain.ErrConflict)
		}
		if err := tx.Commit(); err != nil {
			return LeadRequest{}, false, err
		}
		return existing, true, nil
	} else if qerr != sql.ErrNoRows {
		return LeadRequest{}, false, qerr
	}
	project, err := scanProject(tx.QueryRowContext(ctx,
		`SELECT `+projectColumns+` FROM projects WHERE id=?`, input.ProjectID))
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return LeadRequest{}, false, fmt.Errorf("%w: unknown projectId %q", domain.ErrInvalidInput, input.ProjectID)
		}
		return LeadRequest{}, false, err
	}
	if !project.Registered() || !project.Active() {
		return LeadRequest{}, false, fmt.Errorf("%w: lead request requires an active registered project", domain.ErrUnauthorized)
	}
	if project.ConfigHash != input.ProjectConfigHash {
		return LeadRequest{}, false, fmt.Errorf("%w: project configuration is stale", domain.ErrConflict)
	}
	delegation, err := scanStandingDelegationState(tx.QueryRowContext(ctx,
		`SELECT delegation_json, revoked_at FROM standing_delegations WHERE id=? AND project_id=?`, input.DelegationID, input.ProjectID))
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return LeadRequest{}, false, fmt.Errorf("%w: unknown delegation for project", domain.ErrUnauthorized)
		}
		return LeadRequest{}, false, err
	}
	if err := delegation.Validate(db.now()); err != nil {
		return LeadRequest{}, false, fmt.Errorf("%w: %v", domain.ErrUnauthorized, err)
	}
	if err := reserveDelegationTx(ctx, tx, delegation, 1, 1, 0); err != nil {
		return LeadRequest{}, false, err
	}
	now := db.now()
	request := LeadRequest{
		ID: newID(), ProjectID: input.ProjectID, Objective: input.Objective,
		RequestHash: digest, RequestJSON: input.RequestJSON, IdempotencyKey: input.IdempotencyKey,
		Status: "submitted", DelegationID: input.DelegationID,
		ProjectConfigHash: input.ProjectConfigHash, CreatedAt: now, UpdatedAt: now,
	}
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO lead_requests(id, project_id, objective, request_hash, request_json,
		 idempotency_key, status, delegation_id, project_config_hash, action_id, assessment_id,
		 proposal_id, frozen_plan_id, spec_id, campaign_id, failure_reason, created_at, updated_at)
		 VALUES(?,?,?,?,?,?,?,?,?,'','','','','','','',?,?)`,
		request.ID, request.ProjectID, request.Objective, request.RequestHash, request.RequestJSON,
		request.IdempotencyKey, request.Status, request.DelegationID, request.ProjectConfigHash,
		stamp(now), stamp(now)); err != nil {
		if isUniqueViolation(err) {
			return LeadRequest{}, false, fmt.Errorf("%w: lead request identity or idempotency key already exists", domain.ErrConflict)
		}
		return LeadRequest{}, false, err
	}
	if _, err := appendLeadEvent(ctx, tx, domain.EventLeadRequestCreated, domain.AggregateLeadRequest,
		request.ID, ActorLocalOperator, map[string]any{
			"requestId": request.ID, "projectId": request.ProjectID, "requestHash": request.RequestHash,
			"delegationId": request.DelegationID, "projectConfigHash": request.ProjectConfigHash, "status": request.Status,
		}, now); err != nil {
		return LeadRequest{}, false, err
	}
	if err := tx.Commit(); err != nil {
		return LeadRequest{}, false, err
	}
	return request, false, nil
}

// ClaimLeadRequest performs the only submitted→assessing transition. A
// process restart never reclaims an ambiguous paid assessment.
func (db *DB) ClaimLeadRequest(ctx context.Context, id string) (LeadRequest, bool, error) {
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return LeadRequest{}, false, err
	}
	defer func() { _ = tx.Rollback() }()
	current, err := scanLeadRequest(tx.QueryRowContext(ctx,
		`SELECT `+leadRequestColumns+` FROM lead_requests WHERE id=?`, strings.TrimSpace(id)))
	if err != nil {
		return LeadRequest{}, false, err
	}
	if current.ActionID == "" {
		return LeadRequest{}, false, fmt.Errorf("%w: lead request must bind its Action before assessment", domain.ErrInvalidTransition)
	}
	now := db.now()
	result, err := tx.ExecContext(ctx,
		`UPDATE lead_requests SET status='assessing', updated_at=? WHERE id=? AND status='submitted'`,
		stamp(now), strings.TrimSpace(id))
	if err != nil {
		return LeadRequest{}, false, err
	}
	changed, err := result.RowsAffected()
	if err != nil {
		return LeadRequest{}, false, err
	}
	if changed == 1 {
		if _, err := appendLeadEvent(ctx, tx, domain.EventLeadRequestStarted, domain.AggregateLeadRequest,
			current.ID, ActorLocalOperator, map[string]any{
				"requestId": current.ID, "actionId": current.ActionID, "status": "assessing",
			}, now); err != nil {
			return LeadRequest{}, false, err
		}
	}
	request, err := scanLeadRequest(tx.QueryRowContext(ctx,
		`SELECT `+leadRequestColumns+` FROM lead_requests WHERE id=?`, strings.TrimSpace(id)))
	if err != nil {
		return LeadRequest{}, false, err
	}
	if err := tx.Commit(); err != nil {
		return LeadRequest{}, false, err
	}
	return request, changed == 1, nil
}

// SetLeadActionID binds the Action created from a durable request reservation.
// It is allowed once, while the request is still submitted and before any
// assessment claim can start.
func (db *DB) SetLeadActionID(ctx context.Context, requestID, actionID string) (LeadRequest, bool, error) {
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return LeadRequest{}, false, err
	}
	defer func() { _ = tx.Rollback() }()
	request, err := scanLeadRequest(tx.QueryRowContext(ctx, `SELECT `+leadRequestColumns+` FROM lead_requests WHERE id=?`, strings.TrimSpace(requestID)))
	if err != nil {
		return LeadRequest{}, false, err
	}
	if request.ActionID != "" {
		if request.ActionID != strings.TrimSpace(actionID) {
			return LeadRequest{}, false, fmt.Errorf("%w: lead request Action is immutable", domain.ErrConflict)
		}
		if err := tx.Commit(); err != nil {
			return LeadRequest{}, false, err
		}
		return request, true, nil
	}
	if request.Status != "submitted" {
		return LeadRequest{}, false, fmt.Errorf("%w: Action must be bound before assessment", domain.ErrInvalidTransition)
	}
	var projectID string
	var actionStatus, created, updated string
	var action domain.Action
	if err := tx.QueryRowContext(ctx, `SELECT id, project_id, title, description, status, created_at, updated_at FROM actions WHERE id=?`, strings.TrimSpace(actionID)).Scan(
		&action.ID, &action.ProjectID, &action.Title, &action.Description, &actionStatus,
		&created, &updated); err != nil {
		return LeadRequest{}, false, err
	}
	if err := tx.QueryRowContext(ctx, `SELECT project_id FROM lead_requests WHERE id=?`, request.ID).Scan(&projectID); err != nil {
		return LeadRequest{}, false, err
	}
	if action.ProjectID != projectID {
		return LeadRequest{}, false, fmt.Errorf("%w: Action belongs to a different project", domain.ErrUnauthorized)
	}
	result, err := tx.ExecContext(ctx, `UPDATE lead_requests SET action_id=?, updated_at=? WHERE id=? AND status='submitted' AND action_id=''`, action.ID, stamp(db.now()), request.ID)
	if err != nil {
		return LeadRequest{}, false, err
	}
	changed, err := result.RowsAffected()
	if err != nil {
		return LeadRequest{}, false, err
	}
	request, err = scanLeadRequest(tx.QueryRowContext(ctx, `SELECT `+leadRequestColumns+` FROM lead_requests WHERE id=?`, request.ID))
	if err != nil {
		return LeadRequest{}, false, err
	}
	if err := tx.Commit(); err != nil {
		return LeadRequest{}, false, err
	}
	return request, changed == 1, nil
}

// BlockLeadRequest records a bounded failure outcome so a failed assessment
// cannot remain indistinguishable from work that is still running.
func (db *DB) BlockLeadRequest(ctx context.Context, id, expectedStatus, reason string) (LeadRequest, bool, error) {
	reason = strings.TrimSpace(reason)
	if reason == "" || len(reason) > 1000 {
		return LeadRequest{}, false, fmt.Errorf("%w: failure reason is required and bounded", domain.ErrInvalidInput)
	}
	if expectedStatus != "submitted" && expectedStatus != "assessing" && expectedStatus != "planned" && expectedStatus != "frozen" && expectedStatus != "ready" && expectedStatus != "running" {
		return LeadRequest{}, false, fmt.Errorf("%w: request cannot be blocked from status %q", domain.ErrInvalidTransition, expectedStatus)
	}
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return LeadRequest{}, false, err
	}
	defer func() { _ = tx.Rollback() }()
	var delegationID, frozenPlanID string
	_ = tx.QueryRowContext(ctx,
		`SELECT delegation_id, frozen_plan_id FROM lead_requests WHERE id=? AND status=?`,
		strings.TrimSpace(id), expectedStatus).Scan(&delegationID, &frozenPlanID)
	result, err := tx.ExecContext(ctx,
		`UPDATE lead_requests SET status='blocked', failure_reason=?, updated_at=? WHERE id=? AND status=?`,
		reason, stamp(db.now()), strings.TrimSpace(id), expectedStatus)
	if err != nil {
		return LeadRequest{}, false, err
	}
	changed, err := result.RowsAffected()
	if err != nil {
		return LeadRequest{}, false, err
	}
	if changed == 1 {
		if frozenPlanID == "" && delegationID != "" {
			if err := releaseDelegationTx(ctx, tx, delegationID, 1, 0); err != nil {
				return LeadRequest{}, false, err
			}
		}
		if _, err := appendLeadEvent(ctx, tx, domain.EventLeadRequestBlocked, domain.AggregateLeadRequest,
			strings.TrimSpace(id), ActorLocalOperator, map[string]any{
				"requestId": strings.TrimSpace(id), "status": "blocked",
			}, db.now()); err != nil {
			return LeadRequest{}, false, err
		}
	}
	request, err := scanLeadRequest(tx.QueryRowContext(ctx,
		`SELECT `+leadRequestColumns+` FROM lead_requests WHERE id=?`, strings.TrimSpace(id)))
	if err != nil {
		return LeadRequest{}, false, err
	}
	if err := tx.Commit(); err != nil {
		return LeadRequest{}, false, err
	}
	return request, changed == 1, err
}

// ReconcileLeadRequestsOnBoot fails closed for an assessment whose external
// model session may have started before the process stopped. No automatic
// paid retry is authorized; submitted reservations remain safe to resume.
func (db *DB) ReconcileLeadRequestsOnBoot(ctx context.Context) (int, error) {
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return 0, err
	}
	defer func() { _ = tx.Rollback() }()
	rows, err := tx.QueryContext(ctx, `SELECT id FROM lead_requests WHERE status='assessing' ORDER BY created_at, id`)
	if err != nil {
		return 0, err
	}
	var ids []string
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			_ = rows.Close()
			return 0, err
		}
		ids = append(ids, id)
	}
	if err := rows.Err(); err != nil {
		_ = rows.Close()
		return 0, err
	}
	if err := rows.Close(); err != nil {
		return 0, err
	}
	now := db.now()
	for _, id := range ids {
		if _, err := tx.ExecContext(ctx,
			`UPDATE lead_requests SET status='blocked', failure_reason=?, updated_at=? WHERE id=? AND status='assessing'`,
			"assessment interrupted; external session outcome requires reconciliation", stamp(now), id); err != nil {
			return 0, err
		}
		if _, err := appendLeadEvent(ctx, tx, domain.EventLeadRequestBlocked, domain.AggregateLeadRequest,
			id, ActorLocalOperator, map[string]any{"requestId": id, "status": "blocked"}, now); err != nil {
			return 0, err
		}
	}
	if err := tx.Commit(); err != nil {
		return 0, err
	}
	return len(ids), nil
}

func (db *DB) GetLeadRequest(ctx context.Context, id string) (LeadRequest, error) {
	return scanLeadRequest(db.sql.QueryRowContext(ctx,
		`SELECT `+leadRequestColumns+` FROM lead_requests WHERE id=?`, strings.TrimSpace(id)))
}

type LeadRequestLinks struct {
	AssessmentID string
	ProposalID   string
	FrozenPlanID string
	SpecID       string
	CampaignID   string
}

// AdvanceLeadRequest updates the durable orchestration projection with a
// compare-and-set. Links are write-once; the caller cannot replace evidence
// or plan identities after they have been recorded.
func (db *DB) AdvanceLeadRequest(ctx context.Context, id, expected, next string, links LeadRequestLinks) (LeadRequest, bool, error) {
	if !validLeadTransition(expected, next) {
		return LeadRequest{}, false, fmt.Errorf("%w: invalid lead request transition %q to %q", domain.ErrInvalidTransition, expected, next)
	}
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return LeadRequest{}, false, err
	}
	defer func() { _ = tx.Rollback() }()
	id = strings.TrimSpace(id)
	current, err := scanLeadRequest(tx.QueryRowContext(ctx,
		`SELECT `+leadRequestColumns+` FROM lead_requests WHERE id=?`, id))
	if err != nil {
		return LeadRequest{}, false, err
	}
	if current.Status != expected {
		if err := tx.Commit(); err != nil {
			return LeadRequest{}, false, err
		}
		return current, false, nil
	}
	if expected == "frozen" && next == "ready" && (strings.TrimSpace(links.SpecID) == "" || strings.TrimSpace(links.CampaignID) == "") {
		return LeadRequest{}, false, fmt.Errorf("%w: ready lead request requires a frozen spec and campaign", domain.ErrInvalidInput)
	}
	if !linksCompatible(current, links) {
		return LeadRequest{}, false, fmt.Errorf("%w: lead request links are immutable", domain.ErrConflict)
	}
	now := db.now()
	result, err := tx.ExecContext(ctx,
		`UPDATE lead_requests SET status=?, assessment_id=?, proposal_id=?, frozen_plan_id=?,
		 spec_id=?, campaign_id=?, updated_at=? WHERE id=? AND status=?`,
		next, leadFirstNonEmpty(current.AssessmentID, links.AssessmentID),
		leadFirstNonEmpty(current.ProposalID, links.ProposalID), leadFirstNonEmpty(current.FrozenPlanID, links.FrozenPlanID),
		leadFirstNonEmpty(current.SpecID, links.SpecID), leadFirstNonEmpty(current.CampaignID, links.CampaignID),
		stamp(now), id, expected)
	if err != nil {
		return LeadRequest{}, false, err
	}
	changed, err := result.RowsAffected()
	if err != nil {
		return LeadRequest{}, false, err
	}
	if changed != 1 {
		return LeadRequest{}, false, fmt.Errorf("%w: lead request changed concurrently", domain.ErrConflict)
	}
	if next == "blocked" {
		if _, err := appendLeadEvent(ctx, tx, domain.EventLeadRequestBlocked, domain.AggregateLeadRequest,
			id, ActorLocalOperator, map[string]any{"requestId": id, "status": "blocked"}, now); err != nil {
			return LeadRequest{}, false, err
		}
	}
	updated, err := scanLeadRequest(tx.QueryRowContext(ctx,
		`SELECT `+leadRequestColumns+` FROM lead_requests WHERE id=?`, id))
	if err != nil {
		return LeadRequest{}, false, err
	}
	if err := tx.Commit(); err != nil {
		return LeadRequest{}, false, err
	}
	return updated, true, nil
}

// RecordLeadAutoStart durably records the delegation and risk basis for an
// automatic transition. Execution authority remains the frozen plan and the
// active delegation, not this audit event.
func (db *DB) RecordLeadAutoStart(ctx context.Context, requestID, campaignID, delegationID, riskClass string) (domain.Event, error) {
	requestID = strings.TrimSpace(requestID)
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Event{}, err
	}
	defer func() { _ = tx.Rollback() }()
	now := db.now()
	event, err := appendLeadEvent(ctx, tx, domain.EventLeadRequestAutoStarted, domain.AggregateLeadRequest,
		requestID, ActorLocalOperator, map[string]any{
			"requestId": requestID, "campaignId": strings.TrimSpace(campaignID),
			"delegationId": strings.TrimSpace(delegationID), "riskClass": strings.TrimSpace(riskClass),
			"status": "running",
		}, now)
	if err != nil {
		return domain.Event{}, err
	}
	if err := tx.Commit(); err != nil {
		return domain.Event{}, err
	}
	return event, nil
}

func (db *DB) CreateStandingDelegation(ctx context.Context, input domain.StandingDelegation, idempotencyKey string) (domain.StandingDelegation, bool, error) {
	idempotencyKey = strings.TrimSpace(idempotencyKey)
	if idempotencyKey == "" || len(idempotencyKey) > domain.MaxIdempotencyKeyChars {
		return domain.StandingDelegation{}, false, fmt.Errorf("%w: idempotency key is required and bounded", domain.ErrInvalidInput)
	}
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.StandingDelegation{}, false, err
	}
	defer func() { _ = tx.Rollback() }()
	if idempotencyKey != "" {
		var prior string
		err = tx.QueryRowContext(ctx, `SELECT delegation_json FROM standing_delegations WHERE idempotency_key=?`, idempotencyKey).Scan(&prior)
		if err == nil {
			var stored domain.StandingDelegation
			if err := json.Unmarshal([]byte(prior), &stored); err != nil {
				return domain.StandingDelegation{}, false, err
			}
			priorIntent, _ := standingDelegationIntentHash(stored)
			newIntent, _ := standingDelegationIntentHash(input)
			if priorIntent != newIntent {
				return domain.StandingDelegation{}, false, fmt.Errorf("%w: delegation idempotency key is bound to different content", domain.ErrConflict)
			}
			if err := tx.Commit(); err != nil {
				return domain.StandingDelegation{}, false, err
			}
			return stored, true, nil
		}
		if !errors.Is(err, sql.ErrNoRows) {
			return domain.StandingDelegation{}, false, err
		}
	}
	input.ID = strings.TrimSpace(input.ID)
	if input.ID == "" {
		input.ID = newID()
	}
	if !isLeadID(input.ID) || input.ProjectID == "" || input.ExpiresAt.IsZero() {
		return domain.StandingDelegation{}, false, fmt.Errorf("%w: invalid standing delegation identity or expiry", domain.ErrInvalidInput)
	}
	if strings.TrimSpace(input.AuditID) == "" {
		input.AuditID = newID()
	}
	input.RevokedAt = time.Time{}
	input.ContentHash = input.ComputeContentHash()
	if err := input.Validate(db.now()); err != nil {
		return domain.StandingDelegation{}, false, fmt.Errorf("%w: %v", domain.ErrInvalidInput, err)
	}
	body, err := json.Marshal(input)
	if err != nil {
		return domain.StandingDelegation{}, false, err
	}
	body, err = canonicalJSON(string(body))
	if err != nil {
		return domain.StandingDelegation{}, false, err
	}
	project, err := scanProject(tx.QueryRowContext(ctx, `SELECT `+projectColumns+` FROM projects WHERE id=?`, input.ProjectID))
	if err != nil {
		return domain.StandingDelegation{}, false, err
	}
	if !project.Registered() || !project.Active() {
		return domain.StandingDelegation{}, false, fmt.Errorf("%w: delegation requires an active registered project", domain.ErrUnauthorized)
	}
	if input.Revision < 1 {
		return domain.StandingDelegation{}, false, fmt.Errorf("%w: delegation revision must be positive", domain.ErrInvalidInput)
	}
	now := db.now()
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO standing_delegations(id, project_id, revision, content_hash, delegation_json,
		 revoked_at, idempotency_key, created_at) VALUES(?,?,?,?,?,'',?,?)`,
		input.ID, input.ProjectID, input.Revision, input.ContentHash, string(body), idempotencyKey, stamp(now)); err != nil {
		if isUniqueViolation(err) {
			return domain.StandingDelegation{}, false, fmt.Errorf("%w: delegation id or project revision already exists", domain.ErrConflict)
		}
		return domain.StandingDelegation{}, false, err
	}
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO standing_delegation_usage(delegation_id, reserved_requests, reserved_sessions,
		 reserved_attempts, updated_at) VALUES(?,0,0,0,?)
		 ON CONFLICT(delegation_id) DO NOTHING`, input.ID, stamp(now)); err != nil {
		return domain.StandingDelegation{}, false, err
	}
	if _, err := appendLeadEvent(ctx, tx, domain.EventDelegationIssued, domain.AggregateDelegation,
		input.ID, ActorLocalOperator, map[string]any{
			"delegationId": input.ID, "projectId": input.ProjectID, "contentHash": input.ContentHash,
			"revision": input.Revision, "expiresAt": stamp(input.ExpiresAt), "status": "issued",
		}, now); err != nil {
		return domain.StandingDelegation{}, false, err
	}
	if err := tx.Commit(); err != nil {
		return domain.StandingDelegation{}, false, err
	}
	return input, false, nil
}

func (db *DB) GetStandingDelegation(ctx context.Context, id string) (domain.StandingDelegation, error) {
	return scanStandingDelegationState(db.sql.QueryRowContext(ctx,
		`SELECT delegation_json, revoked_at FROM standing_delegations WHERE id=?`, strings.TrimSpace(id)))
}

func (db *DB) GetDelegationUsage(ctx context.Context, id string) (DelegationUsage, error) {
	var usage DelegationUsage
	var updated string
	err := db.sql.QueryRowContext(ctx,
		`SELECT delegation_id, reserved_requests, reserved_sessions, reserved_attempts, updated_at
		 FROM standing_delegation_usage WHERE delegation_id=?`, strings.TrimSpace(id)).Scan(
		&usage.DelegationID, &usage.ReservedRequests, &usage.ReservedSessions,
		&usage.ReservedAttempts, &updated)
	if err == sql.ErrNoRows {
		return DelegationUsage{DelegationID: strings.TrimSpace(id)}, nil
	}
	if err != nil {
		return DelegationUsage{}, err
	}
	usage.UpdatedAt = parseStamp(updated)
	return usage, nil
}

func reserveDelegationTx(ctx context.Context, tx *sql.Tx, grant domain.StandingDelegation, requests, sessions, attempts int) error {
	if requests < 0 || sessions < 0 || attempts < 0 {
		return fmt.Errorf("%w: delegation reservation must not be negative", domain.ErrInvalidInput)
	}
	now := stamp(time.Now().UTC())
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO standing_delegation_usage(delegation_id, reserved_requests, reserved_sessions,
		 reserved_attempts, updated_at) VALUES(?,0,0,0,?) ON CONFLICT(delegation_id) DO NOTHING`,
		grant.ID, now); err != nil {
		return err
	}
	result, err := tx.ExecContext(ctx,
		`UPDATE standing_delegation_usage
		 SET reserved_requests=reserved_requests+?, reserved_sessions=reserved_sessions+?,
		     reserved_attempts=reserved_attempts+?, updated_at=?
		 WHERE delegation_id=?
		   AND reserved_requests+? <= ? AND reserved_sessions+? <= ? AND reserved_attempts+? <= ?`,
		requests, sessions, attempts, now, grant.ID,
		requests, grant.MaxRequests, sessions, grant.MaxCumulativeSessions,
		attempts, grant.MaxCumulativeAttempts)
	if err != nil {
		return err
	}
	changed, err := result.RowsAffected()
	if err != nil {
		return err
	}
	if changed != 1 {
		return fmt.Errorf("%w: delegation cumulative request, session or attempt limit is exhausted", domain.ErrBudgetExhausted)
	}
	return nil
}

func releaseDelegationTx(ctx context.Context, tx *sql.Tx, delegationID string, sessions, attempts int) error {
	if sessions <= 0 && attempts <= 0 {
		return nil
	}
	_, err := tx.ExecContext(ctx,
		`UPDATE standing_delegation_usage
		 SET reserved_sessions=CASE WHEN reserved_sessions>=? THEN reserved_sessions-? ELSE 0 END,
		     reserved_attempts=CASE WHEN reserved_attempts>=? THEN reserved_attempts-? ELSE 0 END,
		     updated_at=?
		 WHERE delegation_id=?`,
		sessions, sessions, attempts, attempts, stamp(time.Now().UTC()), delegationID)
	return err
}

func (db *DB) RevokeStandingDelegation(ctx context.Context, id string, at time.Time) (domain.StandingDelegation, bool, error) {
	if at.IsZero() {
		at = db.now()
	}
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.StandingDelegation{}, false, err
	}
	defer func() { _ = tx.Rollback() }()
	grant, err := scanStandingDelegationState(tx.QueryRowContext(ctx,
		`SELECT delegation_json, revoked_at FROM standing_delegations WHERE id=?`, strings.TrimSpace(id)))
	if err != nil {
		return domain.StandingDelegation{}, false, err
	}
	changed := grant.RevokedAt.IsZero()
	if changed {
		grant.RevokedAt = at.UTC()
		bodyBytes, err := json.Marshal(grant)
		if err != nil {
			return domain.StandingDelegation{}, false, err
		}
		if _, err := tx.ExecContext(ctx,
			`UPDATE standing_delegations SET delegation_json=?, revoked_at=? WHERE id=? AND revoked_at=''`,
			string(bodyBytes), stamp(grant.RevokedAt), strings.TrimSpace(id)); err != nil {
			return domain.StandingDelegation{}, false, err
		}
		if _, err := appendLeadEvent(ctx, tx, domain.EventDelegationRevoked, domain.AggregateDelegation,
			grant.ID, ActorLocalOperator, map[string]any{
				"delegationId": grant.ID, "contentHash": grant.ContentHash,
				"revokedAt": stamp(grant.RevokedAt), "status": "revoked",
			}, db.now()); err != nil {
			return domain.StandingDelegation{}, false, err
		}
	}
	if err := tx.Commit(); err != nil {
		return domain.StandingDelegation{}, false, err
	}
	return grant, changed, nil
}

// ContainDelegationRevocation immediately blocks open Lead requests and
// terminates their nonterminal Campaigns, cancelling active implementer Runs
// through the existing fencing path. Immutable evidence is retained.
func (db *DB) ContainDelegationRevocation(ctx context.Context, delegationID, reason, correlationID string) ([]domain.Event, []string, error) {
	reason = domain.SanitizeReason(reason)
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return nil, nil, err
	}
	defer func() { _ = tx.Rollback() }()
	now := db.now()
	rows, err := tx.QueryContext(ctx,
		`SELECT id, status FROM lead_requests WHERE delegation_id=?
		 AND status IN ('submitted','assessing','planned','frozen','ready','running')`,
		strings.TrimSpace(delegationID))
	if err != nil {
		return nil, nil, err
	}
	type openRequest struct{ id, status string }
	var requests []openRequest
	for rows.Next() {
		var request openRequest
		if err := rows.Scan(&request.id, &request.status); err != nil {
			_ = rows.Close()
			return nil, nil, err
		}
		requests = append(requests, request)
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return nil, nil, err
	}
	var events []domain.Event
	var campaignIDs []string
	for _, request := range requests {
		result, err := tx.ExecContext(ctx,
			`UPDATE lead_requests SET status='blocked', failure_reason=?, updated_at=? WHERE id=? AND status=?`,
			reason, stamp(now), request.id, request.status)
		if err != nil {
			return nil, nil, err
		}
		changed, err := result.RowsAffected()
		if err != nil {
			return nil, nil, err
		}
		if changed == 1 {
			event, err := appendLeadEvent(ctx, tx, domain.EventLeadRequestBlocked, domain.AggregateLeadRequest,
				request.id, ActorLocalOperator, map[string]any{"requestId": request.id, "status": "blocked"}, now)
			if err != nil {
				return nil, nil, err
			}
			events = append(events, event)
		}
		var campaignID string
		if err := tx.QueryRowContext(ctx,
			`SELECT campaign_id FROM lead_requests WHERE id=?`, request.id).Scan(&campaignID); err == nil && campaignID != "" {
			campaignIDs = append(campaignIDs, campaignID)
		}
	}
	for _, campaignID := range campaignIDs {
		_, campaignEvents, ferr := finishCampaignTx(ctx, tx, campaignID, domain.CampaignCancelled,
			reason, domain.EventCampaignFinished, correlationID, now)
		if ferr != nil {
			return nil, nil, ferr
		}
		events = append(events, campaignEvents...)
	}
	if err := tx.Commit(); err != nil {
		return nil, nil, err
	}
	return events, campaignIDs, nil
}

func (db *DB) CreateLeadAssessment(ctx context.Context, requestID, mechanism, assessmentJSON, routeJSON, idempotencyKey string) (LeadAssessmentRecord, bool, error) {
	requestID = strings.TrimSpace(requestID)
	mechanism = strings.TrimSpace(mechanism)
	assessment, err := canonicalJSON(assessmentJSON)
	if err != nil {
		return LeadAssessmentRecord{}, false, fmt.Errorf("%w: invalid assessment JSON: %v", domain.ErrInvalidInput, err)
	}
	route, err := canonicalJSON(routeJSON)
	if err != nil {
		return LeadAssessmentRecord{}, false, fmt.Errorf("%w: invalid route JSON: %v", domain.ErrInvalidInput, err)
	}
	if mechanism != "DETERMINISTIC" && mechanism != "MODEL" && mechanism != "HYBRID" {
		return LeadAssessmentRecord{}, false, fmt.Errorf("%w: invalid decision mechanism", domain.ErrInvalidInput)
	}
	hash, err := hashJSON(struct{ Mechanism, Assessment, Route string }{mechanism, string(assessment), string(route)})
	if err != nil {
		return LeadAssessmentRecord{}, false, err
	}
	idempotencyKey = strings.TrimSpace(idempotencyKey)
	if idempotencyKey == "" || len(idempotencyKey) > domain.MaxIdempotencyKeyChars {
		return LeadAssessmentRecord{}, false, fmt.Errorf("%w: assessment idempotency key is required and bounded", domain.ErrInvalidInput)
	}
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return LeadAssessmentRecord{}, false, err
	}
	defer func() { _ = tx.Rollback() }()
	var request LeadRequest
	if request, err = scanLeadRequest(tx.QueryRowContext(ctx, `SELECT `+leadRequestColumns+` FROM lead_requests WHERE id=?`, requestID)); err != nil {
		return LeadAssessmentRecord{}, false, err
	}
	if existing, qerr := scanLeadAssessment(tx.QueryRowContext(ctx, `SELECT `+leadAssessmentColumns+` FROM lead_assessments WHERE request_id=?`, requestID)); qerr == nil {
		if existing.AssessmentHash != hash || existing.IdempotencyKey != idempotencyKey {
			return LeadAssessmentRecord{}, false, fmt.Errorf("%w: request already has a different assessment", domain.ErrConflict)
		}
		if err := tx.Commit(); err != nil {
			return LeadAssessmentRecord{}, false, err
		}
		return existing, true, nil
	} else if qerr != sql.ErrNoRows {
		return LeadAssessmentRecord{}, false, qerr
	}
	if request.Status != "assessing" {
		return LeadAssessmentRecord{}, false, fmt.Errorf("%w: lead request is not claimed for assessment", domain.ErrInvalidTransition)
	}
	var projectID, actionID string
	if err := tx.QueryRowContext(ctx, `SELECT project_id, action_id FROM lead_requests WHERE id=?`, requestID).Scan(&projectID, &actionID); err != nil {
		return LeadAssessmentRecord{}, false, err
	}
	if actionID == "" {
		return LeadAssessmentRecord{}, false, fmt.Errorf("%w: lead request has no bound Action", domain.ErrInvalidTransition)
	}
	record := LeadAssessmentRecord{
		ID: newID(), RequestID: requestID, ProjectID: projectID, ActionID: actionID,
		Mechanism: mechanism, AssessmentJSON: string(assessment), RouteJSON: string(route),
		AssessmentHash: hash, IdempotencyKey: idempotencyKey, CreatedAt: db.now(),
	}
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO lead_assessments(id, request_id, project_id, action_id, mechanism, assessment_json,
		 route_json, assessment_hash, idempotency_key, created_at) VALUES(?,?,?,?,?,?,?,?,?,?)`,
		record.ID, record.RequestID, record.ProjectID, record.ActionID, record.Mechanism, record.AssessmentJSON,
		record.RouteJSON, record.AssessmentHash, record.IdempotencyKey, stamp(record.CreatedAt)); err != nil {
		if isUniqueViolation(err) {
			return LeadAssessmentRecord{}, false, fmt.Errorf("%w: assessment identity or idempotency key already exists", domain.ErrConflict)
		}
		return LeadAssessmentRecord{}, false, err
	}
	if _, err := appendLeadEvent(ctx, tx, domain.EventLeadAssessmentRecorded, domain.AggregateLeadRequest,
		request.ID, ActorLocalOperator, map[string]any{
			"requestId": request.ID, "assessmentId": record.ID, "assessmentHash": record.AssessmentHash,
			"mechanism": record.Mechanism, "status": request.Status,
		}, record.CreatedAt); err != nil {
		return LeadAssessmentRecord{}, false, err
	}
	if err := tx.Commit(); err != nil {
		return LeadAssessmentRecord{}, false, err
	}
	return record, false, nil
}

func (db *DB) CreateLeadProposal(ctx context.Context, requestID, assessmentID string, proposal domain.LeadPlanProposal, idempotencyKey string) (LeadProposalRecord, bool, error) {
	requestID, assessmentID = strings.TrimSpace(requestID), strings.TrimSpace(assessmentID)
	proposalJSON, err := json.Marshal(proposal)
	if err != nil {
		return LeadProposalRecord{}, false, err
	}
	proposalJSON, err = canonicalJSON(string(proposalJSON))
	if err != nil {
		return LeadProposalRecord{}, false, err
	}
	hash := proposal.ContentHash()
	idempotencyKey = strings.TrimSpace(idempotencyKey)
	if idempotencyKey == "" || len(idempotencyKey) > domain.MaxIdempotencyKeyChars {
		return LeadProposalRecord{}, false, fmt.Errorf("%w: proposal idempotency key is required and bounded", domain.ErrInvalidInput)
	}
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return LeadProposalRecord{}, false, err
	}
	defer func() { _ = tx.Rollback() }()
	request, err := scanLeadRequest(tx.QueryRowContext(ctx, `SELECT `+leadRequestColumns+` FROM lead_requests WHERE id=?`, requestID))
	if err != nil {
		return LeadProposalRecord{}, false, err
	}
	if existing, qerr := scanLeadProposal(tx.QueryRowContext(ctx, `SELECT `+leadProposalColumns+` FROM lead_proposals WHERE request_id=?`, requestID)); qerr == nil {
		if existing.ContentHash != hash || existing.AssessmentID != assessmentID || existing.IdempotencyKey != idempotencyKey {
			return LeadProposalRecord{}, false, fmt.Errorf("%w: request already has a different proposal", domain.ErrConflict)
		}
		if err := tx.Commit(); err != nil {
			return LeadProposalRecord{}, false, err
		}
		return existing, true, nil
	} else if qerr != sql.ErrNoRows {
		return LeadProposalRecord{}, false, qerr
	}
	if request.Status != "assessing" {
		return LeadProposalRecord{}, false, fmt.Errorf("%w: lead request is not in assessment phase", domain.ErrInvalidTransition)
	}
	var assessmentProject string
	if err := tx.QueryRowContext(ctx, `SELECT project_id FROM lead_assessments WHERE id=? AND request_id=?`, assessmentID, requestID).Scan(&assessmentProject); err != nil {
		return LeadProposalRecord{}, false, err
	}
	if assessmentProject != proposal.ProjectID || request.ProjectID != proposal.ProjectID || request.ActionID != proposal.ActionID {
		return LeadProposalRecord{}, false, fmt.Errorf("%w: proposal project binding differs from request or assessment", domain.ErrUnauthorized)
	}
	record := LeadProposalRecord{
		ID: newID(), RequestID: requestID, ProjectID: proposal.ProjectID,
		AssessmentID: assessmentID, Proposal: proposal, ContentHash: hash,
		IdempotencyKey: idempotencyKey, CreatedAt: db.now(),
	}
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO lead_proposals(id, request_id, project_id, action_id, assessment_id,
		 content_hash, proposal_json, idempotency_key, created_at) VALUES(?,?,?,?,?,?,?,?,?)`,
		record.ID, record.RequestID, record.ProjectID, proposal.ActionID, record.AssessmentID,
		record.ContentHash, string(proposalJSON), record.IdempotencyKey, stamp(record.CreatedAt)); err != nil {
		if isUniqueViolation(err) {
			return LeadProposalRecord{}, false, fmt.Errorf("%w: proposal identity or idempotency key already exists", domain.ErrConflict)
		}
		return LeadProposalRecord{}, false, err
	}
	if _, err := tx.ExecContext(ctx,
		`UPDATE lead_requests SET status='planned', assessment_id=?, proposal_id=?, updated_at=?
		 WHERE id=? AND status='assessing' AND (assessment_id='' OR assessment_id=?)
		 AND (proposal_id='' OR proposal_id=?)`,
		assessmentID, record.ID, stamp(record.CreatedAt), requestID, assessmentID, record.ID); err != nil {
		return LeadProposalRecord{}, false, err
	}
	if err := tx.Commit(); err != nil {
		return LeadProposalRecord{}, false, err
	}
	return record, false, nil
}

func (db *DB) FreezeLeadPlan(ctx context.Context, requestID, proposalID, idempotencyKey string) (FrozenLeadPlanRecord, bool, error) {
	requestID, proposalID = strings.TrimSpace(requestID), strings.TrimSpace(proposalID)
	idempotencyKey = strings.TrimSpace(idempotencyKey)
	if idempotencyKey == "" || len(idempotencyKey) > domain.MaxIdempotencyKeyChars {
		return FrozenLeadPlanRecord{}, false, fmt.Errorf("%w: plan idempotency key is required and bounded", domain.ErrInvalidInput)
	}
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return FrozenLeadPlanRecord{}, false, err
	}
	defer func() { _ = tx.Rollback() }()
	request, err := scanLeadRequest(tx.QueryRowContext(ctx, `SELECT `+leadRequestColumns+` FROM lead_requests WHERE id=?`, requestID))
	if err != nil {
		return FrozenLeadPlanRecord{}, false, err
	}
	proposal, err := scanLeadProposal(tx.QueryRowContext(ctx, `SELECT `+leadProposalColumns+` FROM lead_proposals WHERE id=? AND request_id=?`, proposalID, requestID))
	if err != nil {
		return FrozenLeadPlanRecord{}, false, err
	}
	if existing, qerr := scanFrozenLeadPlan(tx.QueryRowContext(ctx, `SELECT `+frozenLeadPlanColumns+` FROM frozen_lead_plans WHERE request_id=?`, requestID)); qerr == nil {
		if existing.ProposalID != proposalID || existing.IdempotencyKey != idempotencyKey {
			return FrozenLeadPlanRecord{}, false, fmt.Errorf("%w: request already has a different frozen plan", domain.ErrConflict)
		}
		if err := tx.Commit(); err != nil {
			return FrozenLeadPlanRecord{}, false, err
		}
		return existing, true, nil
	} else if qerr != sql.ErrNoRows {
		return FrozenLeadPlanRecord{}, false, qerr
	}
	project, err := scanProject(tx.QueryRowContext(ctx, `SELECT `+projectColumns+` FROM projects WHERE id=?`, request.ProjectID))
	if err != nil {
		return FrozenLeadPlanRecord{}, false, err
	}
	if !project.Registered() || !project.Active() || project.ConfigHash != request.ProjectConfigHash ||
		project.ConfigHash != proposal.Proposal.ProjectConfigHash {
		return FrozenLeadPlanRecord{}, false, fmt.Errorf("%w: project configuration changed before plan freeze", domain.ErrConflict)
	}
	grant, err := scanStandingDelegationState(tx.QueryRowContext(ctx,
		`SELECT delegation_json, revoked_at FROM standing_delegations WHERE id=? AND project_id=?`, request.DelegationID, request.ProjectID))
	if err != nil {
		return FrozenLeadPlanRecord{}, false, err
	}
	if request.Status != "planned" {
		return FrozenLeadPlanRecord{}, false, fmt.Errorf("%w: lead request is not ready to freeze", domain.ErrInvalidTransition)
	}
	if err := grant.Validate(db.now()); err != nil {
		return FrozenLeadPlanRecord{}, false, fmt.Errorf("%w: %v", domain.ErrUnauthorized, err)
	}
	action := domain.Action{}
	var actionStatus, actionCreated, actionUpdated string
	if err := tx.QueryRowContext(ctx, `SELECT id, project_id, title, description, status, created_at, updated_at FROM actions WHERE id=?`, proposal.Proposal.ActionID).Scan(
		&action.ID, &action.ProjectID, &action.Title, &action.Description, &actionStatus,
		&actionCreated, &actionUpdated); err != nil {
		return FrozenLeadPlanRecord{}, false, err
	}
	action.Status = domain.ActionStatus(actionStatus)
	action.CreatedAt, action.UpdatedAt = parseStamp(actionCreated), parseStamp(actionUpdated)
	if err := proposal.Proposal.Validate(grant, project, action, db.now()); err != nil {
		return FrozenLeadPlanRecord{}, false, fmt.Errorf("%w: proposal failed freeze validation: %v", domain.ErrInvalidInput, err)
	}
	additionalSessions := proposal.Proposal.MaxSessions - 1
	additionalAttempts := proposal.Proposal.ImplementerTaskCount() * proposal.Proposal.Budget.MaxAttempts
	if additionalSessions < 0 {
		return FrozenLeadPlanRecord{}, false, fmt.Errorf("%w: plan session ceiling is invalid", domain.ErrInvalidInput)
	}
	if err := reserveDelegationTx(ctx, tx, grant, 0, additionalSessions, additionalAttempts); err != nil {
		return FrozenLeadPlanRecord{}, false, err
	}
	identity := struct {
		RequestID, ProjectID, ProposalHash, GrantHash, ProjectConfigHash string
		ProjectConfigRevision                                            int
	}{
		requestID, project.ID, proposal.ContentHash, grant.ContentHash,
		project.ConfigHash, project.ConfigRevision,
	}
	planHash, err := hashJSON(identity)
	if err != nil {
		return FrozenLeadPlanRecord{}, false, err
	}
	record := FrozenLeadPlanRecord{
		ID: newID(), RequestID: requestID,
		ProjectID: project.ID, ProposalID: proposalID, ProposalHash: proposal.ContentHash,
		DelegationID: grant.ID, DelegationHash: grant.ContentHash,
		ProjectConfigRevision: project.ConfigRevision, ProjectConfigHash: project.ConfigHash,
		PlanHash: planHash, IdempotencyKey: idempotencyKey, CreatedAt: db.now(),
	}
	if _, err := tx.ExecContext(ctx,
		`INSERT INTO frozen_lead_plans(id, request_id, project_id, proposal_id, delegation_id,
		 proposal_hash, delegation_hash, project_config_revision, project_config_hash,
		 plan_hash, idempotency_key, created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)`,
		record.ID, record.RequestID, record.ProjectID, record.ProposalID, record.DelegationID,
		record.ProposalHash, record.DelegationHash, record.ProjectConfigRevision,
		record.ProjectConfigHash, record.PlanHash, record.IdempotencyKey, stamp(record.CreatedAt)); err != nil {
		if isUniqueViolation(err) {
			return FrozenLeadPlanRecord{}, false, fmt.Errorf("%w: frozen plan identity or idempotency key already exists", domain.ErrConflict)
		}
		return FrozenLeadPlanRecord{}, false, err
	}
	if _, err := tx.ExecContext(ctx,
		`UPDATE lead_requests SET status='frozen', frozen_plan_id=?, updated_at=?
		 WHERE id=? AND status='planned' AND (frozen_plan_id='' OR frozen_plan_id=?)`,
		record.ID, stamp(record.CreatedAt), requestID, record.ID); err != nil {
		return FrozenLeadPlanRecord{}, false, err
	}
	if _, err := appendLeadEvent(ctx, tx, domain.EventLeadPlanFrozen, domain.AggregateLeadRequest,
		requestID, ActorLocalOperator, map[string]any{
			"requestId": requestID, "frozenPlanId": record.ID, "proposalHash": record.ProposalHash,
			"delegationHash": record.DelegationHash, "projectConfigHash": record.ProjectConfigHash,
			"planHash": record.PlanHash, "status": "frozen",
		}, record.CreatedAt); err != nil {
		return FrozenLeadPlanRecord{}, false, err
	}
	if err := tx.Commit(); err != nil {
		return FrozenLeadPlanRecord{}, false, err
	}
	return record, false, nil
}

func (db *DB) GetFrozenLeadPlan(ctx context.Context, id string) (FrozenLeadPlanRecord, error) {
	return scanFrozenLeadPlan(db.sql.QueryRowContext(ctx,
		`SELECT `+frozenLeadPlanColumns+` FROM frozen_lead_plans WHERE id=?`, strings.TrimSpace(id)))
}

func (db *DB) GetLeadAssessment(ctx context.Context, id string) (LeadAssessmentRecord, error) {
	return scanLeadAssessment(db.sql.QueryRowContext(ctx,
		`SELECT `+leadAssessmentColumns+` FROM lead_assessments WHERE id=?`, strings.TrimSpace(id)))
}

func (db *DB) GetLeadProposal(ctx context.Context, id string) (LeadProposalRecord, error) {
	return scanLeadProposal(db.sql.QueryRowContext(ctx,
		`SELECT `+leadProposalColumns+` FROM lead_proposals WHERE id=?`, strings.TrimSpace(id)))
}

// CheckLeadCampaignAuthorization rechecks the active delegation and frozen
// bindings before every orchestrator tick. Ordinary campaigns are not
// governed by a Lead request and return applicable=false.
func (db *DB) CheckLeadCampaignAuthorization(ctx context.Context, campaignID string) (applicable, authorized bool, err error) {
	request, err := scanLeadRequest(db.sql.QueryRowContext(ctx,
		`SELECT `+leadRequestColumns+` FROM lead_requests WHERE campaign_id=?`, strings.TrimSpace(campaignID)))
	if errors.Is(err, sql.ErrNoRows) {
		return false, false, nil
	}
	if err != nil {
		return false, false, err
	}
	if (request.Status != "ready" && request.Status != "running") || request.ActionID == "" || request.SpecID == "" || request.CampaignID != strings.TrimSpace(campaignID) {
		return true, false, nil
	}
	plan, err := scanFrozenLeadPlan(db.sql.QueryRowContext(ctx,
		`SELECT `+frozenLeadPlanColumns+` FROM frozen_lead_plans WHERE id=? AND request_id=?`, request.FrozenPlanID, request.ID))
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return true, false, nil
		}
		return true, false, err
	}
	proposal, err := scanLeadProposal(db.sql.QueryRowContext(ctx,
		`SELECT `+leadProposalColumns+` FROM lead_proposals WHERE id=? AND request_id=?`, plan.ProposalID, request.ID))
	if err != nil {
		return true, false, err
	}
	if proposal.ContentHash != plan.ProposalHash || proposal.Proposal.ActionID != request.ActionID ||
		proposal.Proposal.ProjectID != request.ProjectID {
		return true, false, nil
	}
	project, err := scanProject(db.sql.QueryRowContext(ctx,
		`SELECT `+projectColumns+` FROM projects WHERE id=?`, request.ProjectID))
	if err != nil {
		return true, false, err
	}
	if !project.Registered() || !project.Active() || project.ConfigHash != plan.ProjectConfigHash ||
		project.ConfigHash != request.ProjectConfigHash || project.ConfigRevision != plan.ProjectConfigRevision {
		return true, false, nil
	}
	grant, err := scanStandingDelegationState(db.sql.QueryRowContext(ctx,
		`SELECT delegation_json, revoked_at FROM standing_delegations WHERE id=? AND project_id=?`, plan.DelegationID, project.ID))
	if err != nil {
		return true, false, err
	}
	if grant.ContentHash != plan.DelegationHash || grant.ID != request.DelegationID || grant.Validate(db.now()) != nil {
		return true, false, nil
	}
	action, err := db.GetAction(request.ActionID)
	if err != nil {
		return true, false, err
	}
	if action.ProjectID != project.ID || (action.Status != domain.StatusPending && action.Status != domain.StatusInProgress) {
		return true, false, nil
	}
	spec, err := db.GetActionSpec(request.SpecID)
	if err != nil {
		return true, false, err
	}
	campaign, err := db.GetCampaign(campaignID)
	if err != nil {
		return true, false, err
	}
	if campaign.ProjectID != project.ID || campaign.ActionID != action.ID || campaign.SpecID != spec.ID ||
		campaign.SpecHash != spec.ContentHash || campaign.PlanHash == "" ||
		spec.ActionID != action.ID || spec.ProjectID != project.ID || spec.ProjectConfigRevision != plan.ProjectConfigRevision {
		return true, false, nil
	}
	if err := spec.ValidateProjectBinding(project); err != nil {
		return true, false, nil
	}
	if err := campaign.Plan.Validate(); err != nil || campaign.Plan.Hash() != campaign.PlanHash {
		return true, false, nil
	}
	for _, node := range campaign.Plan.Tasks {
		if node.SpecID == "" || node.SpecID == spec.ID {
			continue
		}
		unitSpec, serr := db.GetActionSpec(node.SpecID)
		if serr != nil || unitSpec.ActionID != action.ID || unitSpec.ProjectID != project.ID ||
			unitSpec.ProjectConfigRevision != plan.ProjectConfigRevision {
			return true, false, nil
		}
		if err := unitSpec.ValidateProjectBinding(project); err != nil {
			return true, false, nil
		}
	}
	if err := proposal.Proposal.Validate(grant, project, action, db.now()); err != nil {
		return true, false, nil
	}
	identity := struct {
		RequestID, ProjectID, ProposalHash, GrantHash, ProjectConfigHash string
		ProjectConfigRevision                                            int
	}{
		request.ID, project.ID, plan.ProposalHash, plan.DelegationHash,
		plan.ProjectConfigHash, plan.ProjectConfigRevision,
	}
	planHash, err := hashJSON(identity)
	if err != nil {
		return true, false, err
	}
	if plan.PlanHash != planHash {
		return true, false, nil
	}
	return true, true, nil
}

// LeadSpecManaged reports whether a frozen ActionSpec belongs to a Lead
// request and therefore must execute through its authorized Campaign.
func (db *DB) LeadSpecManaged(ctx context.Context, specID string) (bool, error) {
	var managed bool
	err := db.sql.QueryRowContext(ctx,
		`SELECT EXISTS(
			SELECT 1 FROM action_specs s JOIN lead_requests r ON r.action_id=s.action_id
			WHERE s.id=?)`, strings.TrimSpace(specID)).Scan(&managed)
	return managed, err
}

// LeadActionManaged reports whether a reserved Lead request has claimed this
// Action. It intentionally keys on action_id before spec_id or campaign_id
// linkage exists, closing the interval in which generic campaign routes could
// otherwise dispatch a Lead-managed Action.
func (db *DB) LeadActionManaged(ctx context.Context, actionID string) (bool, error) {
	var managed bool
	err := db.sql.QueryRowContext(ctx,
		`SELECT EXISTS(SELECT 1 FROM lead_requests WHERE action_id=?)`, strings.TrimSpace(actionID)).Scan(&managed)
	return managed, err
}

// NewLeadID exposes the store's cryptographically random identity format to
// the Core API when it must mint an identity before hashing a delegation.
func NewLeadID() string { return newID() }

const leadRequestColumns = `id, project_id, objective, request_hash, request_json,
	idempotency_key, status, delegation_id, project_config_hash, action_id, assessment_id,
	proposal_id, frozen_plan_id, spec_id, campaign_id, failure_reason, created_at, updated_at`

const leadAssessmentColumns = `id, request_id, project_id, action_id, mechanism, assessment_json,
	route_json, assessment_hash, idempotency_key, created_at`

const leadProposalColumns = `id, request_id, project_id, assessment_id, proposal_json,
	content_hash, idempotency_key, created_at`

const frozenLeadPlanColumns = `id, request_id, project_id, proposal_id, proposal_hash,
	delegation_id, delegation_hash, project_config_revision, project_config_hash,
	plan_hash, idempotency_key, created_at`

func scanLeadRequest(row interface{ Scan(...any) error }) (LeadRequest, error) {
	var r LeadRequest
	var created, updated string
	err := row.Scan(&r.ID, &r.ProjectID, &r.Objective, &r.RequestHash, &r.RequestJSON,
		&r.IdempotencyKey, &r.Status, &r.DelegationID, &r.ProjectConfigHash, &r.ActionID,
		&r.AssessmentID, &r.ProposalID, &r.FrozenPlanID, &r.SpecID, &r.CampaignID, &r.FailureReason,
		&created, &updated)
	if err != nil {
		return LeadRequest{}, err
	}
	r.CreatedAt, r.UpdatedAt = parseStamp(created), parseStamp(updated)
	digest, err := hashJSON(struct {
		ProjectID, Objective, RequestJSON, DelegationID, ProjectConfigHash string
	}{r.ProjectID, r.Objective, r.RequestJSON, r.DelegationID, r.ProjectConfigHash})
	if err != nil {
		return LeadRequest{}, err
	}
	if digest != r.RequestHash {
		return LeadRequest{}, errors.New("stored lead request digest does not match its content")
	}
	return r, nil
}

func scanStandingDelegationState(row interface{ Scan(...any) error }) (domain.StandingDelegation, error) {
	var raw, revokedAt string
	if err := row.Scan(&raw, &revokedAt); err != nil {
		return domain.StandingDelegation{}, err
	}
	var grant domain.StandingDelegation
	if err := json.Unmarshal([]byte(raw), &grant); err != nil {
		return domain.StandingDelegation{}, err
	}
	grant.RevokedAt = parseStamp(revokedAt)
	return grant, nil
}

func scanLeadAssessment(row interface{ Scan(...any) error }) (LeadAssessmentRecord, error) {
	var r LeadAssessmentRecord
	var created string
	err := row.Scan(&r.ID, &r.RequestID, &r.ProjectID, &r.ActionID, &r.Mechanism, &r.AssessmentJSON,
		&r.RouteJSON, &r.AssessmentHash, &r.IdempotencyKey, &created)
	if err != nil {
		return LeadAssessmentRecord{}, err
	}
	r.CreatedAt = parseStamp(created)
	digest, err := hashJSON(struct{ Mechanism, Assessment, Route string }{r.Mechanism, r.AssessmentJSON, r.RouteJSON})
	if err != nil {
		return LeadAssessmentRecord{}, err
	}
	if digest != r.AssessmentHash {
		return LeadAssessmentRecord{}, errors.New("stored lead assessment digest does not match its content")
	}
	return r, nil
}

func scanLeadProposal(row interface{ Scan(...any) error }) (LeadProposalRecord, error) {
	var r LeadProposalRecord
	var raw, created string
	err := row.Scan(&r.ID, &r.RequestID, &r.ProjectID, &r.AssessmentID, &raw,
		&r.ContentHash, &r.IdempotencyKey, &created)
	if err != nil {
		return LeadProposalRecord{}, err
	}
	if err := json.Unmarshal([]byte(raw), &r.Proposal); err != nil {
		return LeadProposalRecord{}, err
	}
	if r.Proposal.ContentHash() != r.ContentHash {
		return LeadProposalRecord{}, errors.New("stored lead proposal digest does not match its content")
	}
	r.CreatedAt = parseStamp(created)
	return r, nil
}

func scanFrozenLeadPlan(row interface{ Scan(...any) error }) (FrozenLeadPlanRecord, error) {
	var r FrozenLeadPlanRecord
	var created string
	err := row.Scan(&r.ID, &r.RequestID, &r.ProjectID, &r.ProposalID, &r.ProposalHash,
		&r.DelegationID, &r.DelegationHash, &r.ProjectConfigRevision,
		&r.ProjectConfigHash, &r.PlanHash, &r.IdempotencyKey, &created)
	if err != nil {
		return FrozenLeadPlanRecord{}, err
	}
	r.CreatedAt = parseStamp(created)
	identity := struct {
		RequestID, ProjectID, ProposalHash, GrantHash, ProjectConfigHash string
		ProjectConfigRevision                                            int
	}{
		r.RequestID, r.ProjectID, r.ProposalHash, r.DelegationHash,
		r.ProjectConfigHash, r.ProjectConfigRevision,
	}
	digest, err := hashJSON(identity)
	if err != nil {
		return FrozenLeadPlanRecord{}, err
	}
	if digest != r.PlanHash {
		return FrozenLeadPlanRecord{}, errors.New("stored frozen plan digest does not match its identity")
	}
	return r, nil
}

func canonicalJSON(raw string) ([]byte, error) {
	decoder := json.NewDecoder(strings.NewReader(raw))
	decoder.UseNumber()
	var value any
	if err := decoder.Decode(&value); err != nil {
		return nil, err
	}
	var extra any
	if err := decoder.Decode(&extra); err == nil {
		return nil, errors.New("multiple JSON values")
	} else if !errors.Is(err, io.EOF) {
		return nil, err
	}
	return json.Marshal(value)
}

func hashJSON(value any) (string, error) {
	raw, err := json.Marshal(value)
	if err != nil {
		return "", err
	}
	digest := sha256.Sum256(raw)
	return hex.EncodeToString(digest[:]), nil
}

func standingDelegationIntentHash(grant domain.StandingDelegation) (string, error) {
	grant.ID = ""
	grant.AuditID = ""
	grant.ContentHash = ""
	grant.RevokedAt = time.Time{}
	return hashJSON(grant)
}

func validLeadTransition(expected, next string) bool {
	switch expected + "->" + next {
	case "submitted->assessing", "assessing->planned", "assessing->blocked",
		"planned->frozen", "planned->blocked", "frozen->frozen",
		"frozen->ready", "frozen->blocked", "ready->running", "ready->blocked",
		"running->blocked", "running->running":
		return true
	default:
		return false
	}
}

func linksCompatible(current LeadRequest, links LeadRequestLinks) bool {
	return linkCompatible(current.AssessmentID, links.AssessmentID) &&
		linkCompatible(current.ProposalID, links.ProposalID) &&
		linkCompatible(current.FrozenPlanID, links.FrozenPlanID) &&
		linkCompatible(current.SpecID, links.SpecID) &&
		linkCompatible(current.CampaignID, links.CampaignID)
}

func linkCompatible(current, next string) bool { return current == "" || next == "" || current == next }

func leadFirstNonEmpty(current, next string) string {
	if current != "" {
		return current
	}
	return next
}

func isLeadID(value string) bool {
	if len(value) != 32 {
		return false
	}
	for _, char := range value {
		if (char < '0' || char > '9') && (char < 'a' || char > 'f') {
			return false
		}
	}
	return true
}

func appendLeadEvent(ctx context.Context, tx *sql.Tx, eventType, aggregateType, aggregateID, actor string,
	payload map[string]any, now time.Time,
) (domain.Event, error) {
	event := domain.Event{
		EventID: newID(), EventType: eventType, SchemaVersion: domain.EventSchemaVersion,
		AggregateType: aggregateType, AggregateID: aggregateID,
		OccurredAt: now, RecordedAt: now, Actor: actor, Payload: payload,
	}
	sequence, err := nextSequence(ctx, tx, aggregateType, aggregateID)
	if err != nil {
		return domain.Event{}, err
	}
	event.Sequence = sequence
	if err := insertEvent(ctx, tx, event); err != nil {
		return domain.Event{}, err
	}
	return event, nil
}
