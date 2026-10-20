package domain

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"path"
	"strings"
	"time"
)

// F1-S4-A quality pipeline domain. An ActionSpec is the frozen normative
// input for one engineering Action; a CandidateRevision is the exact,
// immutable git content produced by the Implementer; verifier/reviewer
// evidence and the Core Quality Gate decision are bound to both identities.
// Nothing in this file performs IO: validation and identity rules have one
// source of truth independent of HTTP, git, SQLite and the Paseo adapter.

// QualityPolicyRevision identifies the protected global gate policy. A
// Quality Profile may add checks; it can never remove or weaken these.
const QualityPolicyRevision = "quality-baseline-s4a-v1"

// RiskClass is the approved risk vocabulary. The final diff is reclassified
// by the Core; a spec label is not evidence.
type RiskClass string

const (
	RiskR0 RiskClass = "R0"
	RiskR1 RiskClass = "R1"
	RiskR2 RiskClass = "R2"
	RiskR3 RiskClass = "R3"
)

// ParseRiskClass validates an operator-supplied risk class.
func ParseRiskClass(raw string) (RiskClass, error) {
	switch RiskClass(strings.TrimSpace(raw)) {
	case RiskR0, RiskR1, RiskR2, RiskR3:
		return RiskClass(strings.TrimSpace(raw)), nil
	default:
		return "", fmt.Errorf("unknown riskClass %q (want R0|R1|R2|R3)", raw)
	}
}

// QualityProfile names a stack profile. Profiles add mandatory check kinds
// to the global baseline; they never remove any.
type QualityProfile string

const (
	ProfileNodeBasic   QualityProfile = "node-basic"
	ProfileNodeBrowser QualityProfile = "node-browser"
	ProfileGeneric     QualityProfile = "generic"
)

// RequiredKinds returns the mandatory check kinds for a profile. The global
// baseline always requires functional and negative behavior; these profiles
// add the stack-specific static/test checks.
func (p QualityProfile) RequiredKinds() ([]CheckKind, error) {
	switch p {
	case ProfileNodeBasic:
		return []CheckKind{CheckFormat, CheckTest, CheckFunctional, CheckNegative}, nil
	case ProfileNodeBrowser:
		return []CheckKind{CheckFormat, CheckTest, CheckFunctional, CheckNegative, CheckBrowser}, nil
	case ProfileGeneric:
		return []CheckKind{CheckBuild, CheckTest, CheckFunctional, CheckNegative}, nil
	default:
		return nil, fmt.Errorf("unknown qualityProfile %q", p)
	}
}

// CheckKind is the closed vocabulary shared by specs, verifier evidence and
// the Quality Gate.
type CheckKind string

const (
	CheckFormat     CheckKind = "format"
	CheckLint       CheckKind = "lint"
	CheckBuild      CheckKind = "build"
	CheckTest       CheckKind = "test"
	CheckFunctional CheckKind = "functional"
	CheckNegative   CheckKind = "negative"
	CheckBrowser    CheckKind = "browser"
	// Synthetic gate checks are derived by the Core, never submitted by a
	// verifier or reviewer.
	CheckIdentity     CheckKind = "identity"
	CheckScope        CheckKind = "scope"
	CheckIndependence CheckKind = "independence"
	CheckReview       CheckKind = "review"
	CheckSecurity     CheckKind = "security"
	CheckFreshness    CheckKind = "freshness"
)

// ParseSpecCheckKind accepts only executable acceptance kinds.
func ParseSpecCheckKind(raw string) (CheckKind, error) {
	switch CheckKind(strings.TrimSpace(raw)) {
	case CheckFormat, CheckLint, CheckBuild, CheckTest, CheckFunctional, CheckNegative, CheckBrowser:
		return CheckKind(strings.TrimSpace(raw)), nil
	default:
		return "", fmt.Errorf("unknown check kind %q", raw)
	}
}

// CheckStatus is the per-check result vocabulary. No unchecked status means
// PASS.
type CheckStatus string

const (
	CheckPass          CheckStatus = "PASS"
	CheckFail          CheckStatus = "FAIL"
	CheckSkipped       CheckStatus = "SKIPPED"
	CheckNotApplicable CheckStatus = "NOT_APPLICABLE"
	CheckNotTested     CheckStatus = "NOT_TESTED"
)

// GateStatus is the normative quality outcome.
type GateStatus string

const (
	GateAccepted      GateStatus = "ACCEPTED"
	GateRepair        GateStatus = "REPAIR_REQUIRED"
	GateBlocked       GateStatus = "BLOCKED"
	GateHumanRequired GateStatus = "HUMAN_REQUIRED"
	GateNotTested     GateStatus = "NOT_TESTED"
)

// EvidenceStatus is the verifier bundle's aggregate outcome.
type EvidenceStatus string

const (
	EvidencePassed    EvidenceStatus = "passed"
	EvidenceFailed    EvidenceStatus = "failed"
	EvidenceBlocked   EvidenceStatus = "blocked"
	EvidenceNotTested EvidenceStatus = "not_tested"
)

// Bounds for untrusted spec, evidence and review input.
const (
	MaxSpecGoalChars       = 4000
	MaxAllowedPaths        = 32
	MaxAllowedPathChars    = 256
	MaxSpecChecks          = 16
	MaxCheckCommandArgs    = 8
	MaxCheckCommandChars   = 1024
	MaxCheckArgChars       = 512
	MaxCheckDescription    = 500
	MaxCheckTimeoutSecs    = 600
	MaxCheckExitCode       = 127
	MaxFindings            = 20
	MaxFindingChars        = 500
	MaxIdempotencyKeyChars = 128
	MaxEvidenceActorChars  = 64
	MaxCleanResourceChars  = 256
	MaxCommitHex           = 64
	MaxCandidateRefChars   = 256
	MaxSpecBudgetAttempts  = 5
	MaxSpecBudgetTimeout   = 600
	MinSpecBudgetTimeout   = 30
)

// SpecCheck is one executable acceptance or negative check. Command is an
// argv vector, never a shell string; the verifier executes it directly.
type SpecCheck struct {
	ID          string    `json:"id"`
	Kind        CheckKind `json:"kind"`
	Description string    `json:"description"`
	Command     []string  `json:"command"`
	ExpectExit  int       `json:"expectExit"`
	TimeoutSecs int       `json:"timeoutSecs"`
}

// SpecBudget bounds the Implementer execution.
type SpecBudget struct {
	MaxAttempts int `json:"maxAttempts"`
	TimeoutSecs int `json:"timeoutSecs"`
}

// ActionSpec is the immutable, versioned engineering contract for one
// Action. ContentHash covers every normative field except the Core-assigned
// ID and persistence timestamps.
type ActionSpec struct {
	ID                 string         `json:"id"`
	ProjectID          string         `json:"projectId"`
	ActionID           string         `json:"actionId"`
	Version            int            `json:"version"`
	ContentHash        string         `json:"contentHash"`
	PolicyRevision     string         `json:"policyRevision"`
	Goal               string         `json:"goal"`
	Repository         string         `json:"repository"`
	BaseRevision       string         `json:"baseRevision"`
	AllowedPaths       []string       `json:"allowedPaths"`
	AcceptanceCriteria []SpecCheck    `json:"acceptanceCriteria"`
	NegativeCases      []SpecCheck    `json:"negativeCases"`
	RiskClass          RiskClass      `json:"riskClass"`
	QualityProfile     QualityProfile `json:"qualityProfile"`
	Budget             SpecBudget     `json:"budget"`
	ExecutionMode      string         `json:"executionMode"`
	Provider           string         `json:"provider"`
	Model              string         `json:"model"`
	PaseoMode          string         `json:"paseoMode"`
	CreatedAt          time.Time      `json:"createdAt"`
	FrozenAt           time.Time      `json:"frozenAt"`
}

// NewActionSpecParams carries operator input. The Core assigns ID/version,
// validates the repository/base revision and freezes the row; no update
// path exists.
type NewActionSpecParams struct {
	ProjectID          string
	ActionID           string
	Goal               string
	Repository         string
	BaseRevision       string
	AllowedPaths       []string
	AcceptanceCriteria []SpecCheck
	NegativeCases      []SpecCheck
	RiskClass          string
	QualityProfile     string
	Budget             SpecBudget
	ExecutionMode      string
	Provider           string
	Model              string
	PaseoMode          string
}

// Validate normalizes and validates creation input, returning a spec whose
// ContentHash is computed and whose timestamps are unset.
func (p NewActionSpecParams) Validate() (ActionSpec, error) {
	projectID := strings.TrimSpace(p.ProjectID)
	if projectID == "" {
		projectID = DefaultProjectID
	}
	if err := ValidateProjectID(projectID); err != nil {
		return ActionSpec{}, err
	}
	actionID := strings.TrimSpace(p.ActionID)
	if len(actionID) != 32 || !isLowerHex(actionID) {
		return ActionSpec{}, errors.New("actionId must be a 32-character lowercase hex id")
	}
	goal := strings.TrimSpace(p.Goal)
	if goal == "" {
		return ActionSpec{}, errors.New("goal is required")
	}
	if len(goal) > MaxSpecGoalChars {
		return ActionSpec{}, fmt.Errorf("goal exceeds %d characters", MaxSpecGoalChars)
	}
	repository := strings.TrimSpace(p.Repository)
	if err := validateRepositoryPath(repository); err != nil {
		return ActionSpec{}, err
	}
	base := strings.TrimSpace(p.BaseRevision)
	if !isGitObjectID(base) {
		return ActionSpec{}, errors.New("baseRevision must be a full git object id")
	}
	allowed, err := normalizeAllowedPaths(p.AllowedPaths)
	if err != nil {
		return ActionSpec{}, err
	}
	acceptance, err := normalizeChecks(p.AcceptanceCriteria, false)
	if err != nil {
		return ActionSpec{}, fmt.Errorf("acceptanceCriteria: %w", err)
	}
	negative, err := normalizeChecks(p.NegativeCases, true)
	if err != nil {
		return ActionSpec{}, fmt.Errorf("negativeCases: %w", err)
	}
	risk, err := ParseRiskClass(p.RiskClass)
	if err != nil {
		return ActionSpec{}, err
	}
	profile := QualityProfile(strings.TrimSpace(p.QualityProfile))
	required, err := profile.RequiredKinds()
	if err != nil {
		return ActionSpec{}, err
	}
	if err := requireKinds(append(acceptance, negative...), required); err != nil {
		return ActionSpec{}, err
	}
	if len(acceptance) == 0 || len(negative) == 0 {
		return ActionSpec{}, errors.New("at least one acceptance criterion and one negative case are required")
	}
	budget := p.Budget
	if budget.MaxAttempts == 0 {
		budget.MaxAttempts = 1
	}
	if budget.MaxAttempts < 1 || budget.MaxAttempts > MaxSpecBudgetAttempts {
		return ActionSpec{}, fmt.Errorf("budget.maxAttempts must be 1..%d", MaxSpecBudgetAttempts)
	}
	if budget.TimeoutSecs == 0 {
		budget.TimeoutSecs = DefaultTimeoutSecs
	}
	if budget.TimeoutSecs < MinSpecBudgetTimeout || budget.TimeoutSecs > MaxSpecBudgetTimeout {
		return ActionSpec{}, fmt.Errorf("budget.timeoutSecs must be %d..%d",
			MinSpecBudgetTimeout, MaxSpecBudgetTimeout)
	}
	mode := strings.TrimSpace(p.ExecutionMode)
	if mode == "" {
		mode = string(ExecTrustedLocal)
	}
	if mode != string(ExecTrustedLocal) {
		return ActionSpec{}, fmt.Errorf("engineering Actions require executionMode %q (trusted-local is never isolated)",
			ExecTrustedLocal)
	}
	route, err := ValidatePaseoRoute(p.Provider, p.Model, p.PaseoMode)
	if err != nil {
		return ActionSpec{}, err
	}
	spec := ActionSpec{
		ProjectID:          projectID,
		ActionID:           actionID,
		Version:            1,
		PolicyRevision:     QualityPolicyRevision,
		Goal:               goal,
		Repository:         repository,
		BaseRevision:       base,
		AllowedPaths:       allowed,
		AcceptanceCriteria: acceptance,
		NegativeCases:      negative,
		RiskClass:          risk,
		QualityProfile:     profile,
		Budget:             budget,
		ExecutionMode:      mode,
		Provider:           route.Provider,
		Model:              route.Model,
		PaseoMode:          route.Mode,
	}
	spec.ContentHash = spec.ComputeHash()
	if err := spec.Validate(); err != nil {
		return ActionSpec{}, err
	}
	return spec, nil
}

// Validate rechecks every invariant on a loaded spec, including that its
// content hash matches the normative fields.
func (s ActionSpec) Validate() error {
	if s.ID != "" {
		if len(s.ID) != 32 || !isLowerHex(s.ID) {
			return errors.New("spec id must be a 32-character lowercase hex id")
		}
	}
	if err := ValidateProjectID(s.ProjectID); err != nil {
		return err
	}
	if len(s.ActionID) != 32 || !isLowerHex(s.ActionID) {
		return errors.New("actionId must be a 32-character lowercase hex id")
	}
	if s.Version != 1 {
		return errors.New("action spec version must be 1")
	}
	if s.PolicyRevision != QualityPolicyRevision {
		return fmt.Errorf("unsupported policyRevision %q", s.PolicyRevision)
	}
	if strings.TrimSpace(s.Goal) == "" || len(s.Goal) > MaxSpecGoalChars {
		return errors.New("goal is required and bounded")
	}
	if err := validateRepositoryPath(s.Repository); err != nil {
		return err
	}
	if !isGitObjectID(s.BaseRevision) {
		return errors.New("baseRevision must be a full git object id")
	}
	if _, err := normalizeAllowedPaths(s.AllowedPaths); err != nil {
		return err
	}
	if _, err := normalizeChecks(s.AcceptanceCriteria, false); err != nil {
		return fmt.Errorf("acceptanceCriteria: %w", err)
	}
	if _, err := normalizeChecks(s.NegativeCases, true); err != nil {
		return fmt.Errorf("negativeCases: %w", err)
	}
	if _, err := ParseRiskClass(string(s.RiskClass)); err != nil {
		return err
	}
	required, err := s.QualityProfile.RequiredKinds()
	if err != nil {
		return err
	}
	if err := requireKinds(s.AllChecks(), required); err != nil {
		return err
	}
	if s.Budget.MaxAttempts < 1 || s.Budget.MaxAttempts > MaxSpecBudgetAttempts {
		return errors.New("budget.maxAttempts is out of range")
	}
	if s.Budget.TimeoutSecs < MinSpecBudgetTimeout || s.Budget.TimeoutSecs > MaxSpecBudgetTimeout {
		return errors.New("budget.timeoutSecs is out of range")
	}
	if s.ExecutionMode != string(ExecTrustedLocal) {
		return errors.New("spec executionMode must be trusted-local")
	}
	if _, err := ValidatePaseoRoute(s.Provider, s.Model, s.PaseoMode); err != nil {
		return err
	}
	if !isLowerHex(s.ContentHash) || len(s.ContentHash) != 64 {
		return errors.New("contentHash must be a sha256 hex digest")
	}
	if got := s.ComputeHash(); got != s.ContentHash {
		return errors.New("contentHash does not match the frozen spec content")
	}
	return nil
}

// AllChecks returns acceptance criteria followed by negative cases.
func (s ActionSpec) AllChecks() []SpecCheck {
	out := make([]SpecCheck, 0, len(s.AcceptanceCriteria)+len(s.NegativeCases))
	out = append(out, s.AcceptanceCriteria...)
	out = append(out, s.NegativeCases...)
	return out
}

// ComputeHash returns the deterministic SHA-256 identity of the normative
// spec content. ID, version timestamps and ContentHash are excluded.
func (s ActionSpec) ComputeHash() string {
	payload := struct {
		ProjectID          string         `json:"projectId"`
		ActionID           string         `json:"actionId"`
		Version            int            `json:"version"`
		PolicyRevision     string         `json:"policyRevision"`
		Goal               string         `json:"goal"`
		Repository         string         `json:"repository"`
		BaseRevision       string         `json:"baseRevision"`
		AllowedPaths       []string       `json:"allowedPaths"`
		AcceptanceCriteria []SpecCheck    `json:"acceptanceCriteria"`
		NegativeCases      []SpecCheck    `json:"negativeCases"`
		RiskClass          RiskClass      `json:"riskClass"`
		QualityProfile     QualityProfile `json:"qualityProfile"`
		Budget             SpecBudget     `json:"budget"`
		ExecutionMode      string         `json:"executionMode"`
		Provider           string         `json:"provider"`
		Model              string         `json:"model"`
		PaseoMode          string         `json:"paseoMode"`
	}{
		ProjectID: s.ProjectID, ActionID: s.ActionID, Version: s.Version,
		PolicyRevision: s.PolicyRevision, Goal: s.Goal, Repository: s.Repository,
		BaseRevision: s.BaseRevision, AllowedPaths: s.AllowedPaths,
		AcceptanceCriteria: s.AcceptanceCriteria, NegativeCases: s.NegativeCases,
		RiskClass: s.RiskClass, QualityProfile: s.QualityProfile, Budget: s.Budget,
		ExecutionMode: s.ExecutionMode, Provider: s.Provider, Model: s.Model,
		PaseoMode: s.PaseoMode,
	}
	raw, err := json.Marshal(payload)
	if err != nil {
		return ""
	}
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}

// OracleDigest binds verifier/reviewer evidence to the exact frozen check
// oracle rather than to a verifier-supplied description of it.
func (s ActionSpec) OracleDigest() string {
	raw, err := json.Marshal(struct {
		Acceptance []SpecCheck `json:"acceptanceCriteria"`
		Negative   []SpecCheck `json:"negativeCases"`
	}{Acceptance: s.AcceptanceCriteria, Negative: s.NegativeCases})
	if err != nil {
		return ""
	}
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}

// CheckByID returns the expected spec check for an evidence result.
func (s ActionSpec) CheckByID(id string) (SpecCheck, bool) {
	for _, check := range s.AllChecks() {
		if check.ID == id {
			return check, true
		}
	}
	return SpecCheck{}, false
}

func normalizeAllowedPaths(raw []string) ([]string, error) {
	if len(raw) == 0 {
		return nil, errors.New("allowedPaths must contain at least one repository-relative path")
	}
	if len(raw) > MaxAllowedPaths {
		return nil, fmt.Errorf("allowedPaths carries at most %d entries", MaxAllowedPaths)
	}
	seen := map[string]bool{}
	out := make([]string, 0, len(raw))
	for _, entry := range raw {
		p := strings.TrimSpace(entry)
		if err := validateRelativePath(p); err != nil {
			return nil, err
		}
		if seen[p] {
			continue
		}
		seen[p] = true
		out = append(out, p)
	}
	return out, nil
}

func validateRelativePath(p string) error {
	if p == "" {
		return errors.New("allowed path must not be empty")
	}
	if len(p) > MaxAllowedPathChars {
		return fmt.Errorf("allowed path exceeds %d characters", MaxAllowedPathChars)
	}
	if strings.HasPrefix(p, "/") || strings.Contains(p, "\\") {
		return fmt.Errorf("allowed path %q must be repository-relative", p)
	}
	for _, segment := range strings.Split(p, "/") {
		if segment == ".." || segment == ".git" || segment == "" {
			return fmt.Errorf("allowed path %q is not a safe repository path", p)
		}
	}
	if strings.ContainsAny(p, "\x00\n\r") {
		return fmt.Errorf("allowed path %q carries a control character", p)
	}
	return nil
}

func normalizeChecks(raw []SpecCheck, negative bool) ([]SpecCheck, error) {
	if len(raw) == 0 {
		return nil, nil
	}
	if len(raw) > MaxSpecChecks {
		return nil, fmt.Errorf("at most %d checks are allowed", MaxSpecChecks)
	}
	seen := map[string]bool{}
	out := make([]SpecCheck, 0, len(raw))
	for _, check := range raw {
		id := strings.TrimSpace(check.ID)
		if len(id) == 0 || len(id) > 64 || !isSlug(id) {
			return nil, fmt.Errorf("check id %q must match [a-z0-9-_]{1,64}", check.ID)
		}
		if seen[id] {
			return nil, fmt.Errorf("duplicate check id %q", id)
		}
		seen[id] = true
		kind, err := ParseSpecCheckKind(string(check.Kind))
		if err != nil {
			return nil, err
		}
		if negative && kind != CheckNegative {
			return nil, fmt.Errorf("check %q in negativeCases must have kind %q", id, CheckNegative)
		}
		description := strings.TrimSpace(check.Description)
		if description == "" || len(description) > MaxCheckDescription {
			return nil, fmt.Errorf("check %q requires a bounded description", id)
		}
		command, err := normalizeCommand(check.Command)
		if err != nil {
			return nil, fmt.Errorf("check %q: %w", id, err)
		}
		if check.ExpectExit < 0 || check.ExpectExit > MaxCheckExitCode {
			return nil, fmt.Errorf("check %q expectExit must be 0..%d", id, MaxCheckExitCode)
		}
		timeout := check.TimeoutSecs
		if timeout == 0 {
			timeout = 120
		}
		if timeout < 1 || timeout > MaxCheckTimeoutSecs {
			return nil, fmt.Errorf("check %q timeoutSecs must be 1..%d", id, MaxCheckTimeoutSecs)
		}
		out = append(out, SpecCheck{
			ID: id, Kind: kind, Description: description, Command: command,
			ExpectExit: check.ExpectExit, TimeoutSecs: timeout,
		})
	}
	return out, nil
}

// executableAllowlist bounds the tools an acceptance oracle may invoke. The
// verifier executes argv directly (no shell), so this list is the complete
// command surface of a frozen spec.
var executableAllowlist = map[string]bool{
	"node": true, "npm": true, "npx": true, "go": true, "gofmt": true,
	"python3": true, "git": true, "bash": true, "make": true, "cargo": true,
}

func normalizeCommand(raw []string) ([]string, error) {
	if len(raw) == 0 || len(raw) > MaxCheckCommandArgs {
		return nil, fmt.Errorf("command must carry 1..%d argv entries", MaxCheckCommandArgs)
	}
	total := 0
	out := make([]string, 0, len(raw))
	for _, arg := range raw {
		if arg == "" || len(arg) > MaxCheckArgChars {
			return nil, errors.New("command argument is empty or too long")
		}
		if strings.ContainsAny(arg, "\x00\n\r") {
			return nil, errors.New("command argument carries a control character")
		}
		total += len(arg)
		out = append(out, arg)
	}
	if total > MaxCheckCommandChars {
		return nil, fmt.Errorf("command exceeds %d characters", MaxCheckCommandChars)
	}
	exe := path.Base(out[0])
	if !executableAllowlist[exe] {
		return nil, fmt.Errorf("executable %q is not in the acceptance allowlist", exe)
	}
	return out, nil
}

func requireKinds(checks []SpecCheck, required []CheckKind) error {
	present := map[CheckKind]bool{}
	for _, check := range checks {
		present[check.Kind] = true
	}
	for _, kind := range required {
		if !present[kind] {
			return fmt.Errorf("quality profile requires at least one %q check", kind)
		}
	}
	return nil
}

func validateRepositoryPath(repository string) error {
	if repository == "" {
		return errors.New("repository is required")
	}
	if len(repository) > 1024 {
		return errors.New("repository path exceeds 1024 characters")
	}
	if !path.IsAbs(repository) {
		return errors.New("repository must be an absolute local path")
	}
	if path.Clean(repository) != repository {
		return errors.New("repository must be a clean absolute path")
	}
	if strings.ContainsAny(repository, "\x00\n\r") {
		return errors.New("repository carries a control character")
	}
	return nil
}

func isSlug(value string) bool {
	for _, r := range value {
		if r >= 'a' && r <= 'z' || r >= '0' && r <= '9' || r == '-' || r == '_' {
			continue
		}
		return false
	}
	return true
}

func isLowerHex(value string) bool {
	if value == "" {
		return false
	}
	for _, r := range value {
		ok := r >= '0' && r <= '9' || r >= 'a' && r <= 'f'
		if !ok {
			return false
		}
	}
	return true
}

func isGitObjectID(value string) bool {
	return (len(value) == 40 || len(value) == MaxCommitHex) && isLowerHex(value)
}

// PathAllowed reports whether a repository-relative changed path is inside
// the authorized scope. Patterns support exact files, directory prefixes,
// "*" globs and "dir/**" descendants.
func PathAllowed(changed string, allowed []string) bool {
	if changed == "" || strings.HasPrefix(changed, "/") || hasUnsafeSegment(changed) {
		return false
	}
	for _, pattern := range allowed {
		if matchPath(changed, pattern) {
			return true
		}
	}
	return false
}

func hasUnsafeSegment(value string) bool {
	for _, segment := range strings.Split(value, "/") {
		if segment == ".." || segment == ".git" || segment == "" {
			return true
		}
	}
	return false
}

func matchPath(changed, pattern string) bool {
	if changed == pattern {
		return true
	}
	if strings.HasSuffix(pattern, "/**") {
		prefix := strings.TrimSuffix(pattern, "/**")
		return changed == prefix || strings.HasPrefix(changed, prefix+"/")
	}
	if strings.HasSuffix(pattern, "/") {
		return strings.HasPrefix(changed, pattern)
	}
	if strings.Contains(pattern, "*") {
		if ok, _ := path.Match(pattern, changed); ok {
			return true
		}
		if !strings.Contains(pattern, "/") {
			ok, _ := path.Match(pattern, path.Base(changed))
			return ok
		}
		return false
	}
	return strings.HasPrefix(changed, pattern+"/")
}
