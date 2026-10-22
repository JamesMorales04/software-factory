package domain

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"
)

// S4-B campaign orchestration domain. A Campaign is the Core-owned,
// durable binding between one authorized Action/Spec and an immutable plan
// of role tasks. The plan is data, not a second workflow engine: individual
// Implementer work still executes as an S2/S3 Run with its existing leases,
// fencing and recovery semantics.
type CampaignStatus string

const (
	CampaignPending       CampaignStatus = "pending"
	CampaignRunning       CampaignStatus = "running"
	CampaignAccepted      CampaignStatus = "accepted"
	CampaignRepairNeeded  CampaignStatus = "repair_required"
	CampaignBlocked       CampaignStatus = "blocked"
	CampaignHumanRequired CampaignStatus = "human_required"
	CampaignFailed        CampaignStatus = "failed"
	CampaignCancelled     CampaignStatus = "cancelled"
)

// TaskRole is the closed participant vocabulary for the first engineering
// campaign. QualityGate is Core-internal; the others are independent
// execution identities.
type TaskRole string

const (
	RoleImplementer TaskRole = "implementer"
	RoleVerifier    TaskRole = "verifier"
	RoleReviewer    TaskRole = "reviewer"
	RoleQualityGate TaskRole = "quality_gate"
)

// CampaignTaskStatus is the durable task lifecycle. A failed task may be
// retried by returning to ready while execution budget remains.
type CampaignTaskStatus string

const (
	TaskPending    CampaignTaskStatus = "pending"
	TaskReady      CampaignTaskStatus = "ready"
	TaskDispatched CampaignTaskStatus = "dispatched"
	TaskRunning    CampaignTaskStatus = "running"
	TaskSucceeded  CampaignTaskStatus = "succeeded"
	TaskFailed     CampaignTaskStatus = "failed"
	TaskBlocked    CampaignTaskStatus = "blocked"
	TaskCancelled  CampaignTaskStatus = "cancelled"
)

// Campaign bounds. One campaign is intentionally one Action and a bounded
// repair horizon; speculative general-purpose workflow graphs are out of
// scope for F1-S4-B.
const (
	MaxCampaignRepairCycles    = 3
	DefaultCampaignRepair      = 1
	MinCampaignWallClockSecs   = 120
	MaxCampaignWallClockSecs   = 7200
	DefaultCampaignWallClock   = 1800
	MaxCampaignParallel        = 2
	MaxCampaignTasks           = 32
	MaxCampaignTaskExecutions  = 3
	MaxCampaignImplementerRuns = 24
	DefaultCampaignAttempts    = 1
)

// FailureClass is the deterministic classification of an Implementer
// failure before acceptance. It decides whether the Campaign may recover
// automatically, must repair through the Quality Gate, or must stop.
type FailureClass string

const (
	RecoverableImplementationFailure FailureClass = "RECOVERABLE_IMPLEMENTATION_FAILURE"
	RecoverableExecutionFailure      FailureClass = "RECOVERABLE_EXECUTION_FAILURE"
	QualityRepairRequired            FailureClass = "QUALITY_REPAIR_REQUIRED"
	OracleDefectSuspected            FailureClass = "ORACLE_DEFECT_SUSPECTED"
	PolicyOrSecurityBlocked          FailureClass = "POLICY_OR_SECURITY_BLOCKED"
	UnresolvedExternalRecovery       FailureClass = "UNRESOLVED_EXTERNAL_RECOVERY"
	InfrastructureBlocked            FailureClass = "INFRASTRUCTURE_BLOCKED"
)

// Recoverable is the closed set that may authorize a bounded pre-candidate
// retry. Security, oracle, unresolved external and infrastructure failures
// are never recoverable.
func (c FailureClass) Recoverable() bool {
	return c == RecoverableImplementationFailure || c == RecoverableExecutionFailure
}

// CampaignRecovery is the immutable audit row for one authorized
// pre-candidate recovery. The failed Run/Attempt/session is never mutated;
// the recovery authorizes the next Run against the same frozen spec/task.
type CampaignRecovery struct {
	ID              string       `json:"id"`
	CampaignID      string       `json:"campaignId"`
	TaskID          string       `json:"taskId"`
	AttemptIndex    int          `json:"attemptIndex"`
	FailureClass    FailureClass `json:"failureClass"`
	Reason          string       `json:"reason"`
	PreviousRunID   string       `json:"previousRunId"`
	PreviousAttempt string       `json:"previousAttemptId,omitempty"`
	NewRunID        string       `json:"newRunId,omitempty"`
	CreatedAt       time.Time    `json:"createdAt"`
}

// CampaignTaskPlan is one immutable plan node. Cycle zero is the initial
// implementation; later cycles are authorized only by a REPAIR_REQUIRED
// Quality Gate decision for the immediately preceding cycle.
type CampaignTaskPlan struct {
	Key           string   `json:"key"`
	Role          TaskRole `json:"role"`
	Cycle         int      `json:"cycle"`
	DependsOn     []string `json:"dependsOn"`
	TimeoutSecs   int      `json:"timeoutSecs"`
	MaxExecutions int      `json:"maxExecutions"`
	// S4-D WorkGraph metadata. Legacy S4-B plans leave these empty.
	UnitID      string `json:"unitId,omitempty"`
	UnitKind    string `json:"unitKind,omitempty"`
	SpecID      string `json:"specId,omitempty"`
	SourceKey   string `json:"sourceKey,omitempty"`
	Instruction string `json:"instruction,omitempty"`
}

const (
	LeadUnitKindImplementation = "implementation"
	LeadUnitKindIntegration    = "integration"
	LeadUnitKindRepair         = "repair"
	IntegrationTaskKey         = "integration-0"
)

// LeadUnitTaskKey returns the stable plan key for one WorkUnit.
func LeadUnitTaskKey(unitID string) string { return "unit-" + unitID + "-0" }

// CampaignPlan is the immutable execution graph revision.
type CampaignPlan struct {
	Revision         int                `json:"revision"`
	MaxRepairCycles  int                `json:"maxRepairCycles"`
	Tasks            []CampaignTaskPlan `json:"tasks"`
	Parallelism      int                `json:"parallelism"`
	WallClockSeconds int                `json:"wallClockSeconds"`
}

// Campaign is the durable orchestration record. The plan hash identifies
// the exact immutable graph; the task rows identify execution state.
type Campaign struct {
	ID               string         `json:"id"`
	ProjectID        string         `json:"projectId"`
	ActionID         string         `json:"actionId"`
	SpecID           string         `json:"specId"`
	SpecHash         string         `json:"specHash"`
	PolicyRevision   string         `json:"policyRevision"`
	PlanHash         string         `json:"planHash"`
	Plan             CampaignPlan   `json:"plan"`
	Status           CampaignStatus `json:"status"`
	TerminalReason   string         `json:"terminalReason,omitempty"`
	MaxRepairCycles  int            `json:"maxRepairCycles"`
	RepairCycles     int            `json:"repairCycles"`
	MaxAttempts      int            `json:"maxAttempts"`
	MaxWallClockSecs int            `json:"maxWallClockSecs"`
	MaxParallel      int            `json:"maxParallel"`
	CorrelationID    string         `json:"correlationId,omitempty"`
	IdempotencyKey   string         `json:"idempotencyKey,omitempty"`
	CreatedAt        time.Time      `json:"createdAt"`
	UpdatedAt        time.Time      `json:"updatedAt"`
	StartedAt        time.Time      `json:"startedAt,omitempty"`
	CompletedAt      time.Time      `json:"completedAt,omitempty"`
}

// CampaignTask is one durable task row. Implementer tasks bind an S2 Run;
// verifier/reviewer tasks bind a candidate and a participant execution;
// gate tasks are evaluated by Core itself.
type CampaignTask struct {
	ID                string             `json:"id"`
	CampaignID        string             `json:"campaignId"`
	Key               string             `json:"key"`
	Role              TaskRole           `json:"role"`
	Cycle             int                `json:"cycle"`
	Status            CampaignTaskStatus `json:"status"`
	DependsOn         []string           `json:"dependsOn"`
	ParticipantID     string             `json:"participantId,omitempty"`
	CandidateID       string             `json:"candidateId,omitempty"`
	RunID             string             `json:"runId,omitempty"`
	Outcome           string             `json:"outcome,omitempty"`
	Error             string             `json:"error,omitempty"`
	ExecutionCount    int                `json:"executionCount"`
	MaxExecutions     int                `json:"maxExecutions"`
	IdempotencyKey    string             `json:"idempotencyKey,omitempty"`
	DispatchOwner     string             `json:"dispatchOwner,omitempty"`
	DispatchToken     string             `json:"-"`
	DispatchExpiresAt time.Time          `json:"dispatchExpiresAt,omitempty"`
	CreatedAt         time.Time          `json:"createdAt"`
	UpdatedAt         time.Time          `json:"updatedAt"`
	StartedAt         time.Time          `json:"startedAt,omitempty"`
	CompletedAt       time.Time          `json:"completedAt,omitempty"`
}

// CampaignBudgetUsage is the observable resource projection. Provider
// tokens, quota and cost are not fabricated: ProviderBudget is "unknown"
// unless a future authoritative meter records them.
type CampaignBudgetUsage struct {
	MaxAttempts          int    `json:"maxAttempts"`
	AttemptsUsed         int    `json:"attemptsUsed"`
	MaxRepairCycles      int    `json:"maxRepairCycles"`
	RepairCycles         int    `json:"repairCycles"`
	MaxWallClockSecs     int    `json:"maxWallClockSecs"`
	ElapsedSecs          int64  `json:"elapsedSecs"`
	RemainingWallClock   int64  `json:"remainingWallClockSecs"`
	MaxParallel          int    `json:"maxParallel"`
	ActiveParticipantMax int    `json:"activeParticipantMax"`
	RecoveryAttempts     int    `json:"recoveryAttempts"`
	ProviderBudget       string `json:"providerBudget"`
}

// CampaignProjection is the authoritative read model consumed by Office.
type CampaignProjection struct {
	Campaign   Campaign            `json:"campaign"`
	Tasks      []CampaignTask      `json:"tasks"`
	Budget     CampaignBudgetUsage `json:"budget"`
	Candidate  *CandidateRevision  `json:"candidate,omitempty"`
	Decision   *GateDecision       `json:"decision,omitempty"`
	Recoveries []CampaignRecovery  `json:"recoveries"`
}

// NewCampaignParams is operator input. The Action/project/spec identity is
// derived from the frozen spec, never from a second untrusted claim.
type NewCampaignParams struct {
	SpecID                 string
	MaxRepairCycles        int
	MaxWallClockSecs       int
	MaxParallel            int
	MaxImplementerAttempts int
	IdempotencyKey         string
	// Plan is set by Core-owned Lead planning. When nil the legacy S4-B
	// four-task plan is built deterministically from the budgets.
	Plan *CampaignPlan
}

// Validate normalizes campaign creation input and builds the frozen plan.
func (p NewCampaignParams) Validate() (NewCampaignParams, error) {
	p.SpecID = strings.TrimSpace(p.SpecID)
	if len(p.SpecID) != 32 || !isLowerHex(p.SpecID) {
		return p, errors.New("specId must be a 32-character lowercase hex id")
	}
	if p.MaxRepairCycles < 0 || p.MaxRepairCycles > MaxCampaignRepairCycles {
		return p, fmt.Errorf("maxRepairCycles must be 0..%d", MaxCampaignRepairCycles)
	}
	if p.MaxWallClockSecs == 0 {
		p.MaxWallClockSecs = DefaultCampaignWallClock
	}
	if p.MaxWallClockSecs < MinCampaignWallClockSecs || p.MaxWallClockSecs > MaxCampaignWallClockSecs {
		return p, fmt.Errorf("maxWallClockSecs must be %d..%d",
			MinCampaignWallClockSecs, MaxCampaignWallClockSecs)
	}
	if p.MaxParallel == 0 {
		p.MaxParallel = MaxCampaignParallel
	}
	if p.MaxParallel < 1 || p.MaxParallel > MaxCampaignParallel {
		return p, fmt.Errorf("maxParallel must be 1..%d", MaxCampaignParallel)
	}
	if p.MaxImplementerAttempts < 0 || p.MaxImplementerAttempts > MaxCampaignImplementerRuns {
		return p, fmt.Errorf("maxImplementerAttempts must be 0..%d", MaxCampaignImplementerRuns)
	}
	p.IdempotencyKey = strings.TrimSpace(p.IdempotencyKey)
	if len(p.IdempotencyKey) > MaxIdempotencyKeyChars {
		return p, fmt.Errorf("idempotency key exceeds %d characters", MaxIdempotencyKeyChars)
	}
	if p.Plan != nil {
		if err := p.Plan.Validate(); err != nil {
			return p, fmt.Errorf("campaign plan: %w", err)
		}
		if p.Plan.MaxRepairCycles != p.MaxRepairCycles ||
			p.Plan.WallClockSeconds != p.MaxWallClockSecs ||
			p.Plan.Parallelism != p.MaxParallel {
			return p, errors.New("campaign plan budgets do not match campaign parameters")
		}
	}
	return p, nil
}

// BuildCampaignPlan constructs the deterministic first-engineering-workflow
// graph. Every cycle has implement -> (verify || review) -> gate. Repair
// cycles are dormant until the previous gate authorizes them.
func BuildCampaignPlan(maxRepairCycles, maxWallClockSecs, maxParallel int) CampaignPlan {
	cycles := maxRepairCycles + 1
	tasks := make([]CampaignTaskPlan, 0, cycles*4)
	for cycle := 0; cycle < cycles; cycle++ {
		impl := fmt.Sprintf("implementer-%d", cycle)
		verify := fmt.Sprintf("verifier-%d", cycle)
		review := fmt.Sprintf("reviewer-%d", cycle)
		gate := fmt.Sprintf("gate-%d", cycle)
		implDeps := []string{}
		if cycle > 0 {
			implDeps = []string{fmt.Sprintf("gate-%d", cycle-1)}
		}
		tasks = append(tasks,
			CampaignTaskPlan{
				Key: impl, Role: RoleImplementer, Cycle: cycle, DependsOn: implDeps,
				TimeoutSecs: 900, MaxExecutions: 1,
			},
			CampaignTaskPlan{
				Key: verify, Role: RoleVerifier, Cycle: cycle, DependsOn: []string{impl},
				TimeoutSecs: 900, MaxExecutions: 2,
			},
			CampaignTaskPlan{
				Key: review, Role: RoleReviewer, Cycle: cycle, DependsOn: []string{impl},
				TimeoutSecs: 600, MaxExecutions: 2,
			},
			CampaignTaskPlan{
				Key: gate, Role: RoleQualityGate, Cycle: cycle,
				DependsOn: []string{verify, review}, TimeoutSecs: 120, MaxExecutions: 1,
			},
		)
	}
	return CampaignPlan{
		Revision:         1,
		MaxRepairCycles:  maxRepairCycles,
		Tasks:            tasks,
		Parallelism:      maxParallel,
		WallClockSeconds: maxWallClockSecs,
	}
}

// BuildLeadCampaignPlan constructs a bounded WorkGraph: implementation units
// with explicit dependencies, one Core-owned integration node for multi-unit
// graphs, then the existing verifier/reviewer/gate chain and repair horizon.
func BuildLeadCampaignPlan(p LeadPlanProposal, unitSpecIDs map[string]string, campaignSpecID string) (CampaignPlan, error) {
	if len(p.WorkUnits) == 0 || len(p.WorkUnits) > MaxLeadWorkUnits {
		return CampaignPlan{}, errors.New("work graph has no executable units")
	}
	tasks := make([]CampaignTaskPlan, 0, len(p.WorkUnits)+4+p.MaxRepairCycles*4)
	dependedOn := map[string]bool{}
	for _, unit := range p.WorkUnits {
		specID := unitSpecIDs[unit.ID]
		if len(specID) != 32 || !isLowerHex(specID) {
			return CampaignPlan{}, fmt.Errorf("work unit %q has no frozen spec", unit.ID)
		}
		deps := make([]string, 0, len(unit.DependsOn))
		for _, dep := range unit.DependsOn {
			deps = append(deps, LeadUnitTaskKey(dep))
			dependedOn[dep] = true
		}
		tasks = append(tasks, CampaignTaskPlan{
			Key: LeadUnitTaskKey(unit.ID), Role: RoleImplementer, Cycle: 0,
			DependsOn: deps, TimeoutSecs: 900, MaxExecutions: 1,
			UnitID: unit.ID, UnitKind: LeadUnitKindImplementation, SpecID: specID,
			Instruction: unit.Description,
		})
	}
	sourceKey := LeadUnitTaskKey(p.WorkUnits[0].ID)
	if len(p.WorkUnits) > 1 {
		if len(campaignSpecID) != 32 || !isLowerHex(campaignSpecID) {
			return CampaignPlan{}, errors.New("multi-unit graph requires a frozen integration spec")
		}
		var terminals []string
		for _, unit := range p.WorkUnits {
			if !dependedOn[unit.ID] {
				terminals = append(terminals, LeadUnitTaskKey(unit.ID))
			}
		}
		tasks = append(tasks, CampaignTaskPlan{
			Key: IntegrationTaskKey, Role: RoleImplementer, Cycle: 0,
			DependsOn: terminals, TimeoutSecs: 900, MaxExecutions: 1,
			UnitID: "integration", UnitKind: LeadUnitKindIntegration, SpecID: campaignSpecID,
			Instruction: "Integrate the dependency candidates into one candidate and keep the frozen acceptance oracle green.",
		})
		sourceKey = IntegrationTaskKey
	} else if campaignSpecID != unitSpecIDs[p.WorkUnits[0].ID] {
		return CampaignPlan{}, errors.New("single-unit graph campaign spec must be the unit spec")
	}
	cycles := p.MaxRepairCycles + 1
	for cycle := 0; cycle < cycles; cycle++ {
		if cycle > 0 {
			tasks = append(tasks, CampaignTaskPlan{
				Key: CycleKey(RoleImplementer, cycle), Role: RoleImplementer, Cycle: cycle,
				DependsOn:   []string{CycleKey(RoleQualityGate, cycle-1)},
				TimeoutSecs: 900, MaxExecutions: 1, UnitID: "repair", UnitKind: LeadUnitKindRepair,
				SpecID: campaignSpecID,
			})
			sourceKey = CycleKey(RoleImplementer, cycle)
		}
		verify := CycleKey(RoleVerifier, cycle)
		review := CycleKey(RoleReviewer, cycle)
		gate := CycleKey(RoleQualityGate, cycle)
		tasks = append(tasks,
			CampaignTaskPlan{
				Key: verify, Role: RoleVerifier, Cycle: cycle,
				DependsOn: []string{sourceKey}, TimeoutSecs: 900, MaxExecutions: 2,
				SourceKey: sourceKey,
			},
			CampaignTaskPlan{
				Key: review, Role: RoleReviewer, Cycle: cycle,
				DependsOn: []string{sourceKey}, TimeoutSecs: 600, MaxExecutions: 2,
				SourceKey: sourceKey,
			},
			CampaignTaskPlan{
				Key: gate, Role: RoleQualityGate, Cycle: cycle,
				DependsOn: []string{verify, review}, TimeoutSecs: 120, MaxExecutions: 1,
				SourceKey: sourceKey,
			},
		)
	}
	plan := CampaignPlan{
		Revision: 1, MaxRepairCycles: p.MaxRepairCycles, Tasks: tasks,
		Parallelism: p.MaxParallel, WallClockSeconds: p.MaxWallClockSecs,
	}
	if len(plan.Tasks) > MaxCampaignTasks {
		return CampaignPlan{}, fmt.Errorf("work graph exceeds %d campaign tasks", MaxCampaignTasks)
	}
	if err := plan.Validate(); err != nil {
		return CampaignPlan{}, err
	}
	return plan, nil
}

// Validate checks graph identity and role/dependency invariants.
func (p CampaignPlan) Validate() error {
	if p.Revision != 1 {
		return errors.New("campaign plan revision must be 1")
	}
	if p.MaxRepairCycles < 0 || p.MaxRepairCycles > MaxCampaignRepairCycles {
		return errors.New("campaign plan repair cycles are out of range")
	}
	if p.Parallelism < 1 || p.Parallelism > MaxCampaignParallel {
		return errors.New("campaign plan parallelism is out of range")
	}
	if p.WallClockSeconds < MinCampaignWallClockSecs || p.WallClockSeconds > MaxCampaignWallClockSecs {
		return errors.New("campaign plan wall clock is out of range")
	}
	if len(p.Tasks) == 0 || len(p.Tasks) > MaxCampaignTasks {
		return errors.New("campaign plan task count is out of range")
	}
	seen := map[string]bool{}
	byKey := make(map[string]CampaignTaskPlan, len(p.Tasks))
	cycles := map[string]int{}
	for _, task := range p.Tasks {
		if len(task.Key) == 0 || len(task.Key) > 64 || !isSlug(task.Key) {
			return fmt.Errorf("campaign task key %q is invalid", task.Key)
		}
		if seen[task.Key] {
			return fmt.Errorf("duplicate campaign task key %q", task.Key)
		}
		seen[task.Key] = true
		byKey[task.Key] = task
		cycles[task.Key] = task.Cycle
		switch task.Role {
		case RoleImplementer, RoleVerifier, RoleReviewer, RoleQualityGate:
		default:
			return fmt.Errorf("campaign task %q has unknown role %q", task.Key, task.Role)
		}
		if task.Cycle < 0 || task.Cycle > p.MaxRepairCycles {
			return fmt.Errorf("campaign task %q cycle is out of range", task.Key)
		}
		if task.TimeoutSecs < 30 || task.TimeoutSecs > MaxCampaignWallClockSecs {
			return fmt.Errorf("campaign task %q timeout is out of range", task.Key)
		}
		if task.MaxExecutions < 1 || task.MaxExecutions > MaxCampaignTaskExecutions {
			return fmt.Errorf("campaign task %q execution budget is out of range", task.Key)
		}
		if len(task.DependsOn) > MaxCampaignTasks {
			return fmt.Errorf("campaign task %q has too many dependencies", task.Key)
		}
		if len(task.Instruction) > MaxLeadUnitDescriptionChars {
			return fmt.Errorf("campaign task %q instruction exceeds its bound", task.Key)
		}
		if task.SpecID != "" && (len(task.SpecID) != 32 || !isLowerHex(task.SpecID)) {
			return fmt.Errorf("campaign task %q spec id is invalid", task.Key)
		}
		if task.UnitID != "" {
			if task.Role != RoleImplementer || len(task.UnitID) > 64 || !isSlug(task.UnitID) {
				return fmt.Errorf("campaign task %q has invalid unit metadata", task.Key)
			}
			switch task.UnitKind {
			case LeadUnitKindImplementation, LeadUnitKindIntegration:
				if task.Cycle != 0 {
					return fmt.Errorf("campaign task %q unit kind must be in cycle zero", task.Key)
				}
			case LeadUnitKindRepair:
				if task.Cycle < 1 {
					return fmt.Errorf("campaign task %q repair unit must be in a repair cycle", task.Key)
				}
			default:
				return fmt.Errorf("campaign task %q has unknown unit kind %q", task.Key, task.UnitKind)
			}
		} else if task.UnitKind != "" {
			return fmt.Errorf("campaign task %q has a unit kind without a unit id", task.Key)
		}
		if task.SourceKey != "" && task.Role != RoleVerifier && task.Role != RoleReviewer && task.Role != RoleQualityGate {
			return fmt.Errorf("campaign task %q has a candidate source with a non-evidence role", task.Key)
		}
		for _, dep := range task.DependsOn {
			if dep == task.Key {
				return fmt.Errorf("campaign task %q depends on itself", task.Key)
			}
		}
	}
	for _, task := range p.Tasks {
		for _, dep := range task.DependsOn {
			if !seen[dep] {
				return fmt.Errorf("campaign task %q depends on unknown task %q", task.Key, dep)
			}
			if cycles[dep] > task.Cycle {
				return fmt.Errorf("campaign task %q depends on a later cycle", task.Key)
			}
		}
		if task.SourceKey != "" {
			source, ok := byKey[task.SourceKey]
			if !ok || source.Role != RoleImplementer || source.Cycle != task.Cycle {
				return fmt.Errorf("campaign task %q candidate source %q is invalid", task.Key, task.SourceKey)
			}
		}
	}
	for cycle := 0; cycle <= p.MaxRepairCycles; cycle++ {
		gateKey := CycleKey(RoleQualityGate, cycle)
		verifyKey := CycleKey(RoleVerifier, cycle)
		reviewKey := CycleKey(RoleReviewer, cycle)
		gate, hasGate := byKey[gateKey]
		if !hasGate {
			return fmt.Errorf("campaign plan is missing gate task %q", gateKey)
		}
		if _, ok := byKey[verifyKey]; !ok {
			return fmt.Errorf("campaign plan is missing verifier task %q", verifyKey)
		}
		if _, ok := byKey[reviewKey]; !ok {
			return fmt.Errorf("campaign plan is missing reviewer task %q", reviewKey)
		}
		if !containsString(gate.DependsOn, verifyKey) || !containsString(gate.DependsOn, reviewKey) {
			return fmt.Errorf("campaign gate %q must depend on its verifier and reviewer", gateKey)
		}
		if cycle > 0 {
			if task, ok := byKey[CycleKey(RoleImplementer, cycle)]; !ok || task.Role != RoleImplementer {
				return fmt.Errorf("campaign plan is missing repair implementer %q", CycleKey(RoleImplementer, cycle))
			}
		}
	}
	cycleZeroImplementers := 0
	for _, task := range p.Tasks {
		if task.Role == RoleImplementer && task.Cycle == 0 {
			cycleZeroImplementers++
		}
	}
	if cycleZeroImplementers == 0 {
		return errors.New("campaign plan has no cycle-zero implementer")
	}
	if err := p.validateDAG(); err != nil {
		return err
	}
	return nil
}

func containsString(values []string, candidate string) bool {
	for _, value := range values {
		if value == candidate {
			return true
		}
	}
	return false
}

func (p CampaignPlan) validateDAG() error {
	state := make(map[string]int, len(p.Tasks))
	byKey := make(map[string]CampaignTaskPlan, len(p.Tasks))
	for _, task := range p.Tasks {
		byKey[task.Key] = task
	}
	var visit func(string) error
	visit = func(key string) error {
		switch state[key] {
		case 1:
			return fmt.Errorf("campaign plan dependency cycle at %q", key)
		case 2:
			return nil
		}
		state[key] = 1
		for _, dep := range byKey[key].DependsOn {
			if err := visit(dep); err != nil {
				return err
			}
		}
		state[key] = 2
		return nil
	}
	for key := range byKey {
		if err := visit(key); err != nil {
			return err
		}
	}
	return nil
}

// Hash returns the immutable plan identity.
func (p CampaignPlan) Hash() string {
	raw, err := json.Marshal(p)
	if err != nil {
		return ""
	}
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}

// TaskPlan returns a plan node by key.
func (p CampaignPlan) TaskPlan(key string) (CampaignTaskPlan, bool) {
	for _, task := range p.Tasks {
		if task.Key == key {
			return task, true
		}
	}
	return CampaignTaskPlan{}, false
}

// CycleForRole returns the plan key for one role in a cycle.
func CycleKey(role TaskRole, cycle int) string {
	switch role {
	case RoleImplementer:
		return fmt.Sprintf("implementer-%d", cycle)
	case RoleVerifier:
		return fmt.Sprintf("verifier-%d", cycle)
	case RoleReviewer:
		return fmt.Sprintf("reviewer-%d", cycle)
	case RoleQualityGate:
		return fmt.Sprintf("gate-%d", cycle)
	default:
		return ""
	}
}

// Terminal reports whether a campaign can no longer advance.
func (s CampaignStatus) Terminal() bool {
	switch s {
	case CampaignAccepted, CampaignRepairNeeded, CampaignBlocked,
		CampaignHumanRequired, CampaignFailed, CampaignCancelled:
		return true
	default:
		return false
	}
}

// Terminal reports whether a task can no longer advance.
func (s CampaignTaskStatus) Terminal() bool {
	switch s {
	case TaskSucceeded, TaskFailed, TaskBlocked, TaskCancelled:
		return true
	default:
		return false
	}
}

// CanTransition reports the legal task edges. Failed -> ready is permitted
// only through the store's bounded retry path.
func (s CampaignTaskStatus) CanTransition(to CampaignTaskStatus) bool {
	switch s {
	case TaskPending:
		return to == TaskReady || to == TaskCancelled || to == TaskBlocked
	case TaskReady:
		return to == TaskDispatched || to == TaskCancelled || to == TaskBlocked
	case TaskDispatched:
		return to == TaskRunning || to == TaskSucceeded || to == TaskFailed ||
			to == TaskReady || to == TaskCancelled || to == TaskBlocked
	case TaskRunning:
		return to == TaskSucceeded || to == TaskFailed || to == TaskCancelled || to == TaskBlocked
	case TaskFailed:
		return to == TaskReady
	default:
		return false
	}
}

// ParticipantIdentity is the deterministic scoped identity for a
// verifier/reviewer process in this campaign.
func ParticipantIdentity(campaignID string, role TaskRole, cycle int) string {
	short := campaignID
	if len(short) > 8 {
		short = short[:8]
	}
	return fmt.Sprintf("%s-%s-%d", role, short, cycle)
}

// Validate performs a defensive check of a loaded campaign row.
func (c Campaign) Validate() error {
	if len(c.ID) != 32 || !isLowerHex(c.ID) {
		return errors.New("campaign id must be a 32-character lowercase hex id")
	}
	if err := ValidateProjectID(c.ProjectID); err != nil {
		return err
	}
	if len(c.ActionID) != 32 || !isLowerHex(c.ActionID) {
		return errors.New("campaign actionId must be a 32-character lowercase hex id")
	}
	if len(c.SpecID) != 32 || !isLowerHex(c.SpecID) {
		return errors.New("campaign specId must be a 32-character lowercase hex id")
	}
	if !isLowerHex(c.SpecHash) || len(c.SpecHash) != 64 {
		return errors.New("campaign specHash must be a sha256 hex digest")
	}
	if c.PolicyRevision != QualityPolicyRevision {
		return errors.New("campaign policy revision is not active")
	}
	if err := c.Plan.Validate(); err != nil {
		return err
	}
	if c.PlanHash != c.Plan.Hash() {
		return errors.New("campaign plan hash does not match the immutable plan")
	}
	if c.RepairCycles < 0 || c.RepairCycles > c.MaxRepairCycles {
		return errors.New("campaign repair cycle count is out of range")
	}
	if c.MaxAttempts < 1 || c.MaxAttempts > MaxCampaignImplementerRuns {
		return errors.New("campaign implementer attempt budget is out of range")
	}
	if c.MaxRepairCycles != c.Plan.MaxRepairCycles || c.MaxParallel != c.Plan.Parallelism ||
		c.MaxWallClockSecs != c.Plan.WallClockSeconds {
		return errors.New("campaign budgets do not match the immutable plan")
	}
	switch c.Status {
	case CampaignPending, CampaignRunning, CampaignAccepted, CampaignRepairNeeded,
		CampaignBlocked, CampaignHumanRequired, CampaignFailed, CampaignCancelled:
	default:
		return fmt.Errorf("unknown campaign status %q", c.Status)
	}
	return nil
}
