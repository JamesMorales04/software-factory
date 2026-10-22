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

// LeadObjectiveClass is the closed set of objective classes allowed in the
// first bounded planning slice.
type LeadObjectiveClass string

const (
	LeadFeature     LeadObjectiveClass = "feature"
	LeadMaintenance LeadObjectiveClass = "maintenance"
	LeadRefactor    LeadObjectiveClass = "refactor"
)

// LeadWorkUnitKind is intentionally closed. S4-D accepts one implementation
// unit and delegates its execution to the existing ActionSpec/Campaign path.
type LeadWorkUnitKind string

const LeadImplementationUnit LeadWorkUnitKind = "implementation"

const MaxLeadSessions = MaxCampaignTasks * MaxCampaignTaskExecutions

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
	ExpiresAt        time.Time            `json:"expiresAt"`
	RevokedAt        time.Time            `json:"revokedAt,omitempty"`
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
	return nil
}

// LeadWorkUnit is a proposed graph node. Only one implementation node is
// executable in S4-D; later slices may define additional closed unit kinds.
type LeadWorkUnit struct {
	ID           string       `json:"id"`
	Kind         string       `json:"kind"`
	DependsOn    []string     `json:"dependsOn"`
	AllowedPaths []string     `json:"allowedPaths"`
	Capabilities []Capability `json:"capabilities"`
}

// LeadPlanProposal is advisory model output bound to one current Action and
// Project snapshot. It is not itself execution authority.
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
	if 1+p.Budget.MaxAttempts*(p.MaxRepairCycles+1) > p.MaxSessions {
		return errors.New("assessment and campaign attempts exceed the planned session ceiling")
	}
	if len(p.WorkUnits) != 1 {
		return errors.New("S4-D requires exactly one implementation work unit")
	}
	if err := p.validateWorkUnits(project, g); err != nil {
		return err
	}
	return nil
}

func (p LeadPlanProposal) validateWorkUnits(project Project, grant StandingDelegation) error {
	unit := p.WorkUnits[0]
	if unit.ID == "" || len(unit.ID) > 64 || !isSlug(unit.ID) {
		return errors.New("work unit id is invalid")
	}
	if LeadWorkUnitKind(unit.Kind) != LeadImplementationUnit {
		return fmt.Errorf("unsupported S4-D work unit kind %q", unit.Kind)
	}
	if len(unit.DependsOn) > 0 {
		for _, dep := range unit.DependsOn {
			if dep == unit.ID {
				return errors.New("work unit dependency cycle")
			}
			return fmt.Errorf("work unit dependency %q is missing", dep)
		}
	}
	paths, err := normalizeAllowedPaths(unit.AllowedPaths)
	if err != nil {
		return fmt.Errorf("work unit allowedPaths: %w", err)
	}
	grantPaths, err := normalizeAllowedPaths(grant.AllowedPaths)
	if err != nil {
		return fmt.Errorf("delegation allowedPaths: %w", err)
	}
	for _, path := range paths {
		if !pathCoveredBy(path, grantPaths) {
			return fmt.Errorf("work unit path %q is outside the delegation", path)
		}
	}
	if err := SpecsWithinWorkspaceScope(project.WorkspaceScope, paths); err != nil {
		return err
	}
	if !CapabilitiesSatisfied(project.Capabilities, unit.Capabilities) || !CapabilitiesSatisfied(grant.Capabilities, unit.Capabilities) {
		return errors.New("work unit capability is outside the project or delegation")
	}
	required, _ := RequiredCapabilities(p.QualityProfile)
	if missing := MissingCapabilities(unit.Capabilities, required); len(missing) > 0 {
		return fmt.Errorf("work unit omits required capabilities %v", missing)
	}
	if _, err := NormalizeCapabilities(capabilityStrings(unit.Capabilities)); err != nil {
		return fmt.Errorf("work unit capabilities: %w", err)
	}
	return nil
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
