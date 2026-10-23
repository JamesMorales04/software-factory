// Package orchestrator is the Core-owned deterministic campaign scheduler.
// It advances durable campaign tasks, observes existing Run/Attempt state
// and spawns the separate verifier/reviewer processes with scoped
// credentials. It never infers success from model or process output: only
// durable Core evidence advances a task.
package orchestrator

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
	projectreg "github.com/JamesMorales04/software-factory/core/internal/project"
	"github.com/JamesMorales04/software-factory/core/internal/recovery"
	"github.com/JamesMorales04/software-factory/core/internal/store"
)

// Publisher receives committed domain events for SSE fan-out.
type Publisher interface {
	Publish(domain.Event)
}

// Config binds the local Core URL, scoped participant credentials and
// executable paths. Missing binaries fail tasks closed; they are never
// replaced with an in-process claim of independence.
type Config struct {
	CoreURL       string
	VerifierToken string
	ReviewerToken string
	VerifierBin   string
	ReviewerBin   string
	Interval      time.Duration
	MaxSlots      int
	Logger        *slog.Logger
}

type runningProcess struct {
	campaignID string
	task       domain.CampaignTask
	cancel     context.CancelFunc
}

// Engine advances campaigns on a bounded ticker. Only one reconcile pass
// runs at a time in each instance; concurrent Core instances contend
// through transactional task claims and unique gate decisions.
type Engine struct {
	db     *store.DB
	hub    Publisher
	cfg    Config
	sem    chan struct{}
	wake   chan struct{}
	procs  map[string]*runningProcess
	mu     sync.Mutex
	log    *slog.Logger
	instID string
	root   context.Context
	wg     sync.WaitGroup
}

// New builds an engine. MaxSlots bounds concurrent verifier/reviewer
// processes across all campaigns in this Core instance.
func New(db *store.DB, hub Publisher, cfg Config) *Engine {
	if cfg.Interval <= 0 {
		cfg.Interval = 500 * time.Millisecond
	}
	if cfg.MaxSlots <= 0 {
		cfg.MaxSlots = 4
	}
	if cfg.MaxSlots > 8 {
		cfg.MaxSlots = 8
	}
	logger := cfg.Logger
	if logger == nil {
		logger = slog.Default()
	}
	return &Engine{
		db: db, hub: hub, cfg: cfg,
		sem: make(chan struct{}, cfg.MaxSlots), wake: make(chan struct{}, 1),
		procs: map[string]*runningProcess{}, log: logger,
		instID: "orchestrator-" + randomID(),
	}
}

// Wake nudges the scheduler after an API intent.
func (e *Engine) Wake() {
	select {
	case e.wake <- struct{}{}:
	default:
	}
}

// Run blocks until ctx is canceled, reconciling immediately and then on
// every tick or wake-up.
func (e *Engine) Run(ctx context.Context) {
	e.root = ctx
	e.reconcile(ctx)
	ticker := time.NewTicker(e.cfg.Interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			e.killAll()
			e.wg.Wait()
			return
		case <-ticker.C:
			e.reconcile(ctx)
		case <-e.wake:
			e.reconcile(ctx)
		}
	}
}

// CancelCampaign commits cancellation and tears down local participant
// processes. External Paseo sessions remain owned by the existing Run
// cancellation/recovery path.
func (e *Engine) CancelCampaign(ctx context.Context, campaignID, reason, correlationID string) error {
	if strings.TrimSpace(reason) == "" {
		reason = "operator cancelled"
	}
	if strings.TrimSpace(correlationID) == "" {
		correlationID = "campaign-cancel-" + randomID()
	}
	campaign, events, err := e.db.CancelCampaign(ctx, campaignID, reason, correlationID)
	if err != nil {
		return err
	}
	for _, event := range events {
		e.hub.Publish(event)
	}
	e.killCampaign(campaign.ID)
	e.log.Info("campaign cancelled", "campaignId", campaign.ID)
	return nil
}

func (e *Engine) reconcile(ctx context.Context) {
	if ctx.Err() != nil {
		return
	}
	expired, err := e.db.ExpireCampaigns(ctx, "campaign-sweep-"+randomID())
	if err != nil {
		e.log.Error("campaign expiry sweep failed", "error", err.Error())
	}
	e.publish(expired)
	e.pruneProcesses()
	e.reconcilePendingActionState(ctx)

	campaigns, err := e.db.ListCampaigns("", domain.CampaignRunning, 5000)
	if err != nil {
		e.log.Error("list running campaigns failed", "error", err.Error())
		return
	}
	for _, campaign := range campaigns {
		if ctx.Err() != nil {
			return
		}
		e.reconcileCampaign(ctx, campaign)
	}
}

func (e *Engine) reconcilePendingActionState(ctx context.Context) {
	pending, err := e.db.ListCampaigns("", domain.CampaignPending, 5000)
	if err != nil {
		e.log.Error("list pending campaigns failed", "error", err.Error())
		return
	}
	for _, campaign := range pending {
		action, aerr := e.db.GetAction(campaign.ActionID)
		if aerr != nil {
			e.log.Error("load pending campaign action failed", "campaignId", campaign.ID, "error", aerr.Error())
			continue
		}
		switch action.Status {
		case domain.StatusCancelled:
			_, events, cerr := e.db.CancelCampaign(ctx, campaign.ID,
				"action cancelled outside the campaign", "campaign-pending-"+randomID())
			if cerr != nil {
				e.log.Error("cancel pending campaign failed", "campaignId", campaign.ID, "error", cerr.Error())
				continue
			}
			e.publish(events)
		case domain.StatusCompleted:
			e.block(ctx, campaign, "action completed outside the campaign", "campaign-pending-"+randomID())
		}
	}
}

func (e *Engine) reconcileCampaign(ctx context.Context, campaign domain.Campaign) {
	correlation := "campaign-tick-" + randomID()
	if applicable, authorized, aerr := e.db.CheckLeadCampaignAuthorization(ctx, campaign.ID); aerr != nil {
		e.log.Error("lead authorization check failed", "campaignId", campaign.ID, "error", aerr.Error())
		return
	} else if applicable && !authorized {
		e.cancelCampaignRuns(ctx, campaign, "lead delegation is expired, revoked, or stale", correlation)
		e.block(ctx, campaign, "lead delegation is expired, revoked, or stale", correlation)
		return
	}
	action, err := e.db.GetAction(campaign.ActionID)
	if err != nil {
		e.log.Error("load campaign action failed", "campaignId", campaign.ID, "error", err.Error())
		return
	}
	switch action.Status {
	case domain.StatusCancelled:
		cancelled, events, cerr := e.db.CancelCampaign(ctx, campaign.ID,
			"action cancelled outside the campaign", correlation)
		if cerr != nil {
			e.log.Error("cancel campaign after action cancellation failed", "campaignId", campaign.ID, "error", cerr.Error())
			return
		}
		e.publish(events)
		e.killCampaign(cancelled.ID)
		return
	case domain.StatusCompleted:
		e.block(ctx, campaign, "action completed outside the campaign", correlation)
		return
	}
	project, perr := e.db.GetProject(campaign.ProjectID)
	if perr != nil {
		e.block(ctx, campaign, "campaign project is unavailable", correlation)
		return
	}
	if !project.Active() {
		e.cancelCampaignRuns(ctx, campaign, "project is "+string(project.Status), correlation)
		e.block(ctx, campaign, "project "+project.ID+" is "+string(project.Status), correlation)
		return
	}
	spec, serr := e.db.GetActionSpec(campaign.SpecID)
	if serr != nil {
		e.block(ctx, campaign, "frozen spec is unavailable", correlation)
		return
	}
	if berr := spec.ValidateProjectBinding(project); berr != nil {
		e.cancelCampaignRuns(ctx, campaign, "spec binding rejected", correlation)
		e.block(ctx, campaign, "frozen spec binding rejected: "+berr.Error(), correlation)
		return
	}
	events, err := e.db.PromoteReadyTasks(ctx, campaign.ID, correlation)
	if err != nil {
		e.log.Error("promote campaign tasks failed", "campaignId", campaign.ID, "error", err.Error())
		return
	}
	e.publish(events)

	projection, err := e.db.GetCampaignProjection(campaign.ID)
	if err != nil {
		e.log.Error("load campaign projection failed", "campaignId", campaign.ID, "error", err.Error())
		return
	}
	for _, task := range projection.Tasks {
		switch task.Role {
		case domain.RoleImplementer:
			e.reconcileImplementer(ctx, projection.Campaign, task, correlation)
		case domain.RoleVerifier, domain.RoleReviewer:
			e.reconcileParticipant(ctx, projection.Campaign, task, correlation)
		case domain.RoleQualityGate:
			e.reconcileGate(ctx, projection.Campaign, task, correlation)
		}
	}

	events, err = e.db.PromoteReadyTasks(ctx, campaign.ID, correlation)
	if err != nil {
		e.log.Error("promote repaired campaign tasks failed", "campaignId", campaign.ID, "error", err.Error())
		return
	}
	e.publish(events)
	projection, err = e.db.GetCampaignProjection(campaign.ID)
	if err != nil {
		return
	}
	e.dispatchReady(ctx, projection)
}

func (e *Engine) reconcileImplementer(ctx context.Context, campaign domain.Campaign, task domain.CampaignTask, correlation string) {
	if task.Status != domain.TaskDispatched || task.RunID == "" {
		return
	}
	run, err := e.db.GetRun(task.RunID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			e.failImplementer(ctx, campaign, task, "implementer Run is missing", correlation)
			return
		}
		e.log.Error("load implementer run failed", "runId", task.RunID, "error", err.Error())
		return
	}
	if run.Status == domain.RunQueued {
		spec, serr := e.db.GetActionSpec(campaign.SpecID)
		if serr != nil {
			return
		}
		eligible, werr := e.workerSupports(ctx, spec.Capabilities, spec.ExecutionMode)
		if werr != nil {
			return
		}
		if !eligible && !campaign.StartedAt.IsZero() && time.Since(campaign.StartedAt) > 20*time.Second {
			e.cancelCampaignRuns(ctx, campaign, "no eligible Worker", correlation)
			e.block(ctx, campaign,
				"no online Worker supports required capabilities "+strings.Join(capabilityStrings(spec.Capabilities), ","),
				correlation)
		}
		return
	}
	switch run.Status {
	case domain.RunSucceeded:
		completed, _, events, cerr := e.db.CompleteCampaignTask(ctx, campaign.ID, task.ID, string(run.Status), task.DispatchToken, correlation)
		if cerr != nil {
			e.failImplementer(ctx, campaign, task, "candidate validation failed: "+cerr.Error(), correlation)
			return
		}
		e.publish(events)
		e.log.Info("implementer task completed", "campaignId", campaign.ID,
			"taskKey", completed.Key, "candidateId", completed.CandidateID)
	case domain.RunFailed:
		if run.WorkloadKind == domain.GitIntegrationKind &&
			strings.Contains(strings.ToLower(run.Error), "integrate dependency candidate") {
			e.block(ctx, campaign, "deterministic integration conflict: "+run.Error, correlation)
			return
		}
		spec, serr := e.db.GetActionSpec(campaign.SpecID)
		if serr != nil {
			e.block(ctx, campaign, "frozen spec is unavailable during recovery", correlation)
			return
		}
		unresolved, reason, rerr := e.db.RunRecoveryUnresolved(run.ID)
		if rerr != nil {
			e.log.Error("recovery check failed", "runId", run.ID, "error", rerr.Error())
			return
		}
		used, maxAttempts, uerr := e.db.CampaignAttemptsUsed(campaign.ID)
		if uerr != nil {
			e.log.Error("attempt budget lookup failed", "campaignId", campaign.ID, "error", uerr.Error())
			return
		}
		classification := recovery.Classify(recovery.Input{
			Run: run, Spec: spec, AttemptsUsed: used, MaxAttempts: maxAttempts,
			UnresolvedRecovery: unresolved, UnresolvedReason: reason,
			CleanupUnconfirmed: cleanupUnconfirmed(run),
		})
		if recovery.Recoverable(classification.Class) && used < maxAttempts {
			_, events, aerr := e.db.AuthorizeImplementerRecovery(ctx, campaign.ID, task.ID,
				task.DispatchToken, classification.Class, classification.Reason, correlation)
			if aerr != nil {
				e.log.Error("authorize implementer recovery failed", "campaignId", campaign.ID,
					"taskId", task.ID, "error", aerr.Error())
				e.block(ctx, campaign, "implementer recovery could not be authorized: "+aerr.Error(), correlation)
				return
			}
			e.publish(events)
			e.log.Warn("implementer recovery authorized", "campaignId", campaign.ID,
				"taskKey", task.Key, "failureClass", classification.Class,
				"attemptsUsed", used, "maxAttempts", maxAttempts,
				"reason", classification.Reason)
			return
		}
		switch classification.Class {
		case domain.RecoverableImplementationFailure, domain.RecoverableExecutionFailure:
			e.failImplementer(ctx, campaign, task,
				fmt.Sprintf("implementer attempt budget exhausted (%d/%d): %s: %s",
					used, maxAttempts, classification.Class, classification.Reason), correlation)
		case domain.PolicyOrSecurityBlocked, domain.UnresolvedExternalRecovery, domain.InfrastructureBlocked:
			e.block(ctx, campaign, string(classification.Class)+": "+classification.Reason, correlation)
		case domain.OracleDefectSuspected:
			e.escalate(ctx, campaign, string(classification.Class)+": "+classification.Reason, correlation)
		default:
			e.block(ctx, campaign, "implementer failure is not safely recoverable: "+classification.Reason, correlation)
		}
	case domain.RunCancelled:
		e.block(ctx, campaign, "implementer Run was cancelled outside the campaign", correlation)
	}
}

func (e *Engine) reconcileParticipant(ctx context.Context, campaign domain.Campaign, task domain.CampaignTask, correlation string) {
	if task.Status != domain.TaskDispatched && task.Status != domain.TaskRunning {
		return
	}
	outcome, ok, err := e.db.TaskEvidence(task)
	if err != nil {
		e.log.Error("read participant evidence failed", "taskId", task.ID, "error", err.Error())
		return
	}
	if ok {
		_, _, events, cerr := e.db.CompleteCampaignTask(ctx, campaign.ID, task.ID, outcome, task.DispatchToken, correlation)
		if cerr != nil {
			e.log.Error("complete participant task failed", "taskId", task.ID, "error", cerr.Error())
			return
		}
		e.publish(events)
		return
	}
	if e.hasProcess(task.ID) {
		return
	}
	if time.Now().UTC().Before(task.DispatchExpiresAt) {
		return
	}
	_, events, ferr := e.db.FailParticipantTask(ctx, campaign.ID, task.ID,
		"participant dispatch expired without durable evidence", task.DispatchToken, correlation)
	if ferr != nil {
		e.log.Error("expire participant task failed", "taskId", task.ID, "error", ferr.Error())
		return
	}
	e.publish(events)
}

func (e *Engine) reconcileGate(ctx context.Context, campaign domain.Campaign, task domain.CampaignTask, correlation string) {
	if task.Status != domain.TaskDispatched {
		return
	}
	decision, final, events, err := e.db.EvaluateCampaignGate(ctx, campaign.ID, task.ID, correlation)
	if err != nil {
		e.log.Error("campaign gate evaluation failed", "campaignId", campaign.ID, "error", err.Error())
		e.block(ctx, campaign, "quality gate evaluation failed: "+err.Error(), correlation)
		return
	}
	e.publish(events)
	if !final {
		return
	}
	e.log.Info("campaign gate decided", "campaignId", campaign.ID,
		"candidateId", decision.CandidateID, "status", string(decision.Status))
}

func (e *Engine) cancelCampaignRuns(ctx context.Context, campaign domain.Campaign, reason, correlation string) {
	tasks, err := e.db.CampaignTasks(campaign.ID)
	if err != nil {
		return
	}
	for _, task := range tasks {
		if task.RunID == "" {
			continue
		}
		run, rerr := e.db.GetRun(task.RunID)
		if rerr != nil || run.Status.Terminal() {
			continue
		}
		if _, _, cerr := e.db.CancelRun(ctx, run.ID, correlation+": "+reason); cerr != nil {
			e.log.Error("cancel campaign run failed", "runId", run.ID, "error", cerr.Error())
		}
	}
}

func (e *Engine) dispatchReady(ctx context.Context, projection domain.CampaignProjection) {
	for _, task := range projection.Tasks {
		if task.Status != domain.TaskReady {
			continue
		}
		switch task.Role {
		case domain.RoleImplementer:
			e.dispatchImplementer(ctx, projection.Campaign, task)
		case domain.RoleVerifier, domain.RoleReviewer:
			e.dispatchParticipant(ctx, projection.Campaign, task)
		case domain.RoleQualityGate:
			e.dispatchGate(ctx, projection.Campaign, task)
		}
	}
}

func (e *Engine) dispatchImplementer(ctx context.Context, campaign domain.Campaign, task domain.CampaignTask) {
	node, _ := campaign.Plan.TaskPlan(task.Key)
	specID := campaign.SpecID
	if node.SpecID != "" {
		specID = node.SpecID
	}
	spec, err := e.db.GetActionSpec(specID)
	if err != nil {
		e.log.Error("load campaign spec failed", "campaignId", campaign.ID, "error", err.Error())
		return
	}
	project, err := e.db.GetProject(campaign.ProjectID)
	if err != nil {
		e.block(ctx, campaign, "campaign project is unavailable", "campaign-dispatch-"+randomID())
		return
	}
	if !project.Active() {
		e.block(ctx, campaign, "project "+project.ID+" is "+string(project.Status), "campaign-dispatch-"+randomID())
		return
	}
	if err := spec.ValidateProjectBinding(project); err != nil {
		e.block(ctx, campaign, "frozen spec binding rejected: "+err.Error(), "campaign-dispatch-"+randomID())
		return
	}
	if project.Registered() {
		if len(node.DependsOn) == 0 {
			if rerr := projectreg.RevalidateBound(ctx, spec.Repository, spec.RepositoryIdentity, spec.BaseRevision); rerr != nil {
				e.block(ctx, campaign, "repository revalidation failed: "+rerr.Error(), "campaign-dispatch-"+randomID())
				return
			}
		} else {
			inspected, rerr := projectreg.Inspect(ctx, spec.Repository, project.BaseRefs)
			if rerr != nil || inspected.Identity != spec.RepositoryIdentity {
				e.block(ctx, campaign, "repository identity changed before dependent dispatch", "campaign-dispatch-"+randomID())
				return
			}
		}
	}
	eligible, err := e.workerSupports(ctx, spec.Capabilities, spec.ExecutionMode)
	if err != nil {
		e.log.Error("worker capability lookup failed", "campaignId", campaign.ID, "error", err.Error())
		return
	}
	if !eligible {
		grace := 20 * time.Second
		if campaign.StartedAt.IsZero() || time.Since(campaign.StartedAt) < grace {
			return
		}
		e.block(ctx, campaign,
			"no online Worker supports required capabilities "+strings.Join(capabilityStrings(spec.Capabilities), ","),
			"campaign-dispatch-"+randomID())
		return
	}
	used, maxAttempts, _ := e.db.CampaignAttemptsUsed(campaign.ID)
	text := e.implementerTaskText(campaign, task, spec, used, maxAttempts)
	_, _, events, err := e.db.DispatchImplementer(ctx, campaign.ID, task.ID, text,
		"campaign-dispatch-"+randomID())
	if err != nil {
		if errors.Is(err, domain.ErrBudgetExhausted) {
			e.block(ctx, campaign, err.Error(), "campaign-dispatch-"+randomID())
			return
		}
		if !errors.Is(err, domain.ErrConflict) && !errors.Is(err, domain.ErrInvalidTransition) {
			e.log.Error("dispatch implementer failed", "campaignId", campaign.ID, "error", err.Error())
		}
		return
	}
	e.publish(events)
	e.log.Info("implementer dispatched", "campaignId", campaign.ID,
		"cycle", task.Cycle, "taskKey", task.Key)
}

func (e *Engine) workerSupports(ctx context.Context, required []domain.Capability, mode string) (bool, error) {
	workers, err := e.db.ListWorkers()
	if err != nil {
		return false, err
	}
	for _, worker := range workers {
		if worker.Status != domain.WorkerOnline {
			continue
		}
		if worker.LastHeartbeat.IsZero() || time.Since(worker.LastHeartbeat) > 15*time.Second {
			continue
		}
		if !domain.CapabilitiesSatisfied(worker.Capabilities, required) {
			continue
		}
		supportedMode := false
		for _, candidate := range worker.ExecModes {
			if candidate == mode {
				supportedMode = true
				break
			}
		}
		if supportedMode {
			return true, nil
		}
	}
	return false, nil
}

func capabilityStrings(caps []domain.Capability) []string {
	out := make([]string, len(caps))
	for i, capability := range caps {
		out[i] = string(capability)
	}
	return out
}

func (e *Engine) implementerTaskText(campaign domain.Campaign, task domain.CampaignTask, spec domain.ActionSpec, used, maxAttempts int) string {
	text := strings.TrimSpace(spec.Goal)
	if task.Cycle == 0 {
		if strings.TrimSpace(task.Error) == "" {
			return text
		}
		instruction := fmt.Sprintf(" Recovery attempt %d of at most %d. The previous attempt failed before producing an acceptable candidate: %s. Fix the concrete failure only inside the frozen scope and capabilities, run the relevant tests, and report executable evidence. Do not repeat the previous approach unchanged and do not edit protected acceptance files.",
			task.ExecutionCount+1, maxAttempts, strings.TrimSpace(task.Error))
		return appendBoundedInstruction(text, instruction)
	}
	prior := e.priorGate(campaign, task.Cycle-1)
	reasons := prior.Error
	if reasons == "" {
		reasons = prior.Outcome
	}
	reasons = strings.TrimSpace(reasons)
	if reasons == "" {
		reasons = "the previous candidate did not satisfy the frozen oracle"
	}
	instruction := fmt.Sprintf(" Repair cycle %d: fix the concrete failures reported by the previous Quality Gate (%s) without expanding the authorized scope or changing protected checks. Attempts used %d of at most %d.",
		task.Cycle, reasons, used, maxAttempts)
	if strings.Contains(strings.ToLower(reasons), "browser") || strings.Contains(strings.ToLower(reasons), "capability") {
		instruction += " For the browser capability, " + browserRepairPrerequisite(spec) +
			"run the wrapper with open plus a successful snapshot/find/fill/click before finishing; help-only invocation is not acceptance evidence."
	}
	return appendBoundedInstruction(text, instruction)
}

// browserRepairPrerequisite keeps repair-cycle guidance truthful for a
// Worker-managed test server. It never tells the Implementer to start an
// externally managed server.
func browserRepairPrerequisite(spec domain.ActionSpec) string {
	if spec.BrowserTestServer != nil {
		return "the authorized test server is already managed and running; do not start or restart it. "
	}
	return "start the authorized app and keep server logs outside the repository. "
}

func appendBoundedInstruction(text, instruction string) string {
	remaining := domain.MaxPaseoTaskChars - len(text)
	if remaining <= 0 {
		return text[:domain.MaxPaseoTaskChars]
	}
	if len(instruction) > remaining {
		instruction = instruction[:remaining]
	}
	return text + instruction
}

func cleanupUnconfirmed(run domain.Run) bool {
	for _, key := range []string{"cleanup", "browserCleanup"} {
		value, _ := run.Result[key].(string)
		if strings.HasPrefix(strings.ToLower(strings.TrimSpace(value)), "unconfirmed") {
			return true
		}
	}
	return false
}

func (e *Engine) priorGate(campaign domain.Campaign, cycle int) domain.CampaignTask {
	tasks, err := e.db.CampaignTasks(campaign.ID)
	if err != nil {
		return domain.CampaignTask{}
	}
	key := domain.CycleKey(domain.RoleQualityGate, cycle)
	for _, task := range tasks {
		if task.Key == key {
			return task
		}
	}
	return domain.CampaignTask{}
}

func (e *Engine) dispatchParticipant(ctx context.Context, campaign domain.Campaign, task domain.CampaignTask) {
	select {
	case e.sem <- struct{}{}:
	default:
		return
	}
	release := func() {
		select {
		case <-e.sem:
		default:
		}
	}
	ttl := time.Duration(taskTimeout(campaign, task)+120) * time.Second
	claimed, ok, events, err := e.db.ClaimParticipantTask(ctx, campaign.ID, task.ID,
		e.instID, ttl, "campaign-dispatch-"+randomID())
	if err != nil || !ok {
		release()
		if err != nil && !errors.Is(err, domain.ErrConflict) && !errors.Is(err, domain.ErrInvalidTransition) {
			e.log.Error("claim participant task failed", "taskId", task.ID, "error", err.Error())
		}
		return
	}
	e.publish(events)
	// Resolve the executable only after the durable claim: a missing tool is
	// then a real failed/blocked task, never a permanently ready task.
	bin, err := e.participantBinary(task.Role)
	if err != nil {
		release()
		e.failParticipant(ctx, campaign, claimed, err.Error())
		return
	}
	if err := e.spawnParticipant(ctx, campaign, claimed, bin, release); err != nil {
		release()
		e.failParticipant(ctx, campaign, claimed, err.Error())
	}
}

func (e *Engine) dispatchGate(ctx context.Context, campaign domain.Campaign, task domain.CampaignTask) {
	ttl := 2 * time.Minute
	claimed, ok, events, err := e.db.ClaimGateTask(ctx, campaign.ID, task.ID, e.instID,
		ttl, "campaign-gate-"+randomID())
	if err != nil || !ok {
		if err != nil && !errors.Is(err, domain.ErrConflict) && !errors.Is(err, domain.ErrInvalidTransition) {
			e.log.Error("claim gate task failed", "taskId", task.ID, "error", err.Error())
		}
		return
	}
	e.publish(events)
	e.reconcileGate(ctx, campaign, claimed, "campaign-gate-"+randomID())
}

func (e *Engine) spawnParticipant(ctx context.Context, campaign domain.Campaign, task domain.CampaignTask, bin string, release func()) error {
	timeout := time.Duration(taskTimeout(campaign, task)) * time.Second
	runCtx, cancel := context.WithTimeout(ctx, timeout)
	args := []string{
		"--core", e.cfg.CoreURL,
		"--candidate", task.CandidateID,
		"--instance-id", task.ParticipantID,
		"--idempotency-key", task.IdempotencyKey,
	}
	cmd := exec.CommandContext(runCtx, bin, args...)
	cmd.Env = e.participantEnv(task.Role)
	var stderr boundedBuffer
	cmd.Stdout = io.Discard
	cmd.Stderr = &stderr
	if err := cmd.Start(); err != nil {
		cancel()
		return fmt.Errorf("start %s participant: %w", task.Role, err)
	}
	proc := &runningProcess{campaignID: campaign.ID, task: task, cancel: cancel}
	e.mu.Lock()
	e.procs[task.ID] = proc
	e.mu.Unlock()
	e.log.Info("participant started", "campaignId", campaign.ID,
		"taskKey", task.Key, "participantId", task.ParticipantID, "pid", cmd.Process.Pid)
	e.wg.Add(1)
	go func() {
		defer e.wg.Done()
		waitErr := cmd.Wait()
		cancel()
		release()
		e.mu.Lock()
		delete(e.procs, task.ID)
		e.mu.Unlock()
		if e.root != nil && e.root.Err() != nil {
			return
		}
		e.finishParticipant(context.Background(), campaign, task, waitErr, stderr.String())
	}()
	return nil
}

func (e *Engine) finishParticipant(ctx context.Context, campaign domain.Campaign, task domain.CampaignTask, waitErr error, stderr string) {
	outcome, ok, err := e.db.TaskEvidence(task)
	if err != nil {
		e.log.Error("read participant evidence failed", "taskId", task.ID, "error", err.Error())
		return
	}
	if ok {
		_, _, events, cerr := e.db.CompleteCampaignTask(ctx, campaign.ID, task.ID, outcome,
			task.DispatchToken, "campaign-participant-"+randomID())
		if cerr != nil {
			e.log.Error("complete participant task failed", "taskId", task.ID, "error", cerr.Error())
			return
		}
		e.publish(events)
		return
	}
	reason := "participant exited without durable evidence"
	if waitErr != nil {
		reason = "participant failed: " + sanitizeOutput(stderr)
		if reason == "participant failed: " {
			reason = "participant failed: " + waitErr.Error()
		}
	}
	e.failParticipant(ctx, campaign, task, reason)
}

func (e *Engine) failParticipant(ctx context.Context, campaign domain.Campaign, task domain.CampaignTask, reason string) {
	_, events, err := e.db.FailParticipantTask(ctx, campaign.ID, task.ID,
		reason, task.DispatchToken, "campaign-participant-fail-"+randomID())
	if err != nil {
		e.log.Error("fail participant task failed", "taskId", task.ID, "error", err.Error())
		return
	}
	e.publish(events)
}

func (e *Engine) failImplementer(ctx context.Context, campaign domain.Campaign, task domain.CampaignTask, reason, correlation string) {
	_, events, err := e.db.FailImplementerTask(ctx, campaign.ID, task.ID, reason, task.DispatchToken, correlation)
	if err != nil {
		e.log.Error("fail implementer task failed", "taskId", task.ID, "error", err.Error())
		return
	}
	e.publish(events)
}

func (e *Engine) escalate(ctx context.Context, campaign domain.Campaign, reason, correlation string) {
	_, events, err := e.db.EscalateCampaign(ctx, campaign.ID, reason, correlation)
	if err != nil {
		e.log.Error("escalate campaign failed", "campaignId", campaign.ID, "error", err.Error())
		return
	}
	e.publish(events)
	e.killCampaign(campaign.ID)
}

func (e *Engine) block(ctx context.Context, campaign domain.Campaign, reason, correlation string) {
	_, events, err := e.db.BlockCampaign(ctx, campaign.ID, reason, correlation)
	if err != nil {
		e.log.Error("block campaign failed", "campaignId", campaign.ID, "error", err.Error())
		return
	}
	e.publish(events)
	e.killCampaign(campaign.ID)
}

func (e *Engine) publish(events []domain.Event) {
	for _, event := range events {
		e.hub.Publish(event)
	}
}

func (e *Engine) participantBinary(role domain.TaskRole) (string, error) {
	explicit := e.cfg.VerifierBin
	envName := "FACTORY_VERIFIER_BIN"
	name := "factory-verifier"
	if role == domain.RoleReviewer {
		explicit = e.cfg.ReviewerBin
		envName = "FACTORY_REVIEWER_BIN"
		name = "factory-reviewer"
	}
	if explicit == "" {
		explicit = strings.TrimSpace(os.Getenv(envName))
	}
	if explicit != "" {
		if info, err := os.Stat(explicit); err != nil || info.IsDir() {
			return "", fmt.Errorf("%s participant executable is unavailable at %s", role, explicit)
		}
		return explicit, nil
	}
	if exe, err := os.Executable(); err == nil {
		candidate := filepath.Join(filepath.Dir(exe), name)
		if info, serr := os.Stat(candidate); serr == nil && !info.IsDir() {
			return candidate, nil
		}
	}
	if path, err := exec.LookPath(name); err == nil {
		return path, nil
	}
	return "", fmt.Errorf("%s participant executable %q is not configured or installed", role, name)
}

func (e *Engine) participantEnv(role domain.TaskRole) []string {
	allowed := []string{
		"PATH", "HOME", "TMPDIR", "LC_ALL", "CHROMIUM_EXE",
		"FACTORY_PLAYWRIGHT_CORE", "PLAYWRIGHT_BROWSERS_PATH",
	}
	env := make([]string, 0, len(allowed)+1)
	for _, key := range allowed {
		if value := strings.TrimSpace(os.Getenv(key)); value != "" {
			env = append(env, key+"="+value)
		}
	}
	if role == domain.RoleVerifier {
		return append(env, "FACTORY_VERIFIER_TOKEN="+e.cfg.VerifierToken)
	}
	return append(env, "FACTORY_REVIEWER_TOKEN="+e.cfg.ReviewerToken)
}

func (e *Engine) hasProcess(taskID string) bool {
	e.mu.Lock()
	defer e.mu.Unlock()
	_, ok := e.procs[taskID]
	return ok
}

// pruneProcesses tears down participant processes whose campaign is no
// longer running (timeout, terminal decision observed by another instance,
// or cancellation) without touching jobs still owned by a live campaign.
func (e *Engine) pruneProcesses() {
	e.mu.Lock()
	defer e.mu.Unlock()
	for id, proc := range e.procs {
		campaign, err := e.db.GetCampaign(proc.campaignID)
		if err == nil && campaign.Status == domain.CampaignRunning {
			continue
		}
		proc.cancel()
		delete(e.procs, id)
	}
}

func (e *Engine) killCampaign(campaignID string) {
	e.mu.Lock()
	defer e.mu.Unlock()
	for id, proc := range e.procs {
		if proc.campaignID != campaignID {
			continue
		}
		proc.cancel()
		delete(e.procs, id)
	}
}

func (e *Engine) killAll() {
	e.mu.Lock()
	defer e.mu.Unlock()
	for id, proc := range e.procs {
		proc.cancel()
		delete(e.procs, id)
	}
}

func taskTimeout(campaign domain.Campaign, task domain.CampaignTask) int {
	if node, ok := campaign.Plan.TaskPlan(task.Key); ok {
		return node.TimeoutSecs
	}
	return 900
}

func randomID() string {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return fmt.Sprintf("%d", time.Now().UnixNano())
	}
	return hex.EncodeToString(b[:])
}

func sanitizeOutput(raw string) string {
	value := strings.TrimSpace(raw)
	value = strings.Join(strings.Fields(value), " ")
	if len(value) > 300 {
		value = value[:300]
	}
	return value
}

// boundedBuffer keeps participant stderr bounded so failure reasons stay
// small and never become an unbounded evidence sink.
type boundedBuffer struct {
	buf bytes.Buffer
}

func (b *boundedBuffer) Write(p []byte) (int, error) {
	if b.buf.Len() >= 4096 {
		return len(p), nil
	}
	remaining := 4096 - b.buf.Len()
	if len(p) > remaining {
		_, _ = b.buf.Write(p[:remaining])
		return len(p), nil
	}
	return b.buf.Write(p)
}

func (b *boundedBuffer) String() string {
	return b.buf.String()
}
