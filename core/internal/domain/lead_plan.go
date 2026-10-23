package domain

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"sort"
	"strings"
	"time"
)

// LeadObjectiveClass is the closed set of objective classes allowed in the
// first bounded planning slice.
type LeadObjectiveClass string

const (
	LeadFeature     LeadObjectiveClass = "feature"
	LeadMaintenance LeadObjectiveClass = "maintenance"
	LeadRefactor    LeadObjectiveClass = "refactor"
)

// LeadWorkUnitKind is intentionally closed. S4-D executes implementation
// units through the existing ActionSpec/Campaign path and lets Core add one
// integration node when a graph has more than one unit.
type LeadWorkUnitKind string

const LeadImplementationUnit LeadWorkUnitKind = "implementation"

const (
	MaxLeadWorkUnits            = 8
	MaxLeadWorkUnitDependencies = 8
	MaxLeadWorkUnitIDChars      = 32
	MaxLeadUnitDescriptionChars = 2000
	MaxLeadGraphTasks           = 32
	MaxDelegationRequests       = 16
	MaxCumulativeLeadSessions   = 256
	MaxCumulativeLeadAttempts   = 512
)

const MaxLeadSessions = MaxLeadGraphTasks * MaxCampaignTaskExecutions

// StandingDelegation is an immutable, owner-issued bound on Lead planning.
// Core persistence owns issuance and revocation; this type only validates
// the snapshot supplied for a planning decision.
type StandingDelegation struct {
	ID               string               `json:"id"`
	PrincipalID      string               `json:"principalId"`
	AuditID          string               `json:"auditId"`
	ProjectID        string               `json:"projectId"`
	Revision         int                  `json:"revision"`
	ContentHash      string               `json:"contentHash"`
	ObjectiveClasses []LeadObjectiveClass `json:"objectiveClasses"`
	AllowedPaths     []string             `json:"allowedPaths"`
	Capabilities     []Capability         `json:"capabilities"`
	RiskClasses      []RiskClass          `json:"riskClasses"`
	Provider         string               `json:"provider"`
	Model            string               `json:"model"`
	PaseoMode        string               `json:"paseoMode"`
	MaxAttempts      int                  `json:"maxAttempts"`
	MaxTimeoutSecs   int                  `json:"maxTimeoutSecs"`
	MaxRepairCycles  int                  `json:"maxRepairCycles"`
	MaxWallClockSecs int                  `json:"maxWallClockSecs"`
	MaxParallel      int                  `json:"maxParallel"`
	MaxSessions      int                  `json:"maxSessions"`
	// Reusable cumulative authority. Per-Campaign ceilings remain above;
	// these ceilings bound all requests and Campaigns issued under the grant.
	MaxRequests           int  `json:"maxRequests"`
	MaxCumulativeSessions int  `json:"maxCumulativeSessions"`
	MaxCumulativeAttempts int  `json:"maxCumulativeAttempts"`
	AutonomousStart       bool `json:"autonomousStart"`
	// AutonomousRiskClasses is a subset of RiskClasses. R2/R3 can never be
	// autonomous under this revision.
	AutonomousRiskClasses []RiskClass `json:"autonomousRiskClasses"`
	ExpiresAt             time.Time   `json:"expiresAt"`
	RevokedAt             time.Time   `json:"revokedAt,omitempty"`
}

// ComputeContentHash returns the immutable grant identity. The persisted
// ContentHash field is excluded from its own digest.
func (g StandingDelegation) ComputeContentHash() string {
	g.ContentHash = ""
	g.RevokedAt = time.Time{}
	raw, err := json.Marshal(g)
	if err != nil {
		return ""
	}
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}

// Validate checks an issued grant snapshot without applying defaults.
func (g StandingDelegation) Validate(now time.Time) error {
	if strings.TrimSpace(g.ID) == "" || strings.TrimSpace(g.PrincipalID) == "" || strings.TrimSpace(g.AuditID) == "" {
		return errors.New("delegation id, principalId, and auditId are required")
	}
	if err := ValidateProjectID(g.ProjectID); err != nil {
		return fmt.Errorf("delegation projectId: %w", err)
	}
	if g.Revision < 1 || !isLowerHex(g.ContentHash) || len(g.ContentHash) != 64 || g.ContentHash != g.ComputeContentHash() {
		return errors.New("delegation revision and contentHash must identify an immutable grant")
	}
	if now.IsZero() {
		return errors.New("validation time is required")
	}
	if !g.RevokedAt.IsZero() {
		return errors.New("delegation is revoked")
	}
	if g.ExpiresAt.IsZero() || !now.Before(g.ExpiresAt) {
		return errors.New("delegation is expired or has no expiry")
	}
	if len(g.ObjectiveClasses) == 0 || len(g.AllowedPaths) == 0 || len(g.Capabilities) == 0 || len(g.RiskClasses) == 0 {
		return errors.New("delegation must explicitly grant objective classes, paths, capabilities, and risk classes")
	}
	if _, err := validateObjectiveClasses(g.ObjectiveClasses); err != nil {
		return fmt.Errorf("delegation: %w", err)
	}
	if _, err := normalizeAllowedPaths(g.AllowedPaths); err != nil {
		return fmt.Errorf("delegation allowedPaths: %w", err)
	}
	if _, err := NormalizeCapabilities(capabilityStrings(g.Capabilities)); err != nil {
		return fmt.Errorf("delegation capabilities: %w", err)
	}
	if _, err := validateRiskClasses(g.RiskClasses); err != nil {
		return fmt.Errorf("delegation: %w", err)
	}
	if strings.TrimSpace(g.Provider) == "" || strings.TrimSpace(g.Model) == "" || strings.TrimSpace(g.PaseoMode) == "" {
		return errors.New("delegation must name an exact provider, model, and Paseo mode")
	}
	if _, err := ValidatePaseoRoute(g.Provider, g.Model, g.PaseoMode); err != nil {
		return err
	}
	if g.MaxAttempts < 1 || g.MaxAttempts > MaxSpecBudgetAttempts ||
		g.MaxTimeoutSecs < MinSpecBudgetTimeout || g.MaxTimeoutSecs > MaxSpecBudgetTimeout ||
		g.MaxRepairCycles < 0 || g.MaxRepairCycles > MaxCampaignRepairCycles ||
		g.MaxWallClockSecs < MinCampaignWallClockSecs || g.MaxWallClockSecs > MaxCampaignWallClockSecs ||
		g.MaxParallel < 1 || g.MaxParallel > MaxCampaignParallel || g.MaxSessions < 1 || g.MaxSessions > MaxLeadSessions {
		return errors.New("delegation budgets must be explicit and within supported bounds")
	}
	if g.MaxRequests < 1 || g.MaxRequests > MaxDelegationRequests ||
		g.MaxCumulativeSessions < g.MaxSessions || g.MaxCumulativeSessions > MaxCumulativeLeadSessions ||
		g.MaxCumulativeAttempts < g.MaxAttempts || g.MaxCumulativeAttempts > MaxCumulativeLeadAttempts {
		return errors.New("delegation cumulative limits must be explicit, ordered, and within supported bounds")
	}
	autonomous, err := validateAutonomousRiskClasses(g.AutonomousRiskClasses, g.RiskClasses)
	if err != nil {
		return err
	}
	if g.AutonomousStart != (len(autonomous) > 0) {
		return errors.New("autonomousStart must exactly match a nonempty autonomous risk-class list")
	}
	return nil
}

func validateAutonomousRiskClasses(values, granted []RiskClass) (map[RiskClass]bool, error) {
	valid, err := validateRiskClasses([]RiskClass{RiskR0, RiskR1})
	if err != nil {
		return nil, err
	}
	grantedSet, err := validateRiskClasses(granted)
	if err != nil {
		return nil, err
	}
	out := map[RiskClass]bool{}
	for _, value := range values {
		if !valid[value] {
			return nil, fmt.Errorf("autonomous risk class %q is not permitted", value)
		}
		if !grantedSet[value] {
			return nil, fmt.Errorf("autonomous risk class %q is outside the delegation", value)
		}
		out[value] = true
	}
	return out, nil
}

// LeadWorkUnit is a proposed implementation node. Core adds the integration
// node for multi-unit graphs; models cannot create arbitrary roles.
type LeadWorkUnit struct {
	ID                 string       `json:"id"`
	Kind               string       `json:"kind"`
	Description        string       `json:"description,omitempty"`
	DependsOn          []string     `json:"dependsOn"`
	AllowedPaths       []string     `json:"allowedPaths"`
	Capabilities       []Capability `json:"capabilities"`
	AcceptanceCriteria []SpecCheck  `json:"acceptanceCriteria,omitempty"`
	NegativeCases      []SpecCheck  `json:"negativeCases,omitempty"`
}

// LeadPlanProposal is advisory model output bound to one current Action and
// Project snapshot. It is not itself execution authority.
// Goal returns the bounded per-unit instruction derived from the objective
// and the model's unit description. The description is advisory; Core scope
// remains the frozen spec.
func (u LeadWorkUnit) Goal(objective string) string {
	description := strings.TrimSpace(u.Description)
	if description == "" {
		return objective
	}
	goal := objective + "\n\nWorkUnit " + u.ID + ": " + description
	if len(goal) > MaxSpecGoalChars {
		goal = goal[:MaxSpecGoalChars]
	}
	return goal
}

type LeadPlanProposal struct {
	ProjectID          string             `json:"projectId"`
	ActionID           string             `json:"actionId"`
	BaseRevision       string             `json:"baseRevision"`
	ProjectConfigHash  string             `json:"projectConfigHash"`
	Objective          string             `json:"objective"`
	Class              LeadObjectiveClass `json:"class"`
	Assumptions        []string           `json:"assumptions"`
	Risks              []string           `json:"risks"`
	AcceptanceCriteria []SpecCheck        `json:"acceptanceCriteria"`
	NegativeCases      []SpecCheck        `json:"negativeCases"`
	QualityProfile     QualityProfile     `json:"qualityProfile"`
	RiskClass          RiskClass          `json:"riskClass"`
	Budget             SpecBudget         `json:"budget"`
	Provider           string             `json:"provider"`
	Model              string             `json:"model"`
	PaseoMode          string             `json:"paseoMode"`
	MaxRepairCycles    int                `json:"maxRepairCycles"`
	MaxWallClockSecs   int                `json:"maxWallClockSecs"`
	MaxParallel        int                `json:"maxParallel"`
	MaxSessions        int                `json:"maxSessions"`
	WorkUnits          []LeadWorkUnit     `json:"workUnits"`
}

// ContentHash returns the deterministic identity used to bind idempotent
// persistence to the exact proposal body.
func (p LeadPlanProposal) ContentHash() string {
	raw, err := json.Marshal(p)
	if err != nil {
		return ""
	}
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}

// Validate checks a proposal against the active Project, Action and an
// owner-issued delegation. It performs no IO and never broadens authority.
func (p LeadPlanProposal) Validate(g StandingDelegation, project Project, action Action, now time.Time) error {
	if err := g.Validate(now); err != nil {
		return err
	}
	if err := project.Validate(); err != nil {
		return fmt.Errorf("project: %w", err)
	}
	if !project.Registered() || !project.Active() {
		return errors.New("planning requires an active registered project")
	}
	if p.ProjectID != project.ID || g.ProjectID != project.ID || action.ProjectID != project.ID {
		return errors.New("proposal, delegation, action, and project identities must match")
	}
	if p.ActionID != action.ID || len(action.ID) != 32 || !isLowerHex(action.ID) {
		return errors.New("proposal actionId must identify the bound Action")
	}
	if action.Status != StatusPending && action.Status != StatusInProgress {
		return errors.New("planning requires a nonterminal Action")
	}
	if p.ProjectConfigHash == "" || p.ProjectConfigHash != project.ConfigHash {
		return errors.New("proposal projectConfigHash is stale or mismatched")
	}
	if !isGitObjectID(p.BaseRevision) {
		return errors.New("baseRevision must be a full git object id")
	}
	if strings.TrimSpace(p.Objective) == "" || len(p.Objective) > MaxSpecGoalChars {
		return errors.New("objective is required and bounded")
	}
	if !containsObjectiveClass(g.ObjectiveClasses, p.Class) {
		return errors.New("objective class is outside the delegation")
	}
	if err := validateBoundedNotes("assumptions", p.Assumptions); err != nil {
		return err
	}
	if err := validateBoundedNotes("risks", p.Risks); err != nil {
		return err
	}
	profile := p.QualityProfile
	if profile != project.QualityProfile {
		return errors.New("proposal quality profile differs from registered Project")
	}
	_, err := RequiredCapabilities(profile)
	if err != nil {
		return err
	}
	checks, err := normalizeChecks(p.AcceptanceCriteria, false)
	if err != nil {
		return fmt.Errorf("acceptanceCriteria: %w", err)
	}
	negative, err := normalizeChecks(p.NegativeCases, true)
	if err != nil {
		return fmt.Errorf("negativeCases: %w", err)
	}
	if len(checks) == 0 || len(negative) == 0 {
		return errors.New("at least one acceptance criterion and one negative case are required")
	}
	requiredKinds, _ := profile.RequiredKinds()
	if err := requireKinds(append(checks, negative...), requiredKinds); err != nil {
		return fmt.Errorf("quality profile: %w", err)
	}
	if !containsRiskClass(g.RiskClasses, p.RiskClass) {
		return errors.New("risk class is outside the delegation")
	}
	if p.Budget.MaxAttempts < 1 || p.Budget.MaxAttempts > g.MaxAttempts || p.Budget.MaxAttempts > MaxSpecBudgetAttempts ||
		p.Budget.TimeoutSecs < MinSpecBudgetTimeout || p.Budget.TimeoutSecs > g.MaxTimeoutSecs || p.Budget.TimeoutSecs > MaxSpecBudgetTimeout {
		return errors.New("spec attempts or timeout exceed the delegation")
	}
	if p.Provider != g.Provider || p.Model != g.Model || p.PaseoMode != g.PaseoMode {
		return errors.New("provider, model, and Paseo mode must match the delegated route")
	}
	if p.MaxRepairCycles < 0 || p.MaxRepairCycles > g.MaxRepairCycles || p.MaxRepairCycles > MaxCampaignRepairCycles ||
		p.MaxWallClockSecs < MinCampaignWallClockSecs || p.MaxWallClockSecs > g.MaxWallClockSecs || p.MaxWallClockSecs > MaxCampaignWallClockSecs ||
		p.MaxParallel < 1 || p.MaxParallel > g.MaxParallel || p.MaxParallel > MaxCampaignParallel ||
		p.MaxSessions < 1 || p.MaxSessions > g.MaxSessions || p.MaxSessions > MaxLeadSessions {
		return errors.New("campaign budgets exceed the delegation or supported bounds")
	}
	if len(p.WorkUnits) < 1 || len(p.WorkUnits) > MaxLeadWorkUnits {
		return fmt.Errorf("work graph requires 1..%d implementation units", MaxLeadWorkUnits)
	}
	if err := p.validateWorkUnits(project, g); err != nil {
		return err
	}
	implementerAttempts := p.ImplementerTaskCount() * p.Budget.MaxAttempts
	if implementerAttempts > MaxCampaignImplementerRuns {
		return fmt.Errorf("work graph requires %d implementer attempts, above the supported campaign ceiling", implementerAttempts)
	}
	if 1+implementerAttempts > p.MaxSessions {
		return errors.New("assessment and model-backed work-unit/repair attempts exceed the planned session ceiling")
	}
	return nil
}

// TerminalUnits returns units that no other unit depends on.
func (p LeadPlanProposal) TerminalUnits() []LeadWorkUnit {
	dependedOn := map[string]bool{}
	for _, unit := range p.WorkUnits {
		for _, dep := range unit.DependsOn {
			dependedOn[dep] = true
		}
	}
	var terminal []LeadWorkUnit
	for _, unit := range p.WorkUnits {
		if !dependedOn[unit.ID] {
			terminal = append(terminal, unit)
		}
	}
	return terminal
}

// NeedsIntegration reports whether multiple terminal units require a
// Core-owned integration node.
func (p LeadPlanProposal) NeedsIntegration() bool { return len(p.TerminalUnits()) > 1 }

// ImplementerTaskCount returns the maximum number of model-backed implementer
// executions the frozen plan can dispatch. Deterministic integration is not a
// model attempt and does not consume the provider session budget.
func (p LeadPlanProposal) ImplementerTaskCount() int {
	return len(p.WorkUnits) + p.MaxRepairCycles
}

// UnitScope returns the effective paths and capabilities for one unit. A
// multi-predecessor unit must be able to carry the merged prerequisite
// outputs through integration, so its frozen spec covers its transitive
// dependencies. A single-predecessor unit starts from that candidate and
// keeps its own scope.
func (p LeadPlanProposal) UnitScope(unitID string) ([]string, []Capability) {
	byID := map[string]LeadWorkUnit{}
	for _, unit := range p.WorkUnits {
		byID[unit.ID] = unit
	}
	paths := map[string]bool{}
	capabilities := map[Capability]bool{}
	seen := map[string]bool{}
	var add func(string)
	add = func(id string) {
		if seen[id] {
			return
		}
		seen[id] = true
		unit, ok := byID[id]
		if !ok {
			return
		}
		for _, path := range unit.AllowedPaths {
			paths[path] = true
		}
		for _, capability := range unit.Capabilities {
			capabilities[capability] = true
		}
		for _, dep := range unit.DependsOn {
			add(dep)
		}
	}
	unit, ok := byID[unitID]
	if !ok {
		return nil, nil
	}
	for _, path := range unit.AllowedPaths {
		paths[path] = true
	}
	for _, capability := range unit.Capabilities {
		capabilities[capability] = true
	}
	if len(unit.DependsOn) > 1 {
		for _, dep := range unit.DependsOn {
			add(dep)
		}
	}
	outPaths := make([]string, 0, len(paths))
	for path := range paths {
		outPaths = append(outPaths, path)
	}
	sort.Strings(outPaths)
	outCapabilities := make([]Capability, 0, len(capabilities))
	for capability := range capabilities {
		outCapabilities = append(outCapabilities, capability)
	}
	sort.Slice(outCapabilities, func(i, j int) bool { return outCapabilities[i] < outCapabilities[j] })
	return outPaths, outCapabilities
}

func (p LeadPlanProposal) validateWorkUnits(project Project, grant StandingDelegation) error {
	byID := make(map[string]LeadWorkUnit, len(p.WorkUnits))
	pathsByID := make(map[string][]string, len(p.WorkUnits))
	grantPaths, err := normalizeAllowedPaths(grant.AllowedPaths)
	if err != nil {
		return fmt.Errorf("delegation allowedPaths: %w", err)
	}
	required, _ := RequiredCapabilities(p.QualityProfile)
	requiredKinds, _ := p.QualityProfile.RequiredKinds()
	for _, unit := range p.WorkUnits {
		if unit.ID == "" || len(unit.ID) > MaxLeadWorkUnitIDChars || !isSlug(unit.ID) {
			return errors.New("work unit id is invalid or exceeds its bound")
		}
		if _, exists := byID[unit.ID]; exists {
			return fmt.Errorf("duplicate work unit id %q", unit.ID)
		}
		byID[unit.ID] = unit
		if LeadWorkUnitKind(unit.Kind) != LeadImplementationUnit {
			return fmt.Errorf("unsupported S4-D work unit kind %q", unit.Kind)
		}
		if len(unit.Description) > MaxLeadUnitDescriptionChars {
			return fmt.Errorf("work unit %q description exceeds its bound", unit.ID)
		}
		if len(unit.DependsOn) > MaxLeadWorkUnitDependencies {
			return fmt.Errorf("work unit %q exceeds the dependency bound", unit.ID)
		}
		paths, err := normalizeAllowedPaths(unit.AllowedPaths)
		if err != nil {
			return fmt.Errorf("work unit %q allowedPaths: %w", unit.ID, err)
		}
		for _, path := range paths {
			if !pathCoveredBy(path, grantPaths) {
				return fmt.Errorf("work unit %q path %q is outside the delegation", unit.ID, path)
			}
		}
		if err := SpecsWithinWorkspaceScope(project.WorkspaceScope, paths); err != nil {
			return fmt.Errorf("work unit %q: %w", unit.ID, err)
		}
		if !CapabilitiesSatisfied(project.Capabilities, unit.Capabilities) || !CapabilitiesSatisfied(grant.Capabilities, unit.Capabilities) {
			return fmt.Errorf("work unit %q capability is outside the project or delegation", unit.ID)
		}
		if missing := MissingCapabilities(unit.Capabilities, required); len(missing) > 0 {
			return fmt.Errorf("work unit %q omits required capabilities %v", unit.ID, missing)
		}
		if _, err := NormalizeCapabilities(capabilityStrings(unit.Capabilities)); err != nil {
			return fmt.Errorf("work unit %q capabilities: %w", unit.ID, err)
		}
		if len(unit.AcceptanceCriteria) > 0 || len(unit.NegativeCases) > 0 {
			acceptance, err := normalizeChecks(unit.AcceptanceCriteria, false)
			if err != nil {
				return fmt.Errorf("work unit %q acceptanceCriteria: %w", unit.ID, err)
			}
			negative, err := normalizeChecks(unit.NegativeCases, true)
			if err != nil {
				return fmt.Errorf("work unit %q negativeCases: %w", unit.ID, err)
			}
			if len(acceptance) == 0 || len(negative) == 0 {
				return fmt.Errorf("work unit %q must provide both acceptance and negative checks", unit.ID)
			}
			if err := requireKinds(append(acceptance, negative...), requiredKinds); err != nil {
				return fmt.Errorf("work unit %q quality profile: %w", unit.ID, err)
			}
		}
		pathsByID[unit.ID] = paths
	}
	for _, unit := range p.WorkUnits {
		seen := map[string]bool{}
		for _, dep := range unit.DependsOn {
			if dep == unit.ID {
				return fmt.Errorf("work unit %q depends on itself", unit.ID)
			}
			if _, exists := byID[dep]; !exists {
				return fmt.Errorf("work unit %q depends on unknown unit %q", unit.ID, dep)
			}
			if seen[dep] {
				return fmt.Errorf("work unit %q repeats dependency %q", unit.ID, dep)
			}
			seen[dep] = true
		}
	}
	visiting := map[string]bool{}
	visited := map[string]bool{}
	var visit func(string) error
	visit = func(id string) error {
		if visiting[id] {
			return fmt.Errorf("work graph dependency cycle at %q", id)
		}
		if visited[id] {
			return nil
		}
		visiting[id] = true
		for _, dep := range byID[id].DependsOn {
			if err := visit(dep); err != nil {
				return err
			}
		}
		visiting[id] = false
		visited[id] = true
		return nil
	}
	for id := range byID {
		if err := visit(id); err != nil {
			return err
		}
	}
	for i, left := range p.WorkUnits {
		for _, right := range p.WorkUnits[i+1:] {
			if p.unitIsAncestor(left.ID, right.ID, map[string]bool{}) || p.unitIsAncestor(right.ID, left.ID, map[string]bool{}) {
				continue
			}
			leftPaths, _ := p.UnitScope(left.ID)
			rightPaths, _ := p.UnitScope(right.ID)
			for _, a := range leftPaths {
				for _, b := range rightPaths {
					if pathsOverlap(a, b) {
						return fmt.Errorf("work units %q and %q can run concurrently with overlapping path %q", left.ID, right.ID, a)
					}
				}
			}
		}
	}
	return nil
}

func (p LeadPlanProposal) unitIsAncestor(ancestor, node string, seen map[string]bool) bool {
	if seen[node] {
		return false
	}
	seen[node] = true
	for _, unit := range p.WorkUnits {
		if unit.ID != node {
			continue
		}
		for _, dep := range unit.DependsOn {
			if dep == ancestor || p.unitIsAncestor(ancestor, dep, seen) {
				return true
			}
		}
	}
	return false
}

func pathsOverlap(a, b string) bool {
	return a == b || strings.HasPrefix(a, b+"/") || strings.HasPrefix(b, a+"/")
}

func pathCoveredBy(candidate string, grants []string) bool {
	for _, allowed := range grants {
		if candidate == allowed || strings.HasPrefix(candidate, strings.TrimSuffix(allowed, "/")+"/") {
			return true
		}
	}
	return false
}

func validateObjectiveClasses(values []LeadObjectiveClass) (map[LeadObjectiveClass]bool, error) {
	valid := map[LeadObjectiveClass]bool{}
	for _, value := range values {
		switch value {
		case LeadFeature, LeadMaintenance, LeadRefactor:
			valid[value] = true
		default:
			return nil, fmt.Errorf("unsupported objective class %q", value)
		}
	}
	if len(valid) == 0 {
		return nil, errors.New("at least one objective class is required")
	}
	return valid, nil
}

func containsObjectiveClass(values []LeadObjectiveClass, candidate LeadObjectiveClass) bool {
	set, err := validateObjectiveClasses(values)
	return err == nil && set[candidate]
}

func validateRiskClasses(values []RiskClass) (map[RiskClass]bool, error) {
	valid := map[RiskClass]bool{}
	for _, value := range values {
		if _, err := ParseRiskClass(string(value)); err != nil {
			return nil, err
		}
		valid[value] = true
	}
	if len(valid) == 0 {
		return nil, errors.New("at least one risk class is required")
	}
	return valid, nil
}

func containsRiskClass(values []RiskClass, candidate RiskClass) bool {
	set, err := validateRiskClasses(values)
	return err == nil && set[candidate]
}

func validateBoundedNotes(name string, values []string) error {
	if len(values) > 20 {
		return fmt.Errorf("%s carries at most 20 entries", name)
	}
	for _, value := range values {
		if strings.TrimSpace(value) == "" || len(value) > MaxFindingChars {
			return fmt.Errorf("%s entries must be nonempty and at most %d characters", name, MaxFindingChars)
		}
	}
	return nil
}
