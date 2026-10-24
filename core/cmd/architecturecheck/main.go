// Command architecturecheck validates the canonical capability registry and
// the roadmap generated from it. It intentionally validates repository
// references as paths; it does not claim to understand architectural prose.
package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strings"

	"gopkg.in/yaml.v3"
)

type registry struct {
	SchemaVersion int          `yaml:"schema_version"`
	Milestones    []milestone  `yaml:"milestones"`
	Capabilities  []capability `yaml:"capabilities"`
}

type milestone struct {
	ID                      string   `yaml:"id"`
	Title                   string   `yaml:"title"`
	DecisionStatus          string   `yaml:"decision_status"`
	Dependencies            []string `yaml:"dependencies"`
	Deliverables            []string `yaml:"deliverables"`
	CapabilityIDs           []string `yaml:"capability_ids"`
	Exclusions              []string `yaml:"exclusions"`
	Risks                   []string `yaml:"risks"`
	DeterministicAcceptance []string `yaml:"deterministic_acceptance"`
	RealAcceptance          []string `yaml:"real_acceptance"`
	OwnerDecisions          []string `yaml:"owner_decisions"`
}

type capability struct {
	ID               string       `yaml:"id"`
	Name             string       `yaml:"name"`
	Category         string       `yaml:"category"`
	Purpose          string       `yaml:"purpose"`
	ProductObjective string       `yaml:"product_objective"`
	DecisionStatus   string       `yaml:"decision_status"`
	Implementation   string       `yaml:"implementation_status"`
	Verification     verification `yaml:"verification"`
	Owner            string       `yaml:"owner"`
	Dependencies     []string     `yaml:"dependencies"`
	TargetMilestone  string       `yaml:"target_milestone"`
	Refs             references   `yaml:"refs"`
	Acceptance       acceptance   `yaml:"acceptance"`
	Evidence         evidence     `yaml:"evidence"`
	Limitations      []string     `yaml:"limitations"`
	Blockers         []string     `yaml:"blockers"`
}

type verification struct {
	Deterministic string `yaml:"deterministic"`
	LiveAgent     string `yaml:"live_agent"`
	UAT           string `yaml:"uat"`
	Deployment    string `yaml:"deployment"`
	Isolation     string `yaml:"isolation"`
}

type references struct {
	ADR      []string `yaml:"adr"`
	Contract []string `yaml:"contract"`
	Source   []string `yaml:"source"`
	Test     []string `yaml:"test"`
}

type acceptance struct {
	Deterministic string `yaml:"deterministic"`
	Real          string `yaml:"real"`
}

type evidence struct {
	Deterministic []string `yaml:"deterministic"`
	LiveAgent     []string `yaml:"live_agent"`
	UAT           []string `yaml:"uat"`
	Deployment    []string `yaml:"deployment"`
	Isolation     []string `yaml:"isolation"`
}

var (
	idPattern      = regexp.MustCompile(`^[A-Z][A-Z0-9]*(?:-[A-Z0-9]+)*$`)
	fullSHAPattern = regexp.MustCompile(`^[0-9a-fA-F]{40}$`)
)

type liveCertificationManifest struct {
	Level            string `json:"level"`
	Status           string `json:"status"`
	ImplementationSH string `json:"implementation_sha"`
	CandidateSH      string `json:"candidate_sha"`
}

func main() {
	root := flag.String("root", ".", "repository root")
	registryPath := flag.String("registry", "docs/product/capabilities.yaml", "capability registry path")
	roadmapPath := flag.String("roadmap", "docs/roadmap/ROADMAP.md", "generated roadmap path")
	writeRoadmap := flag.Bool("write-roadmap", false, "write the roadmap generated from the registry")
	baseRef := flag.String("base-ref", "", "git base ref; enforce stable IDs and accepted ADR history")
	flag.Parse()

	if err := run(*root, *registryPath, *roadmapPath, *writeRoadmap, *baseRef); err != nil {
		fmt.Fprintln(os.Stderr, "architecturecheck:", err)
		os.Exit(1)
	}
}

func run(root, registryPath, roadmapPath string, writeRoadmap bool, baseRef string) error {
	root, err := filepath.Abs(root)
	if err != nil {
		return err
	}
	raw, err := os.ReadFile(filepath.Join(root, registryPath))
	if err != nil {
		return fmt.Errorf("read registry: %w", err)
	}
	var data registry
	decoder := yaml.NewDecoder(bytes.NewReader(raw))
	decoder.KnownFields(true)
	if err := decoder.Decode(&data); err != nil {
		return fmt.Errorf("parse registry: %w", err)
	}
	if err := validate(root, data); err != nil {
		return err
	}
	if baseRef != "" {
		if err := validateStableIDs(root, baseRef, data); err != nil {
			return err
		}
		if err := validateAcceptedADRHistory(root, baseRef); err != nil {
			return err
		}
	}
	generated := renderRoadmap(data)
	roadmapFile := filepath.Join(root, roadmapPath)
	if writeRoadmap {
		if err := os.MkdirAll(filepath.Dir(roadmapFile), 0o755); err != nil {
			return err
		}
		if err := os.WriteFile(roadmapFile, []byte(generated), 0o644); err != nil {
			return err
		}
	} else {
		actual, err := os.ReadFile(roadmapFile)
		if err != nil {
			return fmt.Errorf("read roadmap: %w", err)
		}
		if !bytes.Equal(actual, []byte(generated)) {
			return fmt.Errorf("roadmap is stale; run scripts/check-architecture-consistency --write-roadmap")
		}
	}
	return nil
}

var amendmentADRID = regexp.MustCompile(`(?m)^\s*(?:[-*]\s*)?(?:\*\*)?Amended by:\s*(?:\*\*)?\s*(ADR-\d{4})`)

var acceptedADRStatus = regexp.MustCompile(`(?im)^\s*(?:[-*]\s*)?(?:\*\*)?status(?:\*\*)?:?\s*(?:\*\*)?:?\s*["']?accepted(?:\b|[ \t(])`)

func validateAcceptedADRHistory(root, baseRef string) error {
	cmd := exec.Command("git", "diff", "--name-status", "--find-renames", baseRef, "HEAD", "--", "docs/architecture/decisions/ADR-*.md")
	cmd.Dir = root
	output, err := cmd.Output()
	if err != nil {
		return fmt.Errorf("inspect ADR history against %s: %w", baseRef, err)
	}
	for _, line := range strings.Split(strings.TrimSpace(string(output)), "\n") {
		if line == "" {
			continue
		}
		fields := strings.Split(line, "\t")
		if len(fields) < 2 {
			return fmt.Errorf("parse ADR history diff record %q", line)
		}
		status := fields[0]
		if strings.HasPrefix(status, "A") {
			continue
		}
		basePath := fields[1]
		contentCmd := exec.Command("git", "show", baseRef+":"+basePath)
		contentCmd.Dir = root
		baseContent, err := contentCmd.Output()
		if err != nil {
			return fmt.Errorf("read base ADR %s: %w", basePath, err)
		}
		if acceptedADRStatus.Match(baseContent) {
			current, err := os.ReadFile(filepath.Join(root, basePath))
			if err != nil {
				return fmt.Errorf("read current ADR %s: %w", basePath, err)
			}
			amendment := amendmentADRID.FindSubmatch(current)
			if len(amendment) != 2 {
				return fmt.Errorf("accepted ADR %s was changed in place; add a superseding ADR and preserve accepted history", basePath)
			}
			matches, err := filepath.Glob(filepath.Join(root, "docs/architecture/decisions", string(amendment[1])+"-*.md"))
			if err != nil || len(matches) != 1 {
				return fmt.Errorf("accepted ADR %s amendment %s is missing or ambiguous", basePath, amendment[1])
			}
			amendmentContent, err := os.ReadFile(matches[0])
			if err != nil || !acceptedADRStatus.Match(amendmentContent) {
				return fmt.Errorf("accepted ADR %s amendment %s is not accepted", basePath, amendment[1])
			}
		}
	}
	return nil
}

func validateStableIDs(root, baseRef string, current registry) error {
	cmd := exec.Command("git", "show", baseRef+":docs/product/capabilities.yaml")
	cmd.Dir = root
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	previousBytes, err := cmd.Output()
	if err != nil {
		if strings.Contains(stderr.String(), "does not exist in") || strings.Contains(stderr.String(), "not in '") {
			return nil
		}
		return fmt.Errorf("read base capability registry: %w: %s", err, strings.TrimSpace(stderr.String()))
	}
	var previous registry
	if err := yaml.Unmarshal(previousBytes, &previous); err != nil {
		return fmt.Errorf("parse base capability registry: %w", err)
	}
	present := map[string]bool{}
	for _, capability := range current.Capabilities {
		present[capability.ID] = true
	}
	for _, capability := range previous.Capabilities {
		if !present[capability.ID] {
			return fmt.Errorf("capability ID %s was removed; IDs are stable and must be retained", capability.ID)
		}
	}
	return nil
}

func validate(root string, data registry) error {
	var problems []string
	if data.SchemaVersion != 1 {
		problems = append(problems, "schema_version must be 1")
	}
	capabilities := map[string]bool{}
	milestones := map[string]bool{}
	milestoneOrder := map[string]int{}
	for index, m := range data.Milestones {
		if !validID(m.ID) {
			problems = append(problems, "invalid milestone id: "+m.ID)
		}
		if milestones[m.ID] {
			problems = append(problems, "duplicate milestone id: "+m.ID)
		}
		milestones[m.ID] = true
		milestoneOrder[m.ID] = index
		if m.Title == "" || !validDecision(m.DecisionStatus) {
			problems = append(problems, "milestone "+m.ID+" requires title and valid decision_status")
		}
		for field, values := range map[string][]string{"deliverables": m.Deliverables, "exclusions": m.Exclusions, "risks": m.Risks, "deterministic_acceptance": m.DeterministicAcceptance, "real_acceptance": m.RealAcceptance, "owner_decisions": m.OwnerDecisions} {
			if len(values) == 0 {
				problems = append(problems, "milestone "+m.ID+" has empty "+field)
			}
		}
	}
	for _, c := range data.Capabilities {
		if !validID(c.ID) {
			problems = append(problems, "invalid capability id: "+c.ID)
		}
		if capabilities[c.ID] {
			problems = append(problems, "duplicate capability id: "+c.ID)
		}
		capabilities[c.ID] = true
	}
	capabilityByID := make(map[string]capability, len(data.Capabilities))
	for _, c := range data.Capabilities {
		capabilityByID[c.ID] = c
	}
	membership := map[string][]string{}
	for _, m := range data.Milestones {
		for _, id := range m.CapabilityIDs {
			membership[id] = append(membership[id], m.ID)
		}
	}
	for _, c := range data.Capabilities {
		prefix := "capability " + c.ID + " "
		if c.Name == "" || c.Category == "" || c.Purpose == "" || c.ProductObjective == "" || c.Owner == "" {
			problems = append(problems, prefix+"is missing required descriptive fields")
		}
		if !validDecision(c.DecisionStatus) {
			problems = append(problems, prefix+"has invalid decision_status")
		}
		if c.Implementation != "implemented" && c.Implementation != "in_progress" && c.Implementation != "not_implemented" {
			problems = append(problems, prefix+"has invalid implementation_status")
		}
		if !milestones[c.TargetMilestone] {
			problems = append(problems, prefix+"references unknown target milestone "+c.TargetMilestone)
		}
		if len(membership[c.ID]) != 1 {
			problems = append(problems, prefix+"must appear exactly once in milestone capability_ids")
		} else if membership[c.ID][0] != c.TargetMilestone {
			problems = append(problems, prefix+"target_milestone does not match milestone capability_ids membership")
		}
		for _, dep := range c.Dependencies {
			if !capabilities[dep] {
				problems = append(problems, prefix+"references unknown dependency "+dep)
				continue
			}
			dependency := capabilityByID[dep]
			if targetIndex, targetOK := milestoneOrder[c.TargetMilestone]; targetOK {
				if dependencyIndex, dependencyOK := milestoneOrder[dependency.TargetMilestone]; dependencyOK && dependencyIndex > targetIndex {
					problems = append(problems, prefix+"depends on "+dep+" scheduled in later milestone "+dependency.TargetMilestone)
				}
			}
		}
		for _, ref := range append(append(append([]string{}, c.Refs.ADR...), c.Refs.Contract...), append(c.Refs.Source, c.Refs.Test...)...) {
			if err := checkRef(root, ref); err != nil {
				problems = append(problems, prefix+err.Error())
			}
		}
		for _, ev := range append(append(append(append([]string{}, c.Evidence.Deterministic...), c.Evidence.LiveAgent...), append(c.Evidence.UAT, c.Evidence.Deployment...)...), c.Evidence.Isolation...) {
			if err := checkRef(root, ev); err != nil {
				problems = append(problems, prefix+"evidence "+err.Error())
			}
		}
		if c.Implementation == "implemented" && (len(c.Refs.Source) == 0 || len(c.Refs.Test) == 0) {
			problems = append(problems, prefix+"implemented requires source and test references")
		}
		if c.Implementation == "not_implemented" && anyVerified(c.Verification) {
			problems = append(problems, prefix+"not_implemented cannot claim verified or accepted evidence")
		}
		if c.DecisionStatus == "rejected" && c.Implementation == "implemented" {
			problems = append(problems, prefix+"rejected capability cannot be marked implemented")
		}
		if c.Acceptance.Deterministic == "" || c.Acceptance.Real == "" {
			problems = append(problems, prefix+"requires deterministic and real acceptance criteria")
		}
		if !validVerification(c.Verification.Deterministic, "verified", "not_tested", "blocked", "failed") || !validVerification(c.Verification.LiveAgent, "verified", "not_tested", "blocked", "failed") || !validVerification(c.Verification.UAT, "accepted", "not_tested", "blocked", "failed") || !validVerification(c.Verification.Deployment, "verified", "not_tested", "blocked", "failed") || !validVerification(c.Verification.Isolation, "verified", "not_tested", "blocked", "failed") {
			problems = append(problems, prefix+"has invalid verification state")
		}
		if c.Verification.Deterministic == "verified" && len(c.Evidence.Deterministic) == 0 {
			problems = append(problems, prefix+"deterministic verified without evidence")
		} else if c.Verification.Deterministic == "verified" {
			if err := validateEvidenceDimension(root, "deterministic", c.Evidence.Deterministic); err != nil {
				problems = append(problems, prefix+err.Error())
			}
		}
		if c.Verification.LiveAgent == "verified" {
			if err := validateLiveEvidence(root, c.Evidence.LiveAgent); err != nil {
				problems = append(problems, prefix+err.Error())
			}
		}
		if c.Verification.UAT == "accepted" {
			if err := validateEvidenceDimension(root, "uat", c.Evidence.UAT); err != nil {
				problems = append(problems, prefix+err.Error())
			}
		}
		if c.Verification.Deployment == "verified" {
			if err := validateEvidenceDimension(root, "deployment", c.Evidence.Deployment); err != nil {
				problems = append(problems, prefix+err.Error())
			}
		}
		if c.Verification.Isolation == "verified" {
			if err := validateEvidenceDimension(root, "isolation", c.Evidence.Isolation); err != nil {
				problems = append(problems, prefix+err.Error())
			}
		}
		for _, dimension := range evidenceDimensions(c.Verification, c.Evidence) {
			if dimension.state == "not_tested" && len(dimension.records) > 0 {
				problems = append(problems, prefix+dimension.name+" has evidence while marked not_tested")
			}
		}
	}
	for _, m := range data.Milestones {
		for _, dep := range m.Dependencies {
			if !milestones[dep] {
				problems = append(problems, "milestone "+m.ID+" references unknown dependency "+dep)
				continue
			}
			if milestoneOrder[dep] >= milestoneOrder[m.ID] {
				problems = append(problems, "milestone "+m.ID+" depends on "+dep+" which is not scheduled earlier")
			}
		}
		for _, id := range m.CapabilityIDs {
			if !capabilities[id] {
				problems = append(problems, "milestone "+m.ID+" references unknown capability "+id)
			}
		}
	}
	if cycle := dependencyCycle(milestoneIDs(data.Milestones), milestoneEdges(data.Milestones)); len(cycle) > 0 {
		problems = append(problems, "milestone dependency cycle: "+strings.Join(cycle, " -> "))
	}
	if cycle := dependencyCycle(capabilityIDs(data.Capabilities), capabilityEdges(data.Capabilities)); len(cycle) > 0 {
		problems = append(problems, "capability dependency cycle: "+strings.Join(cycle, " -> "))
	}
	if len(problems) > 0 {
		sort.Strings(problems)
		return fmt.Errorf("validation failed:\n - %s", strings.Join(problems, "\n - "))
	}
	return nil
}

func milestoneIDs(values []milestone) []string {
	ids := make([]string, 0, len(values))
	for _, value := range values {
		ids = append(ids, value.ID)
	}
	return ids
}

func capabilityIDs(values []capability) []string {
	ids := make([]string, 0, len(values))
	for _, value := range values {
		ids = append(ids, value.ID)
	}
	return ids
}

func milestoneEdges(values []milestone) map[string][]string {
	edges := make(map[string][]string, len(values))
	for _, value := range values {
		edges[value.ID] = value.Dependencies
	}
	return edges
}

func capabilityEdges(values []capability) map[string][]string {
	edges := make(map[string][]string, len(values))
	for _, value := range values {
		edges[value.ID] = value.Dependencies
	}
	return edges
}

// dependencyCycle returns a deterministic cycle path, including the repeated
// start node, so diagnostics identify the dependency chain that must be fixed.
func dependencyCycle(nodes []string, edges map[string][]string) []string {
	const (
		unvisited = iota
		visiting
		visited
	)
	state := make(map[string]int, len(nodes))
	stack := make([]string, 0, len(nodes))
	position := make(map[string]int, len(nodes))
	var cycle []string
	var visit func(string) bool
	visit = func(node string) bool {
		state[node] = visiting
		position[node] = len(stack)
		stack = append(stack, node)
		dependencies := append([]string(nil), edges[node]...)
		sort.Strings(dependencies)
		for _, dependency := range dependencies {
			if state[dependency] == visiting {
				cycle = append(append([]string(nil), stack[position[dependency]:]...), dependency)
				return true
			}
			if state[dependency] == unvisited && visit(dependency) {
				return true
			}
		}
		stack = stack[:len(stack)-1]
		delete(position, node)
		state[node] = visited
		return false
	}
	sortedNodes := append([]string(nil), nodes...)
	sort.Strings(sortedNodes)
	for _, node := range sortedNodes {
		if state[node] == unvisited && visit(node) {
			return cycle
		}
	}
	return nil
}

func validID(s string) bool { return idPattern.MatchString(s) }
func anyVerified(v verification) bool {
	return v.Deterministic == "verified" || v.LiveAgent == "verified" || v.UAT == "accepted" || v.Deployment == "verified" || v.Isolation == "verified"
}

func validateLiveEvidence(root string, references []string) error {
	if len(references) == 0 {
		return fmt.Errorf("live_agent verified without evidence")
	}
	for _, ref := range references {
		if strings.Contains(ref, "#") || !strings.HasPrefix(ref, "docs/evidence/") || strings.Count(ref, "/") != 2 || !strings.HasSuffix(ref, "certification-manifest.json") {
			return fmt.Errorf("live_agent evidence must reference docs/evidence/*certification-manifest.json")
		}
		content, err := os.ReadFile(filepath.Join(root, ref))
		if err != nil {
			return fmt.Errorf("live_agent certification manifest %q: %w", ref, err)
		}
		var manifest liveCertificationManifest
		if err := json.Unmarshal(content, &manifest); err != nil {
			return fmt.Errorf("live_agent certification manifest %q is invalid JSON: %w", ref, err)
		}
		if manifest.Level != "REAL_PASEO_E2E_VERIFIED" || manifest.Status != "verified" {
			return fmt.Errorf("live_agent certification manifest %q must have level REAL_PASEO_E2E_VERIFIED and status verified", ref)
		}
		if !fullSHAPattern.MatchString(manifest.ImplementationSH) || !fullSHAPattern.MatchString(manifest.CandidateSH) {
			return fmt.Errorf("live_agent certification manifest %q requires full implementation_sha and candidate_sha values", ref)
		}
	}
	return nil
}

type evidenceDimension struct {
	name    string
	state   string
	records []string
}

func evidenceDimensions(v verification, e evidence) []evidenceDimension {
	return []evidenceDimension{
		{name: "deterministic", state: v.Deterministic, records: e.Deterministic},
		{name: "live_agent", state: v.LiveAgent, records: e.LiveAgent},
		{name: "uat", state: v.UAT, records: e.UAT},
		{name: "deployment", state: v.Deployment, records: e.Deployment},
		{name: "isolation", state: v.Isolation, records: e.Isolation},
	}
}

func validDecision(s string) bool {
	return s == "approved" || s == "proposed" || s == "deferred" || s == "rejected"
}

func validVerification(s string, values ...string) bool {
	for _, v := range values {
		if s == v {
			return true
		}
	}
	return false
}

func checkRef(root, ref string) error {
	path := strings.SplitN(ref, "#", 2)[0]
	if path == "" {
		return fmt.Errorf("has empty reference")
	}
	if filepath.IsAbs(path) {
		return fmt.Errorf("reference %q is absolute; repository-relative paths are required", path)
	}
	for _, segment := range strings.Split(filepath.ToSlash(path), "/") {
		if segment == ".." {
			return fmt.Errorf("reference %q contains forbidden parent traversal", path)
		}
	}
	cleanPath := filepath.Clean(path)
	if cleanPath == ".." || strings.HasPrefix(cleanPath, ".."+string(filepath.Separator)) {
		return fmt.Errorf("reference %q escapes repository root", path)
	}
	rootReal, err := filepath.EvalSymlinks(root)
	if err != nil {
		return fmt.Errorf("resolve repository root: %w", err)
	}
	targetReal, err := filepath.EvalSymlinks(filepath.Join(rootReal, cleanPath))
	if err != nil {
		return fmt.Errorf("references missing file %q", path)
	}
	rel, err := filepath.Rel(rootReal, targetReal)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return fmt.Errorf("reference %q resolves outside repository root", path)
	}
	info, err := os.Stat(targetReal)
	if err != nil || info.IsDir() {
		return fmt.Errorf("references missing file %q", path)
	}
	return nil
}

func validateEvidenceDimension(root, dimension string, references []string) error {
	if len(references) == 0 {
		return fmt.Errorf("%s verified without evidence", dimension)
	}
	for _, ref := range references {
		path := strings.SplitN(ref, "#", 2)[0]
		if !strings.HasPrefix(path, "docs/evidence/") || !strings.HasSuffix(strings.ToLower(path), ".json") {
			return fmt.Errorf("%s evidence must reference a JSON file under docs/evidence", dimension)
		}
		content, err := os.ReadFile(filepath.Join(root, path))
		if err != nil {
			return fmt.Errorf("%s evidence %q: %w", dimension, path, err)
		}
		var record map[string]any
		if err := json.Unmarshal(content, &record); err != nil {
			return fmt.Errorf("%s evidence %q is invalid JSON: %w", dimension, path, err)
		}
		switch dimension {
		case "deterministic":
			if !hasRecordedPass(record) {
				return fmt.Errorf("deterministic evidence %q has no recorded passing result", path)
			}
		case "uat":
			if !hasEvidenceLevel(record, "PRODUCT_OWNER_UAT_VERIFIED") {
				return fmt.Errorf("UAT evidence %q must be a PRODUCT_OWNER_UAT_VERIFIED attestation with verified status", path)
			}
		case "deployment":
			if !hasEvidenceLevel(record, "DEPLOYMENT_VERIFIED") {
				return fmt.Errorf("deployment evidence %q must have DEPLOYMENT_VERIFIED level and verified status", path)
			}
		case "isolation":
			if !hasEvidenceLevel(record, "SEC_01_ISOLATION_VERIFIED") {
				return fmt.Errorf("isolation evidence %q must have SEC_01_ISOLATION_VERIFIED level and verified status", path)
			}
		}
	}
	return nil
}

func hasEvidenceLevel(record map[string]any, level string) bool {
	return record["evidence_level"] == level && strings.EqualFold(asString(record["status"]), "verified")
}

func hasRecordedPass(record map[string]any) bool {
	if result := strings.ToUpper(asString(record["result"])); strings.HasPrefix(result, "PASS") {
		return true
	}
	checks, ok := record["checks"].([]any)
	if !ok || len(checks) == 0 {
		return false
	}
	for _, item := range checks {
		check, ok := item.(map[string]any)
		if !ok || check["pass"] != true {
			return false
		}
	}
	return true
}

func asString(value any) string {
	str, _ := value.(string)
	return str
}

func renderRoadmap(data registry) string {
	var out strings.Builder
	capabilities := make(map[string]capability, len(data.Capabilities))
	for _, c := range data.Capabilities {
		capabilities[c.ID] = c
	}
	out.WriteString("# Software Factory Roadmap\n\nThis page is generated from `docs/product/capabilities.yaml`. Edit the registry and run `scripts/check-architecture-consistency --write-roadmap`.\n")
	for _, m := range data.Milestones {
		fmt.Fprintf(&out, "\n## %s — %s\n\nDecision: `%s`\n\n", m.ID, m.Title, m.DecisionStatus)
		writeList(&out, "Dependencies", m.Dependencies)
		writeCapabilities(&out, m.CapabilityIDs, capabilities)
		writeList(&out, "Deliverables", m.Deliverables)
		writeList(&out, "Exclusions", m.Exclusions)
		writeList(&out, "Risks", m.Risks)
		writeList(&out, "Deterministic acceptance", m.DeterministicAcceptance)
		writeList(&out, "Real acceptance", m.RealAcceptance)
		writeList(&out, "Product Owner decisions", m.OwnerDecisions)
	}
	return strings.TrimRight(out.String(), "\n") + "\n"
}

func writeCapabilities(out *strings.Builder, ids []string, capabilities map[string]capability) {
	fmt.Fprintf(out, "### Capabilities\n\n")
	if len(ids) == 0 {
		out.WriteString("- None specified.\n\n")
		return
	}
	for _, id := range ids {
		c, ok := capabilities[id]
		if !ok {
			fmt.Fprintf(out, "- %s — [missing capability definition]\n", id)
			continue
		}
		fmt.Fprintf(out, "- %s — %s (%s, %s)\n", c.ID, c.Name, c.DecisionStatus, c.Implementation)
	}
	out.WriteString("\n")
}

func writeList(out *strings.Builder, title string, values []string) {
	fmt.Fprintf(out, "### %s\n\n", title)
	if len(values) == 0 {
		out.WriteString("- None specified.\n\n")
		return
	}
	for _, v := range values {
		fmt.Fprintf(out, "- %s\n", v)
	}
	out.WriteString("\n")
}
