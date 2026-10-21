package domain

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"net/url"
	"strings"
	"time"
)

// F1-S4-C Project Registry. A Project is the durable, Core-authorized
// binding between a logical project identity, exactly one local Git
// repository, its base-ref policy, quality profile and the capability set
// its runs may receive. TRUSTED_LOCAL remains broad host access: this is
// control-plane scoping, never OS isolation.

// ProjectRegistrationStatus distinguishes the seeded legacy project from
// explicitly registered projects. Legacy rows and their evidence survive;
// they never acquire a newly registered repository.
type ProjectRegistrationStatus string

const (
	ProjectLegacy     ProjectRegistrationStatus = "legacy"
	ProjectRegistered ProjectRegistrationStatus = "registered"
)

// ProjectStatus is the operator-visible execution gate.
type ProjectStatus string

const (
	ProjectActive ProjectStatus = "active"
	ProjectPaused ProjectStatus = "paused"
)

// Capability is the closed S4-C capability vocabulary. The Core grants
// capabilities from the Project configuration and matches them against the
// Worker runtime; a request never widens the grant.
type Capability string

const (
	CapRepositoryEdit   Capability = "repository-edit"
	CapNodeToolchain    Capability = "node-toolchain"
	CapGenericToolchain Capability = "generic-toolchain"
	CapBrowser          Capability = "browser"
)

// AllCapabilities is the complete supported set, in deterministic order.
var AllCapabilities = []Capability{
	CapRepositoryEdit, CapNodeToolchain, CapGenericToolchain, CapBrowser,
}

// ParseCapability validates one capability string.
func ParseCapability(raw string) (Capability, error) {
	value := Capability(strings.TrimSpace(raw))
	for _, known := range AllCapabilities {
		if value == known {
			return value, nil
		}
	}
	return "", fmt.Errorf("unknown capability %q", raw)
}

// NormalizeCapabilities trims, de-duplicates and validates a capability set.
func NormalizeCapabilities(raw []string) ([]Capability, error) {
	seen := map[Capability]bool{}
	out := make([]Capability, 0, len(raw))
	for _, entry := range raw {
		capability, err := ParseCapability(entry)
		if err != nil {
			return nil, err
		}
		if seen[capability] {
			continue
		}
		seen[capability] = true
		out = append(out, capability)
	}
	return out, nil
}

// ContainsCapability reports an exact grant.
func ContainsCapability(granted []Capability, wanted Capability) bool {
	for _, capability := range granted {
		if capability == wanted {
			return true
		}
	}
	return false
}

// CapabilitiesSatisfied reports whether granted is a superset of required.
func CapabilitiesSatisfied(granted, required []Capability) bool {
	for _, capability := range required {
		if !ContainsCapability(granted, capability) {
			return false
		}
	}
	return true
}

// MissingCapabilities returns the required capabilities absent from granted.
func MissingCapabilities(granted, required []Capability) []Capability {
	var missing []Capability
	for _, capability := range required {
		if !ContainsCapability(granted, capability) {
			missing = append(missing, capability)
		}
	}
	return missing
}

// RequiredCapabilities derives the capability snapshot implied by a quality
// profile. A profile can only add requirements; it never removes the
// repository-edit baseline.
func RequiredCapabilities(profile QualityProfile) ([]Capability, error) {
	switch profile {
	case ProfileNodeBasic:
		return []Capability{CapRepositoryEdit, CapNodeToolchain}, nil
	case ProfileNodeBrowser:
		return []Capability{CapRepositoryEdit, CapNodeToolchain, CapBrowser}, nil
	case ProfileGeneric:
		return []Capability{CapRepositoryEdit, CapGenericToolchain}, nil
	default:
		return nil, fmt.Errorf("unknown qualityProfile %q", profile)
	}
}

// BrowserTestServer is the declarative, Project-authorized startup contract
// for a browser-test application. The Worker starts the command in the
// Attempt workspace, waits for the authorized loopback endpoint and owns the
// process lifecycle. It is never a browser-capability evidence source.
type BrowserTestServer struct {
	Command            []string `json:"command"`
	ReadyPath          string   `json:"readyPath,omitempty"`
	StartupTimeoutSecs int      `json:"startupTimeoutSecs,omitempty"`
}

// Project bounds.
const (
	MaxProjectNameChars       = 120
	MaxRepositoryPathChars    = 1024
	MaxBaseRefs               = 8
	MaxBaseRefChars           = 128
	MaxWorkspaceScopeChars    = 256
	MaxBrowserEndpoints       = 4
	MaxBrowserEndpointChars   = 256
	TrustedLocalSecurityLabel = "TRUSTED_LOCAL"
	ProjectConfigVersion      = 1
)

// Project is the Core-authoritative project configuration. ConfigRevision
// and ConfigHash identify the exact revision frozen into an ActionSpec.
type Project struct {
	ID                 string                    `json:"id"`
	Name               string                    `json:"name"`
	RegistrationStatus ProjectRegistrationStatus `json:"registrationStatus"`
	Status             ProjectStatus             `json:"status"`
	RepositoryPath     string                    `json:"repositoryPath,omitempty"`
	RepositoryIdentity string                    `json:"repositoryIdentity,omitempty"`
	BaseRefs           []string                  `json:"baseRefs"`
	QualityProfile     QualityProfile            `json:"qualityProfile"`
	WorkspaceScope     string                    `json:"workspaceScope"`
	Capabilities       []Capability              `json:"capabilities"`
	BrowserEndpoints   []string                  `json:"browserEndpoints"`
	BrowserTestServer  *BrowserTestServer        `json:"browserTestServer,omitempty"`
	ExecutionMode      string                    `json:"executionMode"`
	SecurityLabel      string                    `json:"securityLabel"`
	ConfigRevision     int                       `json:"configRevision"`
	ConfigHash         string                    `json:"configHash"`
	RegisteredAt       time.Time                 `json:"registeredAt,omitempty"`
	CreatedAt          time.Time                 `json:"createdAt"`
	UpdatedAt          time.Time                 `json:"updatedAt"`
}

// Registered reports whether this project has an explicit repository grant.
func (p Project) Registered() bool {
	return p.RegistrationStatus == ProjectRegistered
}

// Active reports whether the project may receive new work.
func (p Project) Active() bool {
	return p.Status == ProjectActive
}

// Validate checks a loaded project row. Legacy rows are validated as the
// historical minimal record; registered rows must carry a complete binding.
func (p Project) Validate() error {
	if err := ValidateProjectID(p.ID); err != nil {
		return err
	}
	if strings.TrimSpace(p.Name) == "" || len(p.Name) > MaxProjectNameChars {
		return errors.New("project name is required and bounded")
	}
	switch p.RegistrationStatus {
	case ProjectLegacy:
		if p.RepositoryPath != "" || p.RepositoryIdentity != "" {
			return errors.New("legacy project must not carry a repository grant")
		}
		if len(p.Capabilities) != 0 {
			return errors.New("legacy project must not carry capability grants")
		}
		return nil
	case ProjectRegistered:
	default:
		return fmt.Errorf("unknown registrationStatus %q", p.RegistrationStatus)
	}
	switch p.Status {
	case ProjectActive, ProjectPaused:
	default:
		return fmt.Errorf("unknown project status %q", p.Status)
	}
	if err := validateRepositoryPath(p.RepositoryPath); err != nil {
		return err
	}
	if !isLowerHex(p.RepositoryIdentity) || len(p.RepositoryIdentity) != 64 {
		return errors.New("repositoryIdentity must be a sha256 hex digest")
	}
	if _, err := normalizeBaseRefs(p.BaseRefs); err != nil {
		return err
	}
	if _, err := p.QualityProfile.RequiredKinds(); err != nil {
		return err
	}
	if err := validateWorkspaceScope(p.WorkspaceScope); err != nil {
		return err
	}
	caps, err := NormalizeCapabilities(capabilityStrings(p.Capabilities))
	if err != nil {
		return err
	}
	if len(caps) != len(p.Capabilities) {
		return errors.New("capabilities carry duplicates")
	}
	required, _ := RequiredCapabilities(p.QualityProfile)
	if missing := MissingCapabilities(caps, required); len(missing) > 0 {
		return fmt.Errorf("project does not grant required capabilities %v", missing)
	}
	if _, err := normalizeBrowserEndpoints(p.BrowserEndpoints, caps); err != nil {
		return err
	}
	if _, err := normalizeBrowserTestServer(p.BrowserTestServer, caps); err != nil {
		return err
	}
	if p.ExecutionMode != string(ExecTrustedLocal) {
		return errors.New("project executionMode must be trusted-local")
	}
	if p.SecurityLabel != TrustedLocalSecurityLabel {
		return errors.New("project securityLabel must be TRUSTED_LOCAL")
	}
	if p.ConfigRevision < 1 || !isLowerHex(p.ConfigHash) || len(p.ConfigHash) != 64 {
		return errors.New("project config revision/hash is invalid")
	}
	if got := p.ComputeConfigHash(); got != p.ConfigHash {
		return errors.New("project configHash does not match the configuration")
	}
	return nil
}

// ComputeConfigHash hashes the immutable configuration fields of a
// registered project. Identity and timestamps are deliberately excluded.
func (p Project) ComputeConfigHash() string {
	payload := struct {
		ID                string             `json:"id"`
		Name              string             `json:"name"`
		RepositoryPath    string             `json:"repositoryPath"`
		RepositoryID      string             `json:"repositoryIdentity"`
		BaseRefs          []string           `json:"baseRefs"`
		QualityProfile    QualityProfile     `json:"qualityProfile"`
		WorkspaceScope    string             `json:"workspaceScope"`
		Capabilities      []Capability       `json:"capabilities"`
		BrowserEndpoints  []string           `json:"browserEndpoints"`
		BrowserTestServer *BrowserTestServer `json:"browserTestServer,omitempty"`
		ExecutionMode     string             `json:"executionMode"`
		SecurityLabel     string             `json:"securityLabel"`
		ConfigRevision    int                `json:"configRevision"`
	}{
		ID: p.ID, Name: p.Name, RepositoryPath: p.RepositoryPath,
		RepositoryID: p.RepositoryIdentity, BaseRefs: p.BaseRefs,
		QualityProfile: p.QualityProfile, WorkspaceScope: p.WorkspaceScope,
		Capabilities: p.Capabilities, BrowserEndpoints: p.BrowserEndpoints,
		BrowserTestServer: p.BrowserTestServer,
		ExecutionMode:     p.ExecutionMode, SecurityLabel: p.SecurityLabel,
		ConfigRevision: p.ConfigRevision,
	}
	raw, err := json.Marshal(payload)
	if err != nil {
		return ""
	}
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}

// NewProjectParams carries operator registration input. Repository identity
// and base tips are computed by the Core's git adapter before persistence.
type NewProjectParams struct {
	ID                 string
	Name               string
	RepositoryPath     string
	RepositoryIdentity string
	BaseRefs           []string
	QualityProfile     string
	WorkspaceScope     string
	Capabilities       []string
	BrowserEndpoints   []string
	BrowserTestServer  *BrowserTestServer
	ExecutionMode      string
}

// Validate normalizes registration input into a project ready for the store
// to assign identity and registration timestamps.
func (p NewProjectParams) Validate() (Project, error) {
	id := strings.TrimSpace(p.ID)
	if err := ValidateProjectID(id); err != nil {
		return Project{}, err
	}
	name := strings.TrimSpace(p.Name)
	if name == "" || len(name) > MaxProjectNameChars {
		return Project{}, fmt.Errorf("name is required and at most %d characters", MaxProjectNameChars)
	}
	repository := strings.TrimSpace(p.RepositoryPath)
	if err := validateRepositoryPath(repository); err != nil {
		return Project{}, err
	}
	if !isLowerHex(p.RepositoryIdentity) || len(p.RepositoryIdentity) != 64 {
		return Project{}, errors.New("repositoryIdentity must be a sha256 hex digest")
	}
	baseRefs, err := normalizeBaseRefs(p.BaseRefs)
	if err != nil {
		return Project{}, err
	}
	profile := QualityProfile(strings.TrimSpace(p.QualityProfile))
	if _, err := profile.RequiredKinds(); err != nil {
		return Project{}, err
	}
	scope := strings.TrimSpace(p.WorkspaceScope)
	if scope == "" {
		scope = "."
	}
	if err := validateWorkspaceScope(scope); err != nil {
		return Project{}, err
	}
	caps, err := NormalizeCapabilities(p.Capabilities)
	if err != nil {
		return Project{}, err
	}
	if len(caps) != len(p.Capabilities) {
		return Project{}, errors.New("capabilities carry duplicates")
	}
	required, _ := RequiredCapabilities(profile)
	if missing := MissingCapabilities(caps, required); len(missing) > 0 {
		return Project{}, fmt.Errorf("capabilities must include %v for quality profile %q", missing, profile)
	}
	endpoints, err := normalizeBrowserEndpoints(p.BrowserEndpoints, caps)
	if err != nil {
		return Project{}, err
	}
	testServer, err := normalizeBrowserTestServer(p.BrowserTestServer, caps)
	if err != nil {
		return Project{}, err
	}
	if ContainsCapability(caps, CapBrowser) && len(endpoints) == 0 {
		return Project{}, errors.New("browser capability requires an authorized local endpoint")
	}
	mode := strings.TrimSpace(p.ExecutionMode)
	if mode == "" {
		mode = string(ExecTrustedLocal)
	}
	if mode != string(ExecTrustedLocal) {
		return Project{}, fmt.Errorf("project executionMode must be %q; no isolated-to-local fallback exists", ExecTrustedLocal)
	}
	project := Project{
		ID:                 id,
		Name:               name,
		RegistrationStatus: ProjectRegistered,
		Status:             ProjectActive,
		RepositoryPath:     repository,
		RepositoryIdentity: p.RepositoryIdentity,
		BaseRefs:           baseRefs,
		QualityProfile:     profile,
		WorkspaceScope:     scope,
		Capabilities:       caps,
		BrowserEndpoints:   endpoints,
		BrowserTestServer:  testServer,
		ExecutionMode:      mode,
		SecurityLabel:      TrustedLocalSecurityLabel,
		ConfigRevision:     1,
	}
	project.ConfigHash = project.ComputeConfigHash()
	if err := project.Validate(); err != nil {
		return Project{}, err
	}
	return project, nil
}

// ProjectUpdateParams is the mutable configuration surface. Repository path
// and identity are immutable once a project owns a frozen spec.
type ProjectUpdateParams struct {
	Name              *string
	Status            *string
	BaseRefs          *[]string
	QualityProfile    *string
	WorkspaceScope    *string
	Capabilities      *[]string
	BrowserEndpoints  *[]string
	BrowserTestServer **BrowserTestServer
	RepositoryPath    *string
}

// Validate applies an update onto current and returns the new configuration
// plus whether any normative field changed. The store owns revision and
// timestamps.
func (p ProjectUpdateParams) Apply(current Project) (Project, bool, error) {
	if !current.Registered() {
		return Project{}, false, errors.New("legacy projects cannot be reconfigured; register a new project")
	}
	next := current
	changed := false
	assign := func(dst *string, src *string) {
		if src == nil {
			return
		}
		value := strings.TrimSpace(*src)
		if *dst != value {
			*dst = value
			changed = true
		}
	}
	if p.Name != nil {
		assign(&next.Name, p.Name)
	}
	if p.Status != nil {
		assign((*string)(&next.Status), p.Status)
	}
	if p.RepositoryPath != nil {
		assign(&next.RepositoryPath, p.RepositoryPath)
	}
	if p.WorkspaceScope != nil {
		assign(&next.WorkspaceScope, p.WorkspaceScope)
	}
	if p.QualityProfile != nil {
		var profile QualityProfile
		assign((*string)(&profile), p.QualityProfile)
		if profile != next.QualityProfile {
			next.QualityProfile = profile
			changed = true
		}
	}
	if p.BaseRefs != nil {
		refs, err := normalizeBaseRefs(*p.BaseRefs)
		if err != nil {
			return Project{}, false, err
		}
		if strings.Join(refs, "\x00") != strings.Join(next.BaseRefs, "\x00") {
			next.BaseRefs = refs
			changed = true
		}
	}
	if p.Capabilities != nil {
		caps, err := NormalizeCapabilities(*p.Capabilities)
		if err != nil {
			return Project{}, false, err
		}
		if strings.Join(capabilityStrings(caps), "\x00") != strings.Join(capabilityStrings(next.Capabilities), "\x00") {
			next.Capabilities = caps
			changed = true
		}
	}
	if p.BrowserEndpoints != nil {
		endpoints, err := normalizeBrowserEndpoints(*p.BrowserEndpoints, next.Capabilities)
		if err != nil {
			return Project{}, false, err
		}
		if strings.Join(endpoints, "\x00") != strings.Join(next.BrowserEndpoints, "\x00") {
			next.BrowserEndpoints = endpoints
			changed = true
		}
	}
	if p.BrowserTestServer != nil {
		testServer, err := normalizeBrowserTestServer(*p.BrowserTestServer, next.Capabilities)
		if err != nil {
			return Project{}, false, err
		}
		if !browserTestServerEqual(testServer, next.BrowserTestServer) {
			next.BrowserTestServer = testServer
			changed = true
		}
	}
	switch next.Status {
	case ProjectActive, ProjectPaused:
	default:
		return Project{}, false, fmt.Errorf("unknown project status %q", next.Status)
	}
	if strings.TrimSpace(next.Name) == "" || len(next.Name) > MaxProjectNameChars {
		return Project{}, false, errors.New("project name is required and bounded")
	}
	if err := validateWorkspaceScope(next.WorkspaceScope); err != nil {
		return Project{}, false, err
	}
	if _, err := next.QualityProfile.RequiredKinds(); err != nil {
		return Project{}, false, err
	}
	required, _ := RequiredCapabilities(next.QualityProfile)
	if missing := MissingCapabilities(next.Capabilities, required); len(missing) > 0 {
		return Project{}, false, fmt.Errorf("capabilities must include %v for quality profile %q", missing, next.QualityProfile)
	}
	if err := validateRepositoryPath(next.RepositoryPath); err != nil {
		return Project{}, false, err
	}
	if !changed {
		return next, false, nil
	}
	next.ConfigRevision = current.ConfigRevision + 1
	next.ConfigHash = next.ComputeConfigHash()
	if err := next.Validate(); err != nil {
		return Project{}, false, err
	}
	return next, true, nil
}

// SpecsWithinWorkspaceScope rejects allowed paths outside the project's
// declared workspace scope.
func SpecsWithinWorkspaceScope(scope string, allowed []string) error {
	if scope == "" || scope == "." {
		return nil
	}
	for _, entry := range allowed {
		if entry != scope && !strings.HasPrefix(entry, scope+"/") {
			return fmt.Errorf("allowed path %q is outside workspace scope %q", entry, scope)
		}
	}
	return nil
}

func capabilityStrings(caps []Capability) []string {
	out := make([]string, len(caps))
	for i, capability := range caps {
		out[i] = string(capability)
	}
	return out
}

func normalizeBaseRefs(raw []string) ([]string, error) {
	if len(raw) == 0 {
		return nil, errors.New("baseRefs must contain at least one authorized ref")
	}
	if len(raw) > MaxBaseRefs {
		return nil, fmt.Errorf("baseRefs carries at most %d entries", MaxBaseRefs)
	}
	seen := map[string]bool{}
	out := make([]string, 0, len(raw))
	for _, entry := range raw {
		ref := strings.TrimSpace(entry)
		if len(ref) == 0 || len(ref) > MaxBaseRefChars {
			return nil, errors.New("base ref is empty or too long")
		}
		if strings.ContainsAny(ref, "\x00\n\r ~^:?*[\\") || strings.HasPrefix(ref, "-") ||
			strings.Contains(ref, "..") || strings.HasSuffix(ref, "/") || strings.HasPrefix(ref, "/") {
			return nil, fmt.Errorf("base ref %q is not a safe Git ref", ref)
		}
		if seen[ref] {
			continue
		}
		seen[ref] = true
		out = append(out, ref)
	}
	return out, nil
}

func validateWorkspaceScope(scope string) error {
	if scope == "" || scope == "." {
		return nil
	}
	if len(scope) > MaxWorkspaceScopeChars {
		return errors.New("workspaceScope is too long")
	}
	if err := validateRelativePath(scope); err != nil {
		return fmt.Errorf("workspaceScope: %w", err)
	}
	return nil
}

func normalizeBrowserTestServer(raw *BrowserTestServer, caps []Capability) (*BrowserTestServer, error) {
	if raw == nil {
		return nil, nil
	}
	if !ContainsCapability(caps, CapBrowser) {
		return nil, errors.New("browserTestServer requires the browser capability")
	}
	command, err := normalizeCommand(raw.Command)
	if err != nil {
		return nil, fmt.Errorf("browserTestServer.command: %w", err)
	}
	readyPath := strings.TrimSpace(raw.ReadyPath)
	if readyPath == "" {
		readyPath = "/"
	}
	if !strings.HasPrefix(readyPath, "/") || strings.Contains(readyPath, "..") ||
		strings.ContainsAny(readyPath, "\x00\n\r") || len(readyPath) > MaxAllowedPathChars {
		return nil, errors.New("browserTestServer.readyPath must be a safe absolute URL path")
	}
	timeout := raw.StartupTimeoutSecs
	if timeout == 0 {
		timeout = 30
	}
	if timeout < 5 || timeout > MaxCheckTimeoutSecs {
		return nil, fmt.Errorf("browserTestServer.startupTimeoutSecs must be 5..%d", MaxCheckTimeoutSecs)
	}
	return &BrowserTestServer{Command: command, ReadyPath: readyPath, StartupTimeoutSecs: timeout}, nil
}

func browserTestServerEqual(left, right *BrowserTestServer) bool {
	if left == nil || right == nil {
		return left == nil && right == nil
	}
	return strings.Join(left.Command, "\x00") == strings.Join(right.Command, "\x00") &&
		left.ReadyPath == right.ReadyPath && left.StartupTimeoutSecs == right.StartupTimeoutSecs
}

func normalizeBrowserEndpoints(raw []string, caps []Capability) ([]string, error) {
	hasBrowser := ContainsCapability(caps, CapBrowser)
	if !hasBrowser {
		if len(raw) > 0 {
			return nil, errors.New("browserEndpoints require the browser capability")
		}
		return nil, nil
	}
	if len(raw) == 0 {
		return nil, nil
	}
	if len(raw) > MaxBrowserEndpoints {
		return nil, fmt.Errorf("browserEndpoints carries at most %d entries", MaxBrowserEndpoints)
	}
	seen := map[string]bool{}
	out := make([]string, 0, len(raw))
	for _, entry := range raw {
		endpoint := strings.TrimSpace(entry)
		if len(endpoint) == 0 || len(endpoint) > MaxBrowserEndpointChars {
			return nil, errors.New("browser endpoint is empty or too long")
		}
		parsed, err := url.Parse(endpoint)
		if err != nil || parsed.Scheme != "http" || parsed.User != nil || parsed.Host == "" {
			return nil, fmt.Errorf("browser endpoint %q must be a plain http URL", endpoint)
		}
		host := parsed.Hostname()
		if host != "127.0.0.1" && host != "localhost" && host != "::1" {
			return nil, fmt.Errorf("browser endpoint %q must target a loopback host", endpoint)
		}
		if seen[endpoint] {
			continue
		}
		seen[endpoint] = true
		out = append(out, endpoint)
	}
	return out, nil
}

// BrowserEndpointAllowed reports whether target is inside the authorized
// loopback endpoint prefix. This is an instruction/control-plane check, not
// an OS-enforced browser boundary.
func BrowserEndpointAllowed(target string, endpoints []string) bool {
	parsed, err := url.Parse(strings.TrimSpace(target))
	if err != nil || parsed.Scheme != "http" || parsed.Host == "" {
		return false
	}
	host := parsed.Hostname()
	if host != "127.0.0.1" && host != "localhost" && host != "::1" {
		return false
	}
	for _, allowed := range endpoints {
		prefix := strings.TrimRight(allowed, "/")
		if target == prefix || strings.HasPrefix(target, prefix+"/") {
			return true
		}
	}
	return false
}
