package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

// S4-B durable campaign orchestration. Campaign rows own the immutable plan
// and terminal outcome; task rows own scheduling state. Implementer tasks
// bind S2 Runs and never create a second attempt lifecycle.

const campaignColumns = `id, project_id, action_id, spec_id, spec_hash,
	policy_revision, plan_hash, plan, status, terminal_reason,
	max_repair_cycles, repair_cycles, max_attempts, max_wall_clock_secs,
	max_parallel, correlation_id, idempotency_key, created_at, updated_at,
	started_at, completed_at`

const campaignTaskColumns = `id, campaign_id, task_key, role, cycle, status,
	depends_on, participant_id, candidate_id, run_id, outcome, error,
	execution_count, max_executions, idempotency_key, dispatch_owner,
	dispatch_token, dispatch_expires_at, created_at, updated_at, started_at,
	completed_at`

const campaignActor = "campaign-orchestrator"

func scanCampaign(row interface{ Scan(dest ...any) error }) (domain.Campaign, error) {
	var c domain.Campaign
	var plan, status, created, updated, started, completed string
	err := row.Scan(
		&c.ID, &c.ProjectID, &c.ActionID, &c.SpecID, &c.SpecHash,
		&c.PolicyRevision, &c.PlanHash, &plan, &status, &c.TerminalReason,
		&c.MaxRepairCycles, &c.RepairCycles, &c.MaxAttempts,
		&c.MaxWallClockSecs, &c.MaxParallel, &c.CorrelationID,
		&c.IdempotencyKey, &created, &updated, &started, &completed,
	)
	if err != nil {
		return domain.Campaign{}, err
	}
	if err := json.Unmarshal([]byte(plan), &c.Plan); err != nil {
		return domain.Campaign{}, err
	}
	c.Status = domain.CampaignStatus(status)
	c.CreatedAt = parseStamp(created)
	c.UpdatedAt = parseStamp(updated)
	c.StartedAt = parseStamp(started)
	c.CompletedAt = parseStamp(completed)
	return c, nil
}

func scanCampaignTask(row interface{ Scan(dest ...any) error }) (domain.CampaignTask, error) {
	var t domain.CampaignTask
	var role, status, deps, created, updated, started, completed, expires string
	err := row.Scan(
		&t.ID, &t.CampaignID, &t.Key, &role, &t.Cycle, &status, &deps,
		&t.ParticipantID, &t.CandidateID, &t.RunID, &t.Outcome, &t.Error,
		&t.ExecutionCount, &t.MaxExecutions, &t.IdempotencyKey,
		&t.DispatchOwner, &t.DispatchToken, &expires, &created, &updated,
		&started, &completed,
	)
	if err != nil {
		return domain.CampaignTask{}, err
	}
	if err := json.Unmarshal([]byte(deps), &t.DependsOn); err != nil {
		return domain.CampaignTask{}, err
	}
	t.Role = domain.TaskRole(role)
	t.Status = domain.CampaignTaskStatus(status)
	t.CreatedAt = parseStamp(created)
	t.UpdatedAt = parseStamp(updated)
	t.StartedAt = parseStamp(started)
	t.CompletedAt = parseStamp(completed)
	t.DispatchExpiresAt = parseStamp(expires)
	return t, nil
}

// CreateCampaign validates the frozen spec/action binding and persists the
// campaign plus its immutable plan's tasks in one transaction.
func (db *DB) CreateCampaign(ctx context.Context, params domain.NewCampaignParams, correlationID string) (domain.Campaign, bool, []domain.Event, error) {
	params, err := params.Validate()
	if err != nil {
		return domain.Campaign{}, false, nil, fmt.Errorf("%w: %v", domain.ErrInvalidInput, err)
	}
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Campaign{}, false, nil, err
	}
	defer func() { _ = tx.Rollback() }()

	spec, err := scanSpec(tx.QueryRowContext(ctx,
		`SELECT `+specColumns+` FROM action_specs WHERE id=?`, params.SpecID))
	if err == sql.ErrNoRows {
		return domain.Campaign{}, false, nil, fmt.Errorf("%w: unknown specId %q", domain.ErrInvalidInput, params.SpecID)
	}
	if err != nil {
		return domain.Campaign{}, false, nil, err
	}
	if err := spec.Validate(); err != nil {
		return domain.Campaign{}, false, nil, fmt.Errorf("%w: frozen spec failed validation: %v", domain.ErrInvalidInput, err)
	}
	if existing, rerr := scanCampaign(tx.QueryRowContext(ctx,
		`SELECT `+campaignColumns+` FROM campaigns WHERE action_id=? AND spec_id=?`,
		spec.ActionID, spec.ID)); rerr == nil {
		if cerr := tx.Commit(); cerr != nil {
			return domain.Campaign{}, false, nil, cerr
		}
		return existing, true, nil, nil
	} else if rerr != sql.ErrNoRows {
		return domain.Campaign{}, false, nil, rerr
	}
	if params.IdempotencyKey != "" {
		if existing, rerr := scanCampaign(tx.QueryRowContext(ctx,
			`SELECT `+campaignColumns+` FROM campaigns WHERE idempotency_key=?`,
			params.IdempotencyKey)); rerr == nil {
			if existing.SpecID != params.SpecID {
				return domain.Campaign{}, false, nil, fmt.Errorf("%w: idempotency key is bound to a different spec", domain.ErrConflict)
			}
			if cerr := tx.Commit(); cerr != nil {
				return domain.Campaign{}, false, nil, cerr
			}
			return existing, true, nil, nil
		} else if rerr != sql.ErrNoRows {
			return domain.Campaign{}, false, nil, rerr
		}
	}
	var actionStatus string
	if err := tx.QueryRowContext(ctx, `SELECT status FROM actions WHERE id=?`, spec.ActionID).Scan(&actionStatus); err != nil {
		if err == sql.ErrNoRows {
			return domain.Campaign{}, false, nil, fmt.Errorf("%w: unknown actionId %q", domain.ErrInvalidInput, spec.ActionID)
		}
		return domain.Campaign{}, false, nil, err
	}
	if actionStatus == string(domain.StatusCompleted) || actionStatus == string(domain.StatusCancelled) {
		return domain.Campaign{}, false, nil, fmt.Errorf("%w: action is %s", domain.ErrInvalidTransition, actionStatus)
	}

	var activeCampaign string
	if err := tx.QueryRowContext(ctx,
		`SELECT id FROM campaigns WHERE action_id=? AND status IN (?,?) LIMIT 1`,
		spec.ActionID, string(domain.CampaignPending), string(domain.CampaignRunning)).Scan(&activeCampaign); err == nil {
		return domain.Campaign{}, false, nil, fmt.Errorf("%w: action already has an active campaign", domain.ErrConflict)
	} else if err != sql.ErrNoRows {
		return domain.Campaign{}, false, nil, err
	}
	plan := domain.BuildCampaignPlan(params.MaxRepairCycles, params.MaxWallClockSecs, params.MaxParallel)
	if err := plan.Validate(); err != nil {
		return domain.Campaign{}, false, nil, fmt.Errorf("%w: campaign plan invalid: %v", domain.ErrInvalidInput, err)
	}
	campaign := domain.Campaign{
		ID:               newID(),
		ProjectID:        spec.ProjectID,
		ActionID:         spec.ActionID,
		SpecID:           spec.ID,
		SpecHash:         spec.ContentHash,
		PolicyRevision:   spec.PolicyRevision,
		PlanHash:         plan.Hash(),
		Plan:             plan,
		Status:           domain.CampaignPending,
		MaxRepairCycles:  params.MaxRepairCycles,
		MaxAttempts:      spec.Budget.MaxAttempts * (params.MaxRepairCycles + 1),
		MaxWallClockSecs: params.MaxWallClockSecs,
		MaxParallel:      params.MaxParallel,
		CorrelationID:    correlationID,
		IdempotencyKey:   params.IdempotencyKey,
		CreatedAt:        now,
		UpdatedAt:        now,
	}
	if err := campaign.Validate(); err != nil {
		return domain.Campaign{}, false, nil, fmt.Errorf("%w: %v", domain.ErrInvalidInput, err)
	}
	rawPlan, err := json.Marshal(plan)
	if err != nil {
		return domain.Campaign{}, false, nil, err
	}
	insertRes, err := tx.ExecContext(ctx,
		`INSERT INTO campaigns(id, project_id, action_id, spec_id, spec_hash,
		 policy_revision, plan_hash, plan, status, terminal_reason,
		 max_repair_cycles, repair_cycles, max_attempts, attempt_budget_version,
		 max_wall_clock_secs, max_parallel, correlation_id, idempotency_key,
		 created_at, updated_at, started_at, completed_at)
		 VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
		 ON CONFLICT DO NOTHING`,
		campaign.ID, campaign.ProjectID, campaign.ActionID, campaign.SpecID,
		campaign.SpecHash, campaign.PolicyRevision, campaign.PlanHash, string(rawPlan),
		string(campaign.Status), "", campaign.MaxRepairCycles, 0, campaign.MaxAttempts, 1,
		campaign.MaxWallClockSecs, campaign.MaxParallel, campaign.CorrelationID,
		campaign.IdempotencyKey, stamp(now), stamp(now), "", "",
	)
	if err != nil {
		return domain.Campaign{}, false, nil, fmt.Errorf("insert campaign: %w", err)
	}
	if affected, aerr := insertRes.RowsAffected(); aerr != nil || affected != 1 {
		if aerr != nil {
			return domain.Campaign{}, false, nil, aerr
		}
		existing, gerr := scanCampaign(tx.QueryRowContext(ctx,
			`SELECT `+campaignColumns+` FROM campaigns WHERE action_id=? AND spec_id=?`,
			campaign.ActionID, campaign.SpecID))
		if gerr != nil {
			return domain.Campaign{}, false, nil, fmt.Errorf("%w: campaign was created concurrently", domain.ErrConflict)
		}
		if cerr := tx.Commit(); cerr != nil {
			return domain.Campaign{}, false, nil, cerr
		}
		return existing, true, nil, nil
	}
	for _, node := range plan.Tasks {
		deps, err := json.Marshal(node.DependsOn)
		if err != nil {
			return domain.Campaign{}, false, nil, err
		}
		participant := ""
		if node.Role == domain.RoleVerifier || node.Role == domain.RoleReviewer {
			participant = domain.ParticipantIdentity(campaign.ID, node.Role, node.Cycle)
		}
		if _, err := tx.ExecContext(ctx,
			`INSERT INTO campaign_tasks(id, campaign_id, task_key, role, cycle,
			 status, depends_on, participant_id, candidate_id, run_id, outcome,
			 error, execution_count, max_executions, idempotency_key,
			 dispatch_owner, dispatch_token, dispatch_expires_at, created_at,
			 updated_at, started_at, completed_at)
			 VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`,
			newID(), campaign.ID, node.Key, string(node.Role), node.Cycle,
			string(domain.TaskPending), string(deps), participant, "", "", "",
			"", 0, node.MaxExecutions, "", "", "", "", stamp(now), stamp(now), "", "",
		); err != nil {
			return domain.Campaign{}, false, nil, fmt.Errorf("insert campaign task %s: %w", node.Key, err)
		}
	}
	payload, _ := json.Marshal(map[string]any{
		"campaignId": campaign.ID, "actionId": campaign.ActionID,
		"specId": campaign.SpecID, "specHash": campaign.SpecHash,
		"planHash": campaign.PlanHash, "taskCount": len(plan.Tasks),
		"maxRepairCycles":  campaign.MaxRepairCycles,
		"maxWallClockSecs": campaign.MaxWallClockSecs,
		"maxParallel":      campaign.MaxParallel,
	})
	event, err := appendCampaignEvent(ctx, tx, domain.EventCampaignCreated, campaign.ID, campaignActor, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.Campaign{}, false, nil, err
	}
	if err := tx.Commit(); err != nil {
		return domain.Campaign{}, false, nil, err
	}
	stored, err := db.GetCampaign(campaign.ID)
	if err != nil {
		return domain.Campaign{}, false, nil, err
	}
	return stored, false, []domain.Event{event}, nil
}

// GetCampaign returns one campaign or ErrNotFound.
func (db *DB) GetCampaign(id string) (domain.Campaign, error) {
	c, err := scanCampaign(db.sql.QueryRow(
		`SELECT `+campaignColumns+` FROM campaigns WHERE id=?`, id))
	if err == sql.ErrNoRows {
		return domain.Campaign{}, domain.ErrNotFound
	}
	return c, err
}

// ListCampaigns returns campaigns newest first.
func (db *DB) ListCampaigns(status domain.CampaignStatus, limit int) ([]domain.Campaign, error) {
	if limit <= 0 || limit > 5000 {
		limit = 100
	}
	q := `SELECT ` + campaignColumns + ` FROM campaigns WHERE 1=1`
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
	var out []domain.Campaign
	for rows.Next() {
		c, err := scanCampaign(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, c)
	}
	return out, rows.Err()
}

// CampaignTasks returns one campaign's task rows in plan order.
func (db *DB) CampaignTasks(campaignID string) ([]domain.CampaignTask, error) {
	rows, err := db.sql.Query(
		`SELECT `+campaignTaskColumns+` FROM campaign_tasks WHERE campaign_id=?
		 ORDER BY cycle ASC,
		 CASE role WHEN 'implementer' THEN 1 WHEN 'verifier' THEN 2 WHEN 'reviewer' THEN 3 ELSE 4 END ASC,
		 task_key ASC`, campaignID)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.CampaignTask
	for rows.Next() {
		t, err := scanCampaignTask(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, t)
	}
	return out, rows.Err()
}

// GetCampaignProjection returns the Core-authoritative read model.
func (db *DB) GetCampaignProjection(campaignID string) (domain.CampaignProjection, error) {
	campaign, err := db.GetCampaign(campaignID)
	if err != nil {
		return domain.CampaignProjection{}, err
	}
	tasks, err := db.CampaignTasks(campaignID)
	if err != nil {
		return domain.CampaignProjection{}, err
	}
	if tasks == nil {
		tasks = []domain.CampaignTask{}
	}
	attempts := 0
	var latestCandidateID string
	for _, task := range tasks {
		if task.Role == domain.RoleImplementer && task.RunID != "" {
			if run, rerr := db.GetRun(task.RunID); rerr == nil {
				attempts += run.AttemptCount
			}
		}
		if task.CandidateID != "" {
			latestCandidateID = task.CandidateID
		}
	}
	now := db.now()
	elapsed := int64(0)
	remaining := int64(campaign.MaxWallClockSecs)
	if !campaign.StartedAt.IsZero() {
		end := now
		if !campaign.CompletedAt.IsZero() {
			end = campaign.CompletedAt
		}
		elapsed = int64(end.Sub(campaign.StartedAt).Seconds())
		if elapsed < 0 {
			elapsed = 0
		}
		remaining -= elapsed
		if remaining < 0 {
			remaining = 0
		}
	}
	projection := domain.CampaignProjection{
		Campaign: campaign,
		Tasks:    tasks,
		Budget: domain.CampaignBudgetUsage{
			MaxAttempts: campaign.MaxAttempts, AttemptsUsed: attempts,
			MaxRepairCycles: campaign.MaxRepairCycles, RepairCycles: campaign.RepairCycles,
			MaxWallClockSecs: campaign.MaxWallClockSecs, ElapsedSecs: elapsed,
			RemainingWallClock: remaining, MaxParallel: campaign.MaxParallel,
			ActiveParticipantMax: campaign.MaxParallel, ProviderBudget: "unknown",
		},
	}
	if latestCandidateID != "" {
		if candidate, cerr := db.GetCandidate(latestCandidateID); cerr == nil {
			projection.Candidate = &candidate
			if decision, derr := db.GetGateDecision(candidate.ID); derr == nil {
				projection.Decision = &decision
			}
		}
	}
	return projection, nil
}

// StartCampaign moves a pending campaign to running and promotes cycle-zero
// tasks. Repeated starts are idempotent and emit no second event.
func (db *DB) StartCampaign(ctx context.Context, campaignID, correlationID string) (domain.Campaign, bool, []domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Campaign{}, false, nil, err
	}
	defer func() { _ = tx.Rollback() }()
	campaign, err := scanCampaign(tx.QueryRowContext(ctx,
		`SELECT `+campaignColumns+` FROM campaigns WHERE id=?`, campaignID))
	if err == sql.ErrNoRows {
		return domain.Campaign{}, false, nil, domain.ErrNotFound
	}
	if err != nil {
		return domain.Campaign{}, false, nil, err
	}
	if campaign.Status != domain.CampaignPending {
		if err := tx.Commit(); err != nil {
			return domain.Campaign{}, false, nil, err
		}
		if campaign.Status == domain.CampaignRunning {
			return campaign, true, nil, nil
		}
		return domain.Campaign{}, false, nil, fmt.Errorf("%w: campaign is %s", domain.ErrInvalidTransition, campaign.Status)
	}
	var actionStatus string
	if err := tx.QueryRowContext(ctx, `SELECT status FROM actions WHERE id=?`, campaign.ActionID).Scan(&actionStatus); err != nil {
		if err == sql.ErrNoRows {
			return domain.Campaign{}, false, nil, fmt.Errorf("%w: unknown actionId %q", domain.ErrInvalidInput, campaign.ActionID)
		}
		return domain.Campaign{}, false, nil, err
	}
	if actionStatus == string(domain.StatusCompleted) || actionStatus == string(domain.StatusCancelled) {
		return domain.Campaign{}, false, nil, fmt.Errorf("%w: action is %s", domain.ErrInvalidTransition, actionStatus)
	}
	startRes, err := tx.ExecContext(ctx,
		`UPDATE campaigns SET status=?, started_at=?, updated_at=? WHERE id=? AND status=?`,
		string(domain.CampaignRunning), stamp(now), stamp(now), campaignID,
		string(domain.CampaignPending),
	)
	if err != nil {
		return domain.Campaign{}, false, nil, err
	}
	if affected, aerr := startRes.RowsAffected(); aerr != nil || affected != 1 {
		if aerr != nil {
			return domain.Campaign{}, false, nil, aerr
		}
		if cerr := tx.Commit(); cerr != nil {
			return domain.Campaign{}, false, nil, cerr
		}
		current, gerr := db.GetCampaign(campaignID)
		if gerr != nil {
			return domain.Campaign{}, false, nil, gerr
		}
		if current.Status == domain.CampaignRunning {
			return current, true, nil, nil
		}
		return domain.Campaign{}, false, nil, fmt.Errorf("%w: campaign is %s", domain.ErrInvalidTransition, current.Status)
	}
	campaign.Status = domain.CampaignRunning
	campaign.StartedAt = now
	campaign.UpdatedAt = now
	payload, _ := json.Marshal(map[string]any{
		"campaignId": campaign.ID, "actionId": campaign.ActionID,
		"specId": campaign.SpecID, "planHash": campaign.PlanHash,
	})
	started, err := appendCampaignEvent(ctx, tx, domain.EventCampaignStarted, campaign.ID, campaignActor, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.Campaign{}, false, nil, err
	}
	events := []domain.Event{started}
	readyEvents, err := promoteTasksTx(ctx, tx, campaign, correlationID, now)
	if err != nil {
		return domain.Campaign{}, false, nil, err
	}
	events = append(events, readyEvents...)
	if err := tx.Commit(); err != nil {
		return domain.Campaign{}, false, nil, err
	}
	stored, err := db.GetCampaign(campaignID)
	if err != nil {
		return domain.Campaign{}, false, nil, err
	}
	return stored, false, events, nil
}

// PromoteReadyTasks advances pending tasks whose dependencies are complete
// and whose cycle has been authorized.
func (db *DB) PromoteReadyTasks(ctx context.Context, campaignID, correlationID string) ([]domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return nil, err
	}
	defer func() { _ = tx.Rollback() }()
	campaign, err := scanCampaign(tx.QueryRowContext(ctx,
		`SELECT `+campaignColumns+` FROM campaigns WHERE id=?`, campaignID))
	if err == sql.ErrNoRows {
		return nil, domain.ErrNotFound
	}
	if err != nil {
		return nil, err
	}
	if campaign.Status != domain.CampaignRunning {
		return nil, nil
	}
	events, err := promoteTasksTx(ctx, tx, campaign, correlationID, now)
	if err != nil {
		return nil, err
	}
	if err := tx.Commit(); err != nil {
		return nil, err
	}
	return events, nil
}

func promoteTasksTx(ctx context.Context, tx *sql.Tx, campaign domain.Campaign, correlationID string, now time.Time) ([]domain.Event, error) {
	tasks, err := campaignTasksTx(ctx, tx, campaign.ID)
	if err != nil {
		return nil, err
	}
	byKey := make(map[string]domain.CampaignTask, len(tasks))
	for _, task := range tasks {
		byKey[task.Key] = task
	}
	var events []domain.Event
	for progress := true; progress; {
		progress = false
		for i := range tasks {
			task := &tasks[i]
			if task.Status != domain.TaskPending || task.Cycle > campaign.RepairCycles {
				continue
			}
			depsReady := true
			for _, dep := range task.DependsOn {
				parent, ok := byKey[dep]
				if !ok || parent.Status != domain.TaskSucceeded {
					depsReady = false
					break
				}
			}
			if !depsReady {
				continue
			}
			candidateID := task.CandidateID
			runID := task.RunID
			if task.Role == domain.RoleVerifier || task.Role == domain.RoleReviewer ||
				task.Role == domain.RoleQualityGate {
				impl := byKey[domain.CycleKey(domain.RoleImplementer, task.Cycle)]
				candidateID = impl.CandidateID
				runID = impl.RunID
			}
			if (task.Role == domain.RoleVerifier || task.Role == domain.RoleReviewer ||
				task.Role == domain.RoleQualityGate) && candidateID == "" {
				continue
			}
			res, err := tx.ExecContext(ctx,
				`UPDATE campaign_tasks SET status=?, candidate_id=?, run_id=?, updated_at=?
				 WHERE id=? AND status=?`,
				string(domain.TaskReady), candidateID, runID, stamp(now),
				task.ID, string(domain.TaskPending),
			)
			if err != nil {
				return nil, err
			}
			affected, err := res.RowsAffected()
			if err != nil {
				return nil, err
			}
			if affected != 1 {
				continue
			}
			task.Status = domain.TaskReady
			task.CandidateID = candidateID
			task.RunID = runID
			byKey[task.Key] = *task
			payload, _ := json.Marshal(map[string]any{
				"campaignId": campaign.ID, "taskId": task.ID, "taskKey": task.Key,
				"role": task.Role, "cycle": task.Cycle, "candidateId": candidateID,
				"runId": runID,
			})
			event, err := appendCampaignEvent(ctx, tx, domain.EventCampaignTaskReady, campaign.ID, campaignActor, correlationID, now, mustDecode(payload))
			if err != nil {
				return nil, err
			}
			events = append(events, event)
			progress = true
		}
	}
	return events, nil
}

// DispatchImplementer atomically creates the spec-bound Run and binds it to
// the implementer task. The Run retains all S2 lease/fencing/recovery
// semantics; the campaign only records the dependency.
func (db *DB) DispatchImplementer(ctx context.Context, campaignID, taskID, taskText, correlationID string) (domain.Campaign, domain.Run, []domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Campaign{}, domain.Run{}, nil, err
	}
	defer func() { _ = tx.Rollback() }()
	campaign, err := scanCampaign(tx.QueryRowContext(ctx,
		`SELECT `+campaignColumns+` FROM campaigns WHERE id=?`, campaignID))
	if err == sql.ErrNoRows {
		return domain.Campaign{}, domain.Run{}, nil, domain.ErrNotFound
	}
	if err != nil {
		return domain.Campaign{}, domain.Run{}, nil, err
	}
	if campaign.Status != domain.CampaignRunning {
		return domain.Campaign{}, domain.Run{}, nil, fmt.Errorf("%w: campaign is %s", domain.ErrInvalidTransition, campaign.Status)
	}
	task, err := scanCampaignTask(tx.QueryRowContext(ctx,
		`SELECT `+campaignTaskColumns+` FROM campaign_tasks WHERE id=? AND campaign_id=?`,
		taskID, campaignID))
	if err != nil {
		if err == sql.ErrNoRows {
			return domain.Campaign{}, domain.Run{}, nil, domain.ErrNotFound
		}
		return domain.Campaign{}, domain.Run{}, nil, err
	}
	if task.Role != domain.RoleImplementer {
		return domain.Campaign{}, domain.Run{}, nil, fmt.Errorf("%w: task %s is not an implementer task", domain.ErrInvalidTransition, task.Key)
	}
	if task.Status == domain.TaskDispatched && task.RunID != "" {
		if run, rerr := scanRun(tx.QueryRowContext(ctx, `SELECT `+runColumns+` FROM runs WHERE id=?`, task.RunID)); rerr == nil {
			if cerr := tx.Commit(); cerr != nil {
				return domain.Campaign{}, domain.Run{}, nil, cerr
			}
			return campaign, run, nil, nil
		}
	}
	if task.Status != domain.TaskReady {
		return domain.Campaign{}, domain.Run{}, nil, fmt.Errorf("%w: implementer task is %s", domain.ErrInvalidTransition, task.Status)
	}
	if task.Cycle > campaign.RepairCycles {
		return domain.Campaign{}, domain.Run{}, nil, fmt.Errorf("%w: repair cycle is not authorized", domain.ErrUnauthorized)
	}
	var attemptsUsed int
	if err := tx.QueryRowContext(ctx,
		`SELECT COALESCE(SUM(r.attempt_count),0) FROM runs r
		 JOIN campaign_tasks t ON t.run_id=r.id
		 WHERE t.campaign_id=? AND t.role=?`,
		campaignID, string(domain.RoleImplementer)).Scan(&attemptsUsed); err != nil {
		return domain.Campaign{}, domain.Run{}, nil, err
	}
	if attemptsUsed >= campaign.MaxAttempts {
		return domain.Campaign{}, domain.Run{}, nil, fmt.Errorf("%w: campaign attempt budget exhausted", domain.ErrBudgetExhausted)
	}
	spec, err := scanSpec(tx.QueryRowContext(ctx,
		`SELECT `+specColumns+` FROM action_specs WHERE id=?`, campaign.SpecID))
	if err != nil {
		return domain.Campaign{}, domain.Run{}, nil, err
	}
	text := strings.TrimSpace(taskText)
	if text == "" {
		text = spec.Goal
	}
	if task.Cycle > 0 {
		text = fmt.Sprintf("%s Repair cycle %d for the same authorized Action and scope.", text, task.Cycle)
	}
	if len(text) > domain.MaxPaseoTaskChars {
		text = text[:domain.MaxPaseoTaskChars]
	}
	params, err := domain.NewRunParams{
		ProjectID: spec.ProjectID, ActionID: spec.ActionID,
		WorkloadKind: domain.PaseoTaskKind,
		WorkloadInput: map[string]any{
			"task": text,
		},
		MaxAttempts: spec.Budget.MaxAttempts, TimeoutSecs: spec.Budget.TimeoutSecs,
		ExecutionMode: string(domain.ExecTrustedLocal), Provider: spec.Provider,
		Model: spec.Model, PaseoMode: spec.PaseoMode, SpecID: spec.ID,
	}.Validate()
	if err != nil {
		return domain.Campaign{}, domain.Run{}, nil, fmt.Errorf("%w: %v", domain.ErrInvalidInput, err)
	}
	run, runEvent, err := createRunTx(ctx, tx, params, correlationID, now)
	if err != nil {
		return domain.Campaign{}, domain.Run{}, nil, err
	}
	dispatchToken := newID()
	res, err := tx.ExecContext(ctx,
		`UPDATE campaign_tasks SET status=?, execution_count=execution_count+1,
		 run_id=?, idempotency_key=?, dispatch_owner=?, dispatch_token=?,
		 dispatch_expires_at=?, started_at=CASE WHEN started_at='' THEN ? ELSE started_at END,
		 updated_at=?
		 WHERE id=? AND campaign_id=? AND status=?`,
		string(domain.TaskDispatched), run.ID, "campaign-"+campaign.ID+"-"+task.Key,
		campaignActor, dispatchToken,
		stamp(now.Add(time.Duration(run.TimeoutSecs+120)*time.Second)),
		stamp(now), stamp(now), task.ID, campaignID, string(domain.TaskReady),
	)
	if err != nil {
		return domain.Campaign{}, domain.Run{}, nil, err
	}
	affected, err := res.RowsAffected()
	if err != nil {
		return domain.Campaign{}, domain.Run{}, nil, err
	}
	if affected != 1 {
		return domain.Campaign{}, domain.Run{}, nil, fmt.Errorf("%w: implementer task was claimed concurrently", domain.ErrConflict)
	}
	payload, _ := json.Marshal(map[string]any{
		"campaignId": campaign.ID, "taskId": task.ID, "taskKey": task.Key,
		"role": task.Role, "cycle": task.Cycle, "runId": run.ID,
		"generation": run.LeaseGeneration,
	})
	event, err := appendCampaignEvent(ctx, tx, domain.EventCampaignTaskDispatched, campaign.ID, campaignActor, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.Campaign{}, domain.Run{}, nil, err
	}
	if err := tx.Commit(); err != nil {
		return domain.Campaign{}, domain.Run{}, nil, err
	}
	return campaign, run, []domain.Event{runEvent, event}, nil
}

// ClaimParticipantTask leases one verifier/reviewer task to this
// orchestrator instance. The task identity and candidate are fixed before
// any process starts.
func (db *DB) ClaimParticipantTask(ctx context.Context, campaignID, taskID, owner string, ttl time.Duration, correlationID string) (domain.CampaignTask, bool, []domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	defer func() { _ = tx.Rollback() }()
	campaign, err := scanCampaign(tx.QueryRowContext(ctx,
		`SELECT `+campaignColumns+` FROM campaigns WHERE id=?`, campaignID))
	if err == sql.ErrNoRows {
		return domain.CampaignTask{}, false, nil, domain.ErrNotFound
	}
	if err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	if campaign.Status != domain.CampaignRunning {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: campaign is %s", domain.ErrInvalidTransition, campaign.Status)
	}
	task, err := scanCampaignTask(tx.QueryRowContext(ctx,
		`SELECT `+campaignTaskColumns+` FROM campaign_tasks WHERE id=? AND campaign_id=?`,
		taskID, campaignID))
	if err != nil {
		if err == sql.ErrNoRows {
			return domain.CampaignTask{}, false, nil, domain.ErrNotFound
		}
		return domain.CampaignTask{}, false, nil, err
	}
	if task.Role != domain.RoleVerifier && task.Role != domain.RoleReviewer {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: task %s is not a participant task", domain.ErrInvalidTransition, task.Key)
	}
	if task.Status != domain.TaskReady {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: participant task is %s", domain.ErrInvalidTransition, task.Status)
	}
	if task.CandidateID == "" {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: participant task has no frozen candidate", domain.ErrInvalidTransition)
	}
	if task.Cycle > campaign.RepairCycles {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: repair cycle is not authorized", domain.ErrUnauthorized)
	}
	var active int
	if err := tx.QueryRowContext(ctx,
		`SELECT COUNT(*) FROM campaign_tasks WHERE campaign_id=? AND role IN (?,?)
		 AND status IN (?,?)`,
		campaignID, string(domain.RoleVerifier), string(domain.RoleReviewer),
		string(domain.TaskDispatched), string(domain.TaskRunning),
	).Scan(&active); err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	if active >= campaign.MaxParallel {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: campaign participant slots are full", domain.ErrConflict)
	}
	idempotencyKey := task.IdempotencyKey
	if idempotencyKey == "" {
		idempotencyKey = fmt.Sprintf("campaign-%s-%s-%s", campaignID, task.Key, task.CandidateID)
		if len(idempotencyKey) > domain.MaxIdempotencyKeyChars {
			idempotencyKey = idempotencyKey[:domain.MaxIdempotencyKeyChars]
		}
	}
	dispatchToken := newID()
	res, err := tx.ExecContext(ctx,
		`UPDATE campaign_tasks SET status=?, execution_count=execution_count+1,
		 idempotency_key=?, dispatch_owner=?, dispatch_token=?, dispatch_expires_at=?,
		 started_at=CASE WHEN started_at='' THEN ? ELSE started_at END, updated_at=?
		 WHERE id=? AND campaign_id=? AND status=?`,
		string(domain.TaskDispatched), idempotencyKey, owner, dispatchToken,
		stamp(now.Add(ttl)), stamp(now), stamp(now), task.ID, campaignID,
		string(domain.TaskReady),
	)
	if err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	affected, err := res.RowsAffected()
	if err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	if affected != 1 {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: participant task was claimed concurrently", domain.ErrConflict)
	}
	task.Status = domain.TaskDispatched
	task.ExecutionCount++
	task.IdempotencyKey = idempotencyKey
	task.DispatchOwner = owner
	task.DispatchToken = dispatchToken
	task.DispatchExpiresAt = now.Add(ttl)
	if task.StartedAt.IsZero() {
		task.StartedAt = now
	}
	task.UpdatedAt = now
	payload, _ := json.Marshal(map[string]any{
		"campaignId": campaignID, "taskId": task.ID, "taskKey": task.Key,
		"role": task.Role, "cycle": task.Cycle, "candidateId": task.CandidateID,
		"participantId": task.ParticipantID, "execution": task.ExecutionCount,
	})
	event, err := appendCampaignEvent(ctx, tx, domain.EventCampaignTaskDispatched, campaignID, campaignActor, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	if err := tx.Commit(); err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	return task, true, []domain.Event{event}, nil
}

// ClaimGateTask leases the Core-internal gate task; no external process is
// started. Evaluation and persistence remain deterministic.
func (db *DB) ClaimGateTask(ctx context.Context, campaignID, taskID, owner string, ttl time.Duration, correlationID string) (domain.CampaignTask, bool, []domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	defer func() { _ = tx.Rollback() }()
	campaign, err := scanCampaign(tx.QueryRowContext(ctx,
		`SELECT `+campaignColumns+` FROM campaigns WHERE id=?`, campaignID))
	if err != nil {
		if err == sql.ErrNoRows {
			return domain.CampaignTask{}, false, nil, domain.ErrNotFound
		}
		return domain.CampaignTask{}, false, nil, err
	}
	if campaign.Status != domain.CampaignRunning {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: campaign is %s", domain.ErrInvalidTransition, campaign.Status)
	}
	task, err := scanCampaignTask(tx.QueryRowContext(ctx,
		`SELECT `+campaignTaskColumns+` FROM campaign_tasks WHERE id=? AND campaign_id=?`,
		taskID, campaignID))
	if err != nil {
		if err == sql.ErrNoRows {
			return domain.CampaignTask{}, false, nil, domain.ErrNotFound
		}
		return domain.CampaignTask{}, false, nil, err
	}
	if task.Role != domain.RoleQualityGate {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: task %s is not a gate task", domain.ErrInvalidTransition, task.Key)
	}
	if task.Status != domain.TaskReady {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: gate task is %s", domain.ErrInvalidTransition, task.Status)
	}
	if task.CandidateID == "" {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: gate task has no candidate", domain.ErrInvalidTransition)
	}
	if task.Cycle > campaign.RepairCycles {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: repair cycle is not authorized", domain.ErrUnauthorized)
	}
	dispatchToken := newID()
	res, err := tx.ExecContext(ctx,
		`UPDATE campaign_tasks SET status=?, execution_count=execution_count+1,
		 dispatch_owner=?, dispatch_token=?, dispatch_expires_at=?,
		 started_at=CASE WHEN started_at='' THEN ? ELSE started_at END, updated_at=?
		 WHERE id=? AND campaign_id=? AND status=?`,
		string(domain.TaskDispatched), owner, dispatchToken, stamp(now.Add(ttl)),
		stamp(now), stamp(now), task.ID, campaignID, string(domain.TaskReady),
	)
	if err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	affected, err := res.RowsAffected()
	if err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	if affected != 1 {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: gate task was claimed concurrently", domain.ErrConflict)
	}
	task.Status = domain.TaskDispatched
	task.ExecutionCount++
	task.DispatchOwner = owner
	task.DispatchToken = dispatchToken
	task.DispatchExpiresAt = now.Add(ttl)
	if task.StartedAt.IsZero() {
		task.StartedAt = now
	}
	task.UpdatedAt = now
	payload, _ := json.Marshal(map[string]any{
		"campaignId": campaignID, "taskId": task.ID, "taskKey": task.Key,
		"role": task.Role, "cycle": task.Cycle, "candidateId": task.CandidateID,
	})
	event, err := appendCampaignEvent(ctx, tx, domain.EventCampaignTaskDispatched, campaignID, campaignActor, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	if err := tx.Commit(); err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	return task, true, []domain.Event{event}, nil
}

// CompleteCampaignTask records an observed successful task execution. It is
// idempotent: a repeated completion returns the existing task without a
// second event.
func (db *DB) CompleteCampaignTask(ctx context.Context, campaignID, taskID, outcome, dispatchToken, correlationID string) (domain.CampaignTask, bool, []domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	defer func() { _ = tx.Rollback() }()
	task, err := scanCampaignTask(tx.QueryRowContext(ctx,
		`SELECT `+campaignTaskColumns+` FROM campaign_tasks WHERE id=? AND campaign_id=?`,
		taskID, campaignID))
	if err != nil {
		if err == sql.ErrNoRows {
			return domain.CampaignTask{}, false, nil, domain.ErrNotFound
		}
		return domain.CampaignTask{}, false, nil, err
	}
	if task.Status == domain.TaskSucceeded {
		if cerr := tx.Commit(); cerr != nil {
			return domain.CampaignTask{}, false, nil, cerr
		}
		return task, true, nil, nil
	}
	if task.Status != domain.TaskDispatched && task.Status != domain.TaskRunning {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: task is %s", domain.ErrInvalidTransition, task.Status)
	}
	if dispatchToken == "" || task.DispatchToken != dispatchToken {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: stale task dispatch owner", domain.ErrStaleGeneration)
	}
	if task.Role == domain.RoleImplementer {
		run, rerr := scanRun(tx.QueryRowContext(ctx,
			`SELECT `+runColumns+` FROM runs WHERE id=?`, task.RunID))
		if rerr != nil {
			return domain.CampaignTask{}, false, nil, rerr
		}
		if run.Status != domain.RunSucceeded {
			return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: implementer Run is %s", domain.ErrInvalidTransition, run.Status)
		}
		candidate, rerr := scanCandidate(tx.QueryRowContext(ctx,
			`SELECT `+candidateColumns+` FROM candidates WHERE implementer_run_id=? ORDER BY created_at DESC LIMIT 1`,
			run.ID))
		if rerr != nil {
			return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: implementer Run has no frozen candidate: %v", domain.ErrInvalidTransition, rerr)
		}
		if candidate.State != domain.CandidateFrozen || candidate.SpecID != run.SpecID {
			return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: candidate is not a frozen identity for this Run", domain.ErrInvalidTransition)
		}
		if _, err := tx.ExecContext(ctx,
			`UPDATE campaign_tasks SET candidate_id=?, outcome=?, updated_at=? WHERE id=?`,
			candidate.ID, candidate.CommitSHA, stamp(now), task.ID,
		); err != nil {
			return domain.CampaignTask{}, false, nil, err
		}
		task.CandidateID = candidate.ID
		outcome = candidate.CommitSHA
	}
	res, err := tx.ExecContext(ctx,
		`UPDATE campaign_tasks SET status=?, outcome=?, error='', completed_at=?,
		 updated_at=?, dispatch_owner='', dispatch_token='', dispatch_expires_at=''
		 WHERE id=? AND campaign_id=? AND status IN (?,?) AND dispatch_token=?`,
		string(domain.TaskSucceeded), outcome, stamp(now), stamp(now),
		task.ID, campaignID, string(domain.TaskDispatched), string(domain.TaskRunning), dispatchToken,
	)
	if err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	affected, err := res.RowsAffected()
	if err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	if affected != 1 {
		return domain.CampaignTask{}, false, nil, fmt.Errorf("%w: task completed concurrently", domain.ErrConflict)
	}
	task.Status = domain.TaskSucceeded
	task.Outcome = outcome
	task.Error = ""
	task.CompletedAt = now
	task.UpdatedAt = now
	task.DispatchOwner = ""
	task.DispatchToken = ""
	task.DispatchExpiresAt = time.Time{}
	payload, _ := json.Marshal(map[string]any{
		"campaignId": campaignID, "taskId": task.ID, "taskKey": task.Key,
		"role": task.Role, "cycle": task.Cycle, "candidateId": task.CandidateID,
		"runId": task.RunID, "outcome": outcome,
	})
	event, err := appendCampaignEvent(ctx, tx, domain.EventCampaignTaskCompleted, campaignID, campaignActor, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	if err := tx.Commit(); err != nil {
		return domain.CampaignTask{}, false, nil, err
	}
	return task, false, []domain.Event{event}, nil
}

// FailParticipantTask applies bounded retry or terminal campaign blocking
// when a verifier/reviewer process cannot produce evidence.
func (db *DB) FailParticipantTask(ctx context.Context, campaignID, taskID, reason, dispatchToken, correlationID string) (domain.CampaignTask, []domain.Event, error) {
	reason = domain.SanitizeReason(reason)
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.CampaignTask{}, nil, err
	}
	defer func() { _ = tx.Rollback() }()
	campaign, err := scanCampaign(tx.QueryRowContext(ctx,
		`SELECT `+campaignColumns+` FROM campaigns WHERE id=?`, campaignID))
	if err != nil {
		if err == sql.ErrNoRows {
			return domain.CampaignTask{}, nil, domain.ErrNotFound
		}
		return domain.CampaignTask{}, nil, err
	}
	task, err := scanCampaignTask(tx.QueryRowContext(ctx,
		`SELECT `+campaignTaskColumns+` FROM campaign_tasks WHERE id=? AND campaign_id=?`,
		taskID, campaignID))
	if err != nil {
		if err == sql.ErrNoRows {
			return domain.CampaignTask{}, nil, domain.ErrNotFound
		}
		return domain.CampaignTask{}, nil, err
	}
	if task.Status.Terminal() {
		return task, nil, nil
	}
	if dispatchToken == "" || task.DispatchToken != dispatchToken {
		return task, nil, fmt.Errorf("%w: stale task dispatch owner", domain.ErrStaleGeneration)
	}
	if campaign.Status != domain.CampaignRunning {
		if _, events, ferr := finishCampaignTx(ctx, tx, campaignID, domain.CampaignCancelled, "campaign is no longer running", domain.EventCampaignCancelled, correlationID, now); ferr != nil {
			return domain.CampaignTask{}, nil, ferr
		} else if cerr := tx.Commit(); cerr != nil {
			return domain.CampaignTask{}, nil, cerr
		} else {
			return task, events, nil
		}
	}
	retry := task.ExecutionCount < task.MaxExecutions
	if retry {
		retryRes, err := tx.ExecContext(ctx,
			`UPDATE campaign_tasks SET status=?, error=?, outcome='retry', dispatch_owner='',
			 dispatch_token='', dispatch_expires_at='', updated_at=?
			 WHERE id=? AND status IN (?,?) AND dispatch_token=?`,
			string(domain.TaskReady), reason, stamp(now), task.ID,
			string(domain.TaskDispatched), string(domain.TaskRunning), dispatchToken,
		)
		if err != nil {
			return domain.CampaignTask{}, nil, err
		}
		if affected, aerr := retryRes.RowsAffected(); aerr != nil || affected != 1 {
			if aerr != nil {
				return domain.CampaignTask{}, nil, aerr
			}
			if cerr := tx.Commit(); cerr != nil {
				return domain.CampaignTask{}, nil, cerr
			}
			current, gerr := db.getCampaignTask(campaignID, taskID)
			return current, nil, gerr
		}
		task.Status = domain.TaskReady
		task.Error = reason
		task.Outcome = "retry"
		task.DispatchOwner = ""
		task.DispatchToken = ""
		task.DispatchExpiresAt = time.Time{}
		task.UpdatedAt = now
		payload, _ := json.Marshal(map[string]any{
			"campaignId": campaignID, "taskId": task.ID, "taskKey": task.Key,
			"role": task.Role, "cycle": task.Cycle, "error": reason,
			"willRetry": true, "execution": task.ExecutionCount,
		})
		event, err := appendCampaignEvent(ctx, tx, domain.EventCampaignTaskFailed, campaignID, campaignActor, correlationID, now, mustDecode(payload))
		if err != nil {
			return domain.CampaignTask{}, nil, err
		}
		if err := tx.Commit(); err != nil {
			return domain.CampaignTask{}, nil, err
		}
		return task, []domain.Event{event}, nil
	}
	failRes, err := tx.ExecContext(ctx,
		`UPDATE campaign_tasks SET status=?, error=?, outcome='exhausted',
		 completed_at=?, updated_at=?, dispatch_owner='', dispatch_token='',
		 dispatch_expires_at='' WHERE id=? AND status IN (?,?) AND dispatch_token=?`,
		string(domain.TaskFailed), reason, stamp(now), stamp(now), task.ID,
		string(domain.TaskDispatched), string(domain.TaskRunning), dispatchToken,
	)
	if err != nil {
		return domain.CampaignTask{}, nil, err
	}
	if affected, aerr := failRes.RowsAffected(); aerr != nil || affected != 1 {
		if aerr != nil {
			return domain.CampaignTask{}, nil, aerr
		}
		if cerr := tx.Commit(); cerr != nil {
			return domain.CampaignTask{}, nil, cerr
		}
		current, gerr := db.getCampaignTask(campaignID, taskID)
		return current, nil, gerr
	}
	task.Status = domain.TaskFailed
	task.Error = reason
	task.Outcome = "exhausted"
	task.CompletedAt = now
	task.UpdatedAt = now
	payload, _ := json.Marshal(map[string]any{
		"campaignId": campaignID, "taskId": task.ID, "taskKey": task.Key,
		"role": task.Role, "cycle": task.Cycle, "error": reason,
		"willRetry": false, "execution": task.ExecutionCount,
	})
	failed, err := appendCampaignEvent(ctx, tx, domain.EventCampaignTaskFailed, campaignID, campaignActor, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.CampaignTask{}, nil, err
	}
	events := []domain.Event{failed}
	_, finished, err := finishCampaignTx(ctx, tx, campaignID, domain.CampaignBlocked,
		string(task.Role)+" participant exhausted: "+reason, domain.EventCampaignFinished, correlationID, now)
	if err != nil {
		return domain.CampaignTask{}, nil, err
	}
	events = append(events, finished...)
	if err := tx.Commit(); err != nil {
		return domain.CampaignTask{}, nil, err
	}
	return task, events, nil
}

// FailImplementerTask terminates a campaign when its spec-bound Run fails
// and no earlier candidate can advance. It never authorizes repair: only a
// Quality Gate REPAIR_REQUIRED result can do that.
func (db *DB) FailImplementerTask(ctx context.Context, campaignID, taskID, reason, dispatchToken, correlationID string) (domain.CampaignTask, []domain.Event, error) {
	reason = domain.SanitizeReason(reason)
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.CampaignTask{}, nil, err
	}
	defer func() { _ = tx.Rollback() }()
	task, err := scanCampaignTask(tx.QueryRowContext(ctx,
		`SELECT `+campaignTaskColumns+` FROM campaign_tasks WHERE id=? AND campaign_id=?`,
		taskID, campaignID))
	if err != nil {
		if err == sql.ErrNoRows {
			return domain.CampaignTask{}, nil, domain.ErrNotFound
		}
		return domain.CampaignTask{}, nil, err
	}
	if task.Status.Terminal() {
		return task, nil, nil
	}
	if dispatchToken == "" || task.DispatchToken != dispatchToken {
		return task, nil, fmt.Errorf("%w: stale task dispatch owner", domain.ErrStaleGeneration)
	}
	failRes, err := tx.ExecContext(ctx,
		`UPDATE campaign_tasks SET status=?, error=?, outcome='failed', completed_at=?,
		 updated_at=?, dispatch_owner='', dispatch_token='', dispatch_expires_at=''
		 WHERE id=? AND status IN (?,?) AND dispatch_token=?`,
		string(domain.TaskFailed), reason, stamp(now), stamp(now), task.ID,
		string(domain.TaskDispatched), string(domain.TaskRunning), dispatchToken,
	)
	if err != nil {
		return domain.CampaignTask{}, nil, err
	}
	if affected, aerr := failRes.RowsAffected(); aerr != nil || affected != 1 {
		if aerr != nil {
			return domain.CampaignTask{}, nil, aerr
		}
		if cerr := tx.Commit(); cerr != nil {
			return domain.CampaignTask{}, nil, cerr
		}
		current, gerr := db.getCampaignTask(campaignID, taskID)
		return current, nil, gerr
	}
	task.Status = domain.TaskFailed
	task.Error = reason
	task.Outcome = "failed"
	task.CompletedAt = now
	task.UpdatedAt = now
	payload, _ := json.Marshal(map[string]any{
		"campaignId": campaignID, "taskId": task.ID, "taskKey": task.Key,
		"role": task.Role, "cycle": task.Cycle, "error": reason,
		"willRetry": false, "runId": task.RunID,
	})
	failed, err := appendCampaignEvent(ctx, tx, domain.EventCampaignTaskFailed, campaignID, campaignActor, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.CampaignTask{}, nil, err
	}
	events := []domain.Event{failed}
	_, finished, err := finishCampaignTx(ctx, tx, campaignID, domain.CampaignFailed,
		"implementer Run failed: "+reason, domain.EventCampaignFinished, correlationID, now)
	if err != nil {
		return domain.CampaignTask{}, nil, err
	}
	events = append(events, finished...)
	if err := tx.Commit(); err != nil {
		return domain.CampaignTask{}, nil, err
	}
	return task, events, nil
}

// BlockCampaign is the explicit fail-closed path for unresolved recovery,
// timeouts and missing mandatory evidence. It never starts a repair cycle.
func (db *DB) BlockCampaign(ctx context.Context, campaignID, reason, correlationID string) (domain.Campaign, []domain.Event, error) {
	reason = domain.SanitizeReason(reason)
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Campaign{}, nil, err
	}
	defer func() { _ = tx.Rollback() }()
	campaign, events, err := finishCampaignTx(ctx, tx, campaignID, domain.CampaignBlocked,
		reason, domain.EventCampaignFinished, correlationID, now)
	if err != nil {
		return domain.Campaign{}, nil, err
	}
	if err := tx.Commit(); err != nil {
		return domain.Campaign{}, nil, err
	}
	return campaign, events, nil
}

// EvaluateCampaignGate evaluates the exact frozen candidate and atomically
// applies the single authoritative outcome to the campaign task graph.
func (db *DB) EvaluateCampaignGate(ctx context.Context, campaignID, taskID, correlationID string) (domain.GateDecision, bool, []domain.Event, error) {
	task, err := db.getCampaignTask(campaignID, taskID)
	if err != nil {
		return domain.GateDecision{}, false, nil, err
	}
	campaign, err := db.GetCampaign(campaignID)
	if err != nil {
		return domain.GateDecision{}, false, nil, err
	}
	if campaign.Status != domain.CampaignRunning {
		return domain.GateDecision{}, false, nil, fmt.Errorf("%w: campaign is %s", domain.ErrInvalidTransition, campaign.Status)
	}
	if task.Role != domain.RoleQualityGate || task.CandidateID == "" {
		return domain.GateDecision{}, false, nil, fmt.Errorf("%w: task is not a candidate gate", domain.ErrInvalidTransition)
	}
	decision, final, _, gateEvent, err := db.EvaluateGate(ctx, task.CandidateID, correlationID)
	if err != nil {
		return domain.GateDecision{}, false, nil, err
	}
	if !final {
		_, events, ferr := db.FailParticipantTask(ctx, campaignID, taskID,
			"quality gate requires complete verifier and reviewer evidence", task.DispatchToken, correlationID)
		return decision, false, events, ferr
	}
	_, events, err := db.ApplyGateOutcome(ctx, campaignID, taskID, task.DispatchToken, decision, correlationID)
	if err != nil {
		return domain.GateDecision{}, final, nil, err
	}
	if gateEvent != nil {
		events = append([]domain.Event{*gateEvent}, events...)
	}
	return decision, true, events, nil
}

// ApplyGateOutcome persists the task completion and the campaign-level
// consequence of the one authoritative Quality Gate decision.
func (db *DB) ApplyGateOutcome(ctx context.Context, campaignID, taskID, dispatchToken string, decision domain.GateDecision, correlationID string) (domain.Campaign, []domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Campaign{}, nil, err
	}
	defer func() { _ = tx.Rollback() }()
	campaign, err := scanCampaign(tx.QueryRowContext(ctx,
		`SELECT `+campaignColumns+` FROM campaigns WHERE id=?`, campaignID))
	if err != nil {
		if err == sql.ErrNoRows {
			return domain.Campaign{}, nil, domain.ErrNotFound
		}
		return domain.Campaign{}, nil, err
	}
	task, err := scanCampaignTask(tx.QueryRowContext(ctx,
		`SELECT `+campaignTaskColumns+` FROM campaign_tasks WHERE id=? AND campaign_id=?`,
		taskID, campaignID))
	if err != nil {
		if err == sql.ErrNoRows {
			return domain.Campaign{}, nil, domain.ErrNotFound
		}
		return domain.Campaign{}, nil, err
	}
	if task.Role != domain.RoleQualityGate || decision.CandidateID != task.CandidateID {
		return domain.Campaign{}, nil, fmt.Errorf("%w: gate decision does not bind this campaign task", domain.ErrUnauthorized)
	}
	if campaign.Status.Terminal() {
		if err := tx.Commit(); err != nil {
			return domain.Campaign{}, nil, err
		}
		return campaign, nil, nil
	}
	if task.Status == domain.TaskSucceeded {
		if err := tx.Commit(); err != nil {
			return domain.Campaign{}, nil, err
		}
		return campaign, nil, nil
	}
	if task.Status != domain.TaskDispatched && task.Status != domain.TaskRunning {
		return domain.Campaign{}, nil, fmt.Errorf("%w: gate task is %s", domain.ErrInvalidTransition, task.Status)
	}
	if dispatchToken == "" || task.DispatchToken != dispatchToken {
		return domain.Campaign{}, nil, fmt.Errorf("%w: stale gate dispatch owner", domain.ErrStaleGeneration)
	}
	durable, derr := scanDecision(tx.QueryRowContext(ctx,
		`SELECT `+decisionColumns+` FROM gate_decisions WHERE candidate_id=?`, task.CandidateID))
	if derr != nil {
		return domain.Campaign{}, nil, fmt.Errorf("%w: no durable decision for campaign gate", domain.ErrNotFound)
	}
	if durable.ID != decision.ID || durable.Status != decision.Status ||
		durable.CandidateSHA != decision.CandidateSHA {
		return domain.Campaign{}, nil, fmt.Errorf("%w: gate decision does not match durable Core state", domain.ErrUnauthorized)
	}
	gateRes, err := tx.ExecContext(ctx,
		`UPDATE campaign_tasks SET status=?, outcome=?, error=?, completed_at=?,
		 updated_at=?, dispatch_owner='', dispatch_token='', dispatch_expires_at=''
		 WHERE id=? AND status IN (?,?) AND dispatch_token=?`,
		string(domain.TaskSucceeded), string(decision.Status),
		strings.Join(decision.Reasons, "; "), stamp(now), stamp(now), task.ID,
		string(domain.TaskDispatched), string(domain.TaskRunning), dispatchToken,
	)
	if err != nil {
		return domain.Campaign{}, nil, err
	}
	if affected, aerr := gateRes.RowsAffected(); aerr != nil || affected != 1 {
		if aerr != nil {
			return domain.Campaign{}, nil, aerr
		}
		if cerr := tx.Commit(); cerr != nil {
			return domain.Campaign{}, nil, cerr
		}
		current, gerr := db.GetCampaign(campaignID)
		return current, nil, gerr
	}
	payload, _ := json.Marshal(map[string]any{
		"campaignId": campaignID, "taskId": task.ID, "taskKey": task.Key,
		"role": task.Role, "cycle": task.Cycle, "candidateId": task.CandidateID,
		"outcome": decision.Status,
	})
	completed, err := appendCampaignEvent(ctx, tx, domain.EventCampaignTaskCompleted, campaignID, campaignActor, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.Campaign{}, nil, err
	}
	events := []domain.Event{completed}

	switch decision.Status {
	case domain.GateAccepted:
		_, finished, ferr := finishCampaignTx(ctx, tx, campaignID, domain.CampaignAccepted,
			"quality gate ACCEPTED", domain.EventCampaignFinished, correlationID, now)
		if ferr != nil {
			return domain.Campaign{}, nil, ferr
		}
		events = append(events, finished...)
	case domain.GateRepair:
		if campaign.RepairCycles < campaign.MaxRepairCycles {
			if _, err := tx.ExecContext(ctx,
				`UPDATE campaigns SET repair_cycles=repair_cycles+1, updated_at=?
				 WHERE id=? AND status=?`,
				stamp(now), campaignID, string(domain.CampaignRunning),
			); err != nil {
				return domain.Campaign{}, nil, err
			}
			campaign.RepairCycles++
			repairPayload, _ := json.Marshal(map[string]any{
				"campaignId": campaignID, "candidateId": task.CandidateID,
				"decisionId": decision.ID, "repairCycle": campaign.RepairCycles,
				"maxRepairCycles": campaign.MaxRepairCycles,
				"reasons":         decision.Reasons,
			})
			repairEvent, err := appendCampaignEvent(ctx, tx, domain.EventCampaignRepairAuthorized, campaignID, campaignActor, correlationID, now, mustDecode(repairPayload))
			if err != nil {
				return domain.Campaign{}, nil, err
			}
			events = append(events, repairEvent)
		} else {
			_, finished, ferr := finishCampaignTx(ctx, tx, campaignID, domain.CampaignRepairNeeded,
				"repair budget exhausted", domain.EventCampaignFinished, correlationID, now)
			if ferr != nil {
				return domain.Campaign{}, nil, ferr
			}
			events = append(events, finished...)
		}
	case domain.GateBlocked:
		_, finished, ferr := finishCampaignTx(ctx, tx, campaignID, domain.CampaignBlocked,
			"quality gate BLOCKED: "+strings.Join(decision.Reasons, "; "),
			domain.EventCampaignFinished, correlationID, now)
		if ferr != nil {
			return domain.Campaign{}, nil, ferr
		}
		events = append(events, finished...)
	case domain.GateHumanRequired:
		_, finished, ferr := finishCampaignTx(ctx, tx, campaignID, domain.CampaignHumanRequired,
			"quality gate HUMAN_REQUIRED: "+strings.Join(decision.Reasons, "; "),
			domain.EventCampaignFinished, correlationID, now)
		if ferr != nil {
			return domain.Campaign{}, nil, ferr
		}
		events = append(events, finished...)
	default:
		_, finished, ferr := finishCampaignTx(ctx, tx, campaignID, domain.CampaignBlocked,
			"quality gate did not establish acceptance: "+string(decision.Status),
			domain.EventCampaignFinished, correlationID, now)
		if ferr != nil {
			return domain.Campaign{}, nil, ferr
		}
		events = append(events, finished...)
	}
	if err := tx.Commit(); err != nil {
		return domain.Campaign{}, nil, err
	}
	stored, err := db.GetCampaign(campaignID)
	if err != nil {
		return domain.Campaign{}, nil, err
	}
	return stored, events, nil
}

// CancelCampaign cancels the campaign, open tasks and active implementer
// Runs in one transaction. Participant process teardown is the
// orchestrator's responsibility after this commit.
func (db *DB) CancelCampaign(ctx context.Context, campaignID, reason, correlationID string) (domain.Campaign, []domain.Event, error) {
	reason = domain.SanitizeReason(reason)
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return domain.Campaign{}, nil, err
	}
	defer func() { _ = tx.Rollback() }()
	campaign, err := scanCampaign(tx.QueryRowContext(ctx,
		`SELECT `+campaignColumns+` FROM campaigns WHERE id=?`, campaignID))
	if err != nil {
		if err == sql.ErrNoRows {
			return domain.Campaign{}, nil, domain.ErrNotFound
		}
		return domain.Campaign{}, nil, err
	}
	if campaign.Status.Terminal() {
		if err := tx.Commit(); err != nil {
			return domain.Campaign{}, nil, err
		}
		if campaign.Status == domain.CampaignCancelled {
			return campaign, nil, nil
		}
		return domain.Campaign{}, nil, fmt.Errorf("%w: campaign is %s", domain.ErrInvalidTransition, campaign.Status)
	}
	tasks, err := campaignTasksTx(ctx, tx, campaignID)
	if err != nil {
		return domain.Campaign{}, nil, err
	}
	var events []domain.Event
	for _, task := range tasks {
		if task.Role == domain.RoleImplementer && task.RunID != "" && !task.Status.Terminal() {
			run, rerr := scanRun(tx.QueryRowContext(ctx, `SELECT `+runColumns+` FROM runs WHERE id=?`, task.RunID))
			if rerr == nil && !run.Status.Terminal() {
				cancelled, runEvents, cerr := cancelRunTx(ctx, tx, run.ID, correlationID, now, campaignActor)
				if cerr != nil {
					return domain.Campaign{}, nil, cerr
				}
				run = cancelled
				events = append(events, runEvents...)
			} else if rerr != nil && rerr != sql.ErrNoRows {
				return domain.Campaign{}, nil, rerr
			}
		}
	}
	finished, finishEvents, err := finishCampaignTx(ctx, tx, campaignID, domain.CampaignCancelled,
		reason, domain.EventCampaignCancelled, correlationID, now)
	if err != nil {
		return domain.Campaign{}, nil, err
	}
	events = append(events, finishEvents...)
	if err := tx.Commit(); err != nil {
		return domain.Campaign{}, nil, err
	}
	return finished, events, nil
}

// ExpireCampaigns fails closed on wall-clock exhaustion and cancels active
// Runs. It is safe to call repeatedly and emits at most one terminal event
// per campaign.
func (db *DB) ExpireCampaigns(ctx context.Context, correlationID string) ([]domain.Event, error) {
	now := db.now()
	tx, err := db.sql.BeginTx(ctx, nil)
	if err != nil {
		return nil, err
	}
	defer func() { _ = tx.Rollback() }()
	rows, err := tx.QueryContext(ctx,
		`SELECT `+campaignColumns+` FROM campaigns WHERE status=?`,
		string(domain.CampaignRunning))
	if err != nil {
		return nil, err
	}
	var expired []domain.Campaign
	for rows.Next() {
		c, serr := scanCampaign(rows)
		if serr != nil {
			_ = rows.Close()
			return nil, serr
		}
		if c.StartedAt.IsZero() {
			continue
		}
		if now.Before(c.StartedAt.Add(time.Duration(c.MaxWallClockSecs) * time.Second)) {
			continue
		}
		expired = append(expired, c)
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return nil, err
	}
	var events []domain.Event
	for _, campaign := range expired {
		tasks, terr := campaignTasksTx(ctx, tx, campaign.ID)
		if terr != nil {
			return nil, terr
		}
		for _, task := range tasks {
			if task.Role == domain.RoleImplementer && task.RunID != "" && !task.Status.Terminal() {
				run, rerr := scanRun(tx.QueryRowContext(ctx, `SELECT `+runColumns+` FROM runs WHERE id=?`, task.RunID))
				if rerr == nil && !run.Status.Terminal() {
					_, runEvents, cerr := cancelRunTx(ctx, tx, run.ID, correlationID, now, campaignActor)
					if cerr != nil {
						return nil, cerr
					}
					events = append(events, runEvents...)
				} else if rerr != nil && rerr != sql.ErrNoRows {
					return nil, rerr
				}
			}
		}
		_, finishEvents, ferr := finishCampaignTx(ctx, tx, campaign.ID, domain.CampaignBlocked,
			"campaign wall-clock budget exhausted", domain.EventCampaignFinished, correlationID, now)
		if ferr != nil {
			return nil, ferr
		}
		events = append(events, finishEvents...)
	}
	if err := tx.Commit(); err != nil {
		return nil, err
	}
	return events, nil
}

// RunRecoveryUnresolved reports whether an Implementer Run still has an
// unproven external Paseo generation. Such a Run must never be replaced or
// let dependent verification proceed.
func (db *DB) RunRecoveryUnresolved(runID string) (bool, string, error) {
	var bindingStatus, recoveryStatus string
	err := db.sql.QueryRow(
		`SELECT b.status, COALESCE(rec.status,'')
		 FROM paseo_bindings b
		 LEFT JOIN paseo_recovery rec ON rec.attempt_id=b.attempt_id
		 WHERE b.run_id=? ORDER BY b.seq DESC LIMIT 1`, runID,
	).Scan(&bindingStatus, &recoveryStatus)
	if err == sql.ErrNoRows {
		return false, "", nil
	}
	if err != nil {
		return false, "", err
	}
	binding := domain.PaseoSessionStatus(bindingStatus)
	if !binding.Terminal() {
		return true, "paseo session binding " + bindingStatus + " is not terminal", nil
	}
	if recoveryStatus == "" {
		if binding == domain.PaseoOrphaned {
			return true, "orphaned paseo session has no recovery receipt", nil
		}
		return false, "", nil
	}
	recovery := domain.PaseoRecoveryStatus(recoveryStatus)
	if !recovery.SafeForReplacement() {
		return true, "paseo recovery is " + recoveryStatus, nil
	}
	return false, "", nil
}

// TaskEvidence returns the concrete durable evidence linked by the task's
// idempotency key. It is used to confirm participant success without
// trusting the child process exit status.
func (db *DB) TaskEvidence(task domain.CampaignTask) (outcome string, ok bool, err error) {
	switch task.Role {
	case domain.RoleVerifier:
		var status string
		err = db.sql.QueryRow(
			`SELECT status FROM candidate_verifications
			 WHERE candidate_id=? AND idempotency_key=? AND actor=?`,
			task.CandidateID, task.IdempotencyKey, task.ParticipantID).Scan(&status)
		if err == sql.ErrNoRows {
			return "", false, nil
		}
		return status, err == nil, err
	case domain.RoleReviewer:
		var verdict string
		err = db.sql.QueryRow(
			`SELECT verdict FROM candidate_reviews
			 WHERE candidate_id=? AND idempotency_key=? AND actor=?`,
			task.CandidateID, task.IdempotencyKey, task.ParticipantID).Scan(&verdict)
		if err == sql.ErrNoRows {
			return "", false, nil
		}
		return verdict, err == nil, err
	default:
		return "", false, nil
	}
}

func (db *DB) getCampaignTask(campaignID, taskID string) (domain.CampaignTask, error) {
	t, err := scanCampaignTask(db.sql.QueryRow(
		`SELECT `+campaignTaskColumns+` FROM campaign_tasks WHERE id=? AND campaign_id=?`,
		taskID, campaignID))
	if err == sql.ErrNoRows {
		return domain.CampaignTask{}, domain.ErrNotFound
	}
	return t, err
}

func campaignTasksTx(ctx context.Context, tx *sql.Tx, campaignID string) ([]domain.CampaignTask, error) {
	rows, err := tx.QueryContext(ctx,
		`SELECT `+campaignTaskColumns+` FROM campaign_tasks WHERE campaign_id=?
		 ORDER BY cycle ASC,
		 CASE role WHEN 'implementer' THEN 1 WHEN 'verifier' THEN 2 WHEN 'reviewer' THEN 3 ELSE 4 END ASC,
		 task_key ASC`, campaignID)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()
	var out []domain.CampaignTask
	for rows.Next() {
		t, err := scanCampaignTask(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, t)
	}
	return out, rows.Err()
}

func finishCampaignTx(ctx context.Context, tx *sql.Tx, campaignID string, status domain.CampaignStatus, reason, eventType, correlationID string, now time.Time) (domain.Campaign, []domain.Event, error) {
	campaign, err := scanCampaign(tx.QueryRowContext(ctx,
		`SELECT `+campaignColumns+` FROM campaigns WHERE id=?`, campaignID))
	if err != nil {
		if err == sql.ErrNoRows {
			return domain.Campaign{}, nil, domain.ErrNotFound
		}
		return domain.Campaign{}, nil, err
	}
	if campaign.Status.Terminal() {
		return campaign, nil, nil
	}
	reason = domain.SanitizeReason(reason)
	finishRes, err := tx.ExecContext(ctx,
		`UPDATE campaigns SET status=?, terminal_reason=?, completed_at=?, updated_at=?
		 WHERE id=? AND status=? AND completed_at=''`,
		string(status), reason, stamp(now), stamp(now), campaignID,
		string(campaign.Status),
	)
	if err != nil {
		return domain.Campaign{}, nil, err
	}
	if affected, aerr := finishRes.RowsAffected(); aerr != nil || affected != 1 {
		if aerr != nil {
			return domain.Campaign{}, nil, aerr
		}
		current, gerr := scanCampaign(tx.QueryRowContext(ctx,
			`SELECT `+campaignColumns+` FROM campaigns WHERE id=?`, campaignID))
		if gerr != nil {
			return domain.Campaign{}, nil, gerr
		}
		return current, nil, nil
	}
	campaign.Status = status
	campaign.TerminalReason = reason
	campaign.CompletedAt = now
	campaign.UpdatedAt = now
	activeEvents, err := cancelActiveImplementerRunsTx(ctx, tx, campaignID, correlationID, now)
	if err != nil {
		return domain.Campaign{}, nil, err
	}
	events, err := cancelOpenTasksTx(ctx, tx, campaignID, reason, correlationID, now)
	if err != nil {
		return domain.Campaign{}, nil, err
	}
	payload, _ := json.Marshal(map[string]any{
		"campaignId": campaignID, "status": status, "reason": reason,
		"repairCycles": campaign.RepairCycles,
	})
	finished, err := appendCampaignEvent(ctx, tx, eventType, campaignID, campaignActor, correlationID, now, mustDecode(payload))
	if err != nil {
		return domain.Campaign{}, nil, err
	}
	return campaign, append(activeEvents, append(events, finished)...), nil
}

// cancelActiveImplementerRunsTx cancels any still-live Run bound to an
// implementer task when a campaign terminates. The Run cancellation path
// keeps its own fencing, candidate rejection and orphan recording.
func cancelActiveImplementerRunsTx(ctx context.Context, tx *sql.Tx, campaignID, correlationID string, now time.Time) ([]domain.Event, error) {
	rows, err := tx.QueryContext(ctx,
		`SELECT r.id FROM runs r JOIN campaign_tasks t ON t.run_id=r.id
		 WHERE t.campaign_id=? AND t.role=? AND r.status IN (?,?,?)`,
		campaignID, string(domain.RoleImplementer),
		string(domain.RunQueued), string(domain.RunLeased), string(domain.RunRunning))
	if err != nil {
		return nil, err
	}
	var runIDs []string
	for rows.Next() {
		var runID string
		if err := rows.Scan(&runID); err != nil {
			_ = rows.Close()
			return nil, err
		}
		runIDs = append(runIDs, runID)
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return nil, err
	}
	var events []domain.Event
	for _, runID := range runIDs {
		_, runEvents, cerr := cancelRunTx(ctx, tx, runID, correlationID, now, campaignActor)
		if cerr != nil {
			return nil, cerr
		}
		events = append(events, runEvents...)
	}
	return events, nil
}

func cancelOpenTasksTx(ctx context.Context, tx *sql.Tx, campaignID, reason, correlationID string, now time.Time) ([]domain.Event, error) {
	rows, err := tx.QueryContext(ctx,
		`SELECT `+campaignTaskColumns+` FROM campaign_tasks WHERE campaign_id=?
		 AND status IN (?,?,?,?) ORDER BY cycle ASC,
		 CASE role WHEN 'implementer' THEN 1 WHEN 'verifier' THEN 2 WHEN 'reviewer' THEN 3 ELSE 4 END ASC,
		 task_key ASC`,
		campaignID, string(domain.TaskPending), string(domain.TaskReady),
		string(domain.TaskDispatched), string(domain.TaskRunning))
	if err != nil {
		return nil, err
	}
	var open []domain.CampaignTask
	for rows.Next() {
		t, serr := scanCampaignTask(rows)
		if serr != nil {
			_ = rows.Close()
			return nil, serr
		}
		open = append(open, t)
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return nil, err
	}
	var events []domain.Event
	for _, task := range open {
		if _, err := tx.ExecContext(ctx,
			`UPDATE campaign_tasks SET status=?, error=?, completed_at=?, updated_at=?,
			 dispatch_owner='', dispatch_token='', dispatch_expires_at=''
			 WHERE id=? AND status IN (?,?,?,?)`,
			string(domain.TaskCancelled), reason, stamp(now), stamp(now), task.ID,
			string(domain.TaskPending), string(domain.TaskReady),
			string(domain.TaskDispatched), string(domain.TaskRunning),
		); err != nil {
			return nil, err
		}
		payload, _ := json.Marshal(map[string]any{
			"campaignId": campaignID, "taskId": task.ID, "taskKey": task.Key,
			"role": task.Role, "cycle": task.Cycle, "reason": reason,
		})
		event, err := appendCampaignEvent(ctx, tx, domain.EventCampaignTaskCancelled, campaignID, campaignActor, correlationID, now, mustDecode(payload))
		if err != nil {
			return nil, err
		}
		events = append(events, event)
	}
	return events, nil
}

func appendCampaignEvent(ctx context.Context, tx *sql.Tx, eventType, campaignID, actor, correlationID string, now time.Time, payload map[string]any) (domain.Event, error) {
	event := domain.Event{
		EventID:       newID(),
		EventType:     eventType,
		SchemaVersion: domain.EventSchemaVersion,
		AggregateType: domain.AggregateCampaign,
		AggregateID:   campaignID,
		OccurredAt:    now,
		RecordedAt:    now,
		CorrelationID: correlationID,
		Actor:         actor,
		Payload:       payload,
	}
	seq, err := nextSequence(ctx, tx, domain.AggregateCampaign, campaignID)
	if err != nil {
		return domain.Event{}, err
	}
	event.Sequence = seq
	if err := insertEvent(ctx, tx, event); err != nil {
		return domain.Event{}, err
	}
	return event, nil
}
