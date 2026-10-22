package main

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"gopkg.in/yaml.v3"
)

func validFixture(t *testing.T) (string, registry) {
	t.Helper()
	root := t.TempDir()
	for _, path := range []string{"src/feature.go", "tests/feature_test.go", "docs/architecture/ADR-0001.md", "docs/contracts/contract.md", "docs/evidence/live.json", "docs/evidence/unit.json", "docs/evidence/f1-certification-manifest.json"} {
		full := filepath.Join(root, path)
		if err := os.MkdirAll(filepath.Dir(full), 0o755); err != nil {
			t.Fatal(err)
		}
		content := []byte("fixture")
		if path == "docs/evidence/live.json" {
			content = []byte(`{"candidate_sha":"0123456789abcdef0123456789abcdef01234567"}`)
		}
		if path == "docs/evidence/f1-certification-manifest.json" {
			content = []byte(`{"level":"REAL_PASEO_E2E_VERIFIED","status":"verified","implementation_sha":"0123456789abcdef0123456789abcdef01234567","candidate_sha":"abcdef0123456789abcdef0123456789abcdef01"}`)
		}
		if err := os.WriteFile(full, content, 0o644); err != nil {
			t.Fatal(err)
		}
	}
	data := registry{
		SchemaVersion: 1,
		Milestones:    []milestone{{ID: "F1-S1", Title: "Foundation", DecisionStatus: "approved", Deliverables: []string{"Foundation"}, CapabilityIDs: []string{"CAP-001"}, Exclusions: []string{"No deployment"}, Risks: []string{"Risk"}, DeterministicAcceptance: []string{"Tests pass"}, RealAcceptance: []string{"Use product"}, OwnerDecisions: []string{"None"}}},
		Capabilities:  []capability{{ID: "CAP-001", Name: "Feature", Category: "runtime", Purpose: "Do work", ProductObjective: "Objective", DecisionStatus: "approved", Implementation: "implemented", Verification: verification{Deterministic: "verified", LiveAgent: "verified", UAT: "accepted", Deployment: "not_tested", Isolation: "not_tested"}, Owner: "Core", TargetMilestone: "F1-S1", Refs: references{ADR: []string{"docs/architecture/ADR-0001.md"}, Contract: []string{"docs/contracts/contract.md"}, Source: []string{"src/feature.go"}, Test: []string{"tests/feature_test.go"}}, Acceptance: acceptance{Deterministic: "Test it", Real: "Run it"}, Evidence: evidence{Deterministic: []string{"docs/evidence/unit.json"}, LiveAgent: []string{"docs/evidence/f1-certification-manifest.json"}, UAT: []string{"docs/evidence/live.json"}}}},
	}
	return root, data
}

func TestAcceptedADRHistoryCannotBeRewritten(t *testing.T) {
	root := t.TempDir()
	gitTest(t, root, "init", "-q")
	gitTest(t, root, "config", "user.email", "architecturecheck@example.invalid")
	gitTest(t, root, "config", "user.name", "Architecture Checker Test")
	adrDir := filepath.Join(root, "docs/architecture/decisions")
	if err := os.MkdirAll(adrDir, 0o755); err != nil {
		t.Fatal(err)
	}
	adrPath := filepath.Join(adrDir, "ADR-0001-example.md")
	if err := os.WriteFile(adrPath, []byte("# ADR-0001\n\n**Status:** Accepted\n\nOriginal text.\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	gitTest(t, root, "add", ".")
	gitTest(t, root, "commit", "-qm", "accepted ADR")
	baseRef := gitTest(t, root, "rev-parse", "HEAD")
	if err := os.WriteFile(adrPath, []byte("# ADR-0001\n\n**Status:** Accepted\n\nRewritten text.\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	gitTest(t, root, "add", ".")
	gitTest(t, root, "commit", "-qm", "rewrite accepted ADR")
	if err := validateAcceptedADRHistory(root, baseRef); err == nil || !strings.Contains(err.Error(), "changed in place") {
		t.Fatalf("expected accepted ADR rewrite rejection, got %v", err)
	}
}

func TestNewSupersedingADRIsAllowed(t *testing.T) {
	root := t.TempDir()
	gitTest(t, root, "init", "-q")
	gitTest(t, root, "config", "user.email", "architecturecheck@example.invalid")
	gitTest(t, root, "config", "user.name", "Architecture Checker Test")
	adrDir := filepath.Join(root, "docs/architecture/decisions")
	if err := os.MkdirAll(adrDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(adrDir, "ADR-0001-example.md"), []byte("status: Accepted\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	gitTest(t, root, "add", ".")
	gitTest(t, root, "commit", "-qm", "accepted ADR")
	baseRef := gitTest(t, root, "rev-parse", "HEAD")
	if err := os.WriteFile(filepath.Join(adrDir, "ADR-0002-superseding.md"), []byte("status: Proposed\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	gitTest(t, root, "add", ".")
	gitTest(t, root, "commit", "-qm", "add superseding ADR")
	if err := validateAcceptedADRHistory(root, baseRef); err != nil {
		t.Fatalf("new superseding ADR should be allowed: %v", err)
	}
}

func gitTest(t *testing.T, dir string, args ...string) string {
	t.Helper()
	cmd := exec.Command("git", args...)
	cmd.Dir = dir
	output, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("git %v failed: %v\n%s", args, err, output)
	}
	return strings.TrimSpace(string(output))
}

func TestValidRegistry(t *testing.T) {
	root, data := validFixture(t)
	if err := validate(root, data); err != nil {
		t.Fatal(err)
	}
}

func TestLiveAgentRequiresVerifiedCertificationManifest(t *testing.T) {
	root, data := validFixture(t)
	manifestPath := filepath.Join(root, "docs/evidence/f1-certification-manifest.json")
	tests := []struct {
		name    string
		content string
		want    string
	}{
		{"wrong level", `{"level":"SIMULATED","status":"verified","implementation_sha":"0123456789abcdef0123456789abcdef01234567","candidate_sha":"abcdef0123456789abcdef0123456789abcdef01"}`, "must have level REAL_PASEO_E2E_VERIFIED"},
		{"rejected status", `{"level":"REAL_PASEO_E2E_VERIFIED","status":"rejected","implementation_sha":"0123456789abcdef0123456789abcdef01234567","candidate_sha":"abcdef0123456789abcdef0123456789abcdef01"}`, "must have level REAL_PASEO_E2E_VERIFIED"},
		{"short implementation sha", `{"level":"REAL_PASEO_E2E_VERIFIED","status":"verified","implementation_sha":"9567bc5","candidate_sha":"abcdef0123456789abcdef0123456789abcdef01"}`, "requires full implementation_sha and candidate_sha"},
		{"short candidate sha", `{"level":"REAL_PASEO_E2E_VERIFIED","status":"verified","implementation_sha":"0123456789abcdef0123456789abcdef01234567","candidate_sha":"e49f62d"}`, "requires full implementation_sha and candidate_sha"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if err := os.WriteFile(manifestPath, []byte(tt.content), 0o644); err != nil {
				t.Fatal(err)
			}
			err := validate(root, data)
			if err == nil || !strings.Contains(err.Error(), tt.want) {
				t.Fatalf("want error containing %q, got %v", tt.want, err)
			}
		})
	}
}

func TestRegistryRejectsInvalidGovernanceClaims(t *testing.T) {
	tests := []struct {
		name   string
		mutate func(*registry)
		want   string
	}{
		{"duplicate id", func(r *registry) { r.Capabilities = append(r.Capabilities, r.Capabilities[0]) }, "duplicate capability id"},
		{"missing reference", func(r *registry) { r.Capabilities[0].Refs.ADR = []string{"docs/architecture/missing.md"} }, "references missing file"},
		{"implemented without source and tests", func(r *registry) { r.Capabilities[0].Refs.Source = nil; r.Capabilities[0].Refs.Test = nil }, "implemented requires source and test references"},
		{"unsupported live claim", func(r *registry) { r.Capabilities[0].Evidence.LiveAgent = []string{"docs/evidence/live.json"} }, "must reference docs/evidence/*certification-manifest.json"},
		{"unknown dependency", func(r *registry) { r.Capabilities[0].Dependencies = []string{"CAP-999"} }, "unknown dependency"},
		{"invalid state combination", func(r *registry) {
			r.Capabilities[0].Verification.Deterministic = "verified"
			r.Capabilities[0].Evidence.Deterministic = nil
		}, "deterministic verified without evidence"},
		{"bad decision", func(r *registry) { r.Capabilities[0].DecisionStatus = "implemented" }, "invalid decision_status"},
		{"not implemented with verified claims", func(r *registry) { r.Capabilities[0].Implementation = "not_implemented" }, "not_implemented cannot claim verified"},
		{"rejected but implemented", func(r *registry) { r.Capabilities[0].DecisionStatus = "rejected" }, "rejected capability cannot be marked implemented"},
		{"evidence while not tested", func(r *registry) { r.Capabilities[0].Verification.Deterministic = "not_tested" }, "has evidence while marked not_tested"},
		{"missing milestone membership", func(r *registry) { r.Milestones[0].CapabilityIDs = nil }, "must appear exactly once"},
		{"duplicate milestone membership", func(r *registry) {
			r.Milestones = append(r.Milestones, milestone{ID: "F1-S2", Title: "Second", DecisionStatus: "proposed", CapabilityIDs: []string{"CAP-001"}, Deliverables: []string{"One"}, Exclusions: []string{"One"}, Risks: []string{"One"}, DeterministicAcceptance: []string{"One"}, RealAcceptance: []string{"One"}, OwnerDecisions: []string{"One"}})
		}, "must appear exactly once"},
		{"mismatched target milestone", func(r *registry) {
			r.Capabilities[0].TargetMilestone = "F1-S2"
			r.Milestones = append(r.Milestones, milestone{ID: "F1-S2", Title: "Second", DecisionStatus: "proposed", Deliverables: []string{"One"}, Exclusions: []string{"One"}, Risks: []string{"One"}, DeterministicAcceptance: []string{"One"}, RealAcceptance: []string{"One"}, OwnerDecisions: []string{"One"}})
		}, "target_milestone does not match"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			root, data := validFixture(t)
			tt.mutate(&data)
			err := validate(root, data)
			if err == nil || !strings.Contains(err.Error(), tt.want) {
				t.Fatalf("want error containing %q, got %v", tt.want, err)
			}
		})
	}
}

func TestRoadmapGenerationIsDeterministic(t *testing.T) {
	_, data := validFixture(t)
	first := renderRoadmap(data)
	second := renderRoadmap(data)
	if first != second {
		t.Fatal("roadmap generation changed between runs")
	}
	if !strings.Contains(first, "CAP-001") {
		t.Fatal("generated roadmap omitted capability ID")
	}
}

func TestRunRejectsStaleRoadmap(t *testing.T) {
	root, data := validFixture(t)
	registryBytes, err := yaml.Marshal(data)
	if err != nil {
		t.Fatal(err)
	}
	registryPath := "docs/product/capabilities.yaml"
	roadmapPath := "docs/roadmap/ROADMAP.md"
	for _, path := range []string{registryPath, roadmapPath} {
		full := filepath.Join(root, path)
		if err := os.MkdirAll(filepath.Dir(full), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(root, registryPath), registryBytes, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, roadmapPath), []byte("stale"), 0o644); err != nil {
		t.Fatal(err)
	}
	err = run(root, registryPath, roadmapPath, false, "", "")
	if err == nil || !strings.Contains(err.Error(), "roadmap is stale") {
		t.Fatalf("expected stale roadmap failure, got %v", err)
	}
}

func TestContractImpactRequiresMetadataOnlyForProtectedPaths(t *testing.T) {
	impact := impactFile{Entries: []impactEntry{{ID: "GOV-01", Paths: []string{"docs/architecture/decisions/ADR.md"}, Change: "Updated the architecture decision for campaign testing.", Impact: "Keeps architectural records current with the campaign scope."}}}
	if err := validateImpact([]string{"core/internal/feature.go"}, impact, "docs/architecture/contract-impact.yaml"); err != nil {
		t.Fatalf("ordinary code change unexpectedly required metadata: %v", err)
	}
	impact.Entries = append(impact.Entries, impactEntry{ID: "GOV-02", Paths: []string{"docs/architecture/contract-impact.yaml"}, Change: "Added exact metadata for this documentation change.", Impact: "Requires later architecture changes to record concrete effect."})
	if err := validateImpact([]string{"docs/architecture/decisions/ADR.md", "docs/architecture/contract-impact.yaml"}, impact, "docs/architecture/contract-impact.yaml"); err != nil {
		t.Fatal(err)
	}
	if err := validateImpact([]string{"docs/quality/baseline.md", "docs/architecture/contract-impact.yaml"}, impact, "docs/architecture/contract-impact.yaml"); err == nil || !strings.Contains(err.Error(), "metadata missing") {
		t.Fatalf("expected missing metadata error, got %v", err)
	}
	if err := validateImpact([]string{"docs/quality/baseline.md"}, impact, "docs/architecture/contract-impact.yaml"); err == nil || !strings.Contains(err.Error(), "without updating") {
		t.Fatalf("expected impact file update requirement, got %v", err)
	}
	impact.Entries[0].Paths = []string{"docs/quality/*"}
	if err := validateImpact([]string{"docs/quality/baseline.md", "docs/architecture/contract-impact.yaml"}, impact, "docs/architecture/contract-impact.yaml"); err == nil || !strings.Contains(err.Error(), "exact path") {
		t.Fatalf("expected broad pattern rejection, got %v", err)
	}
	impact.Entries[0].Paths = []string{"docs/quality/baseline.md"}
	impact.Entries[0].Change = "updated"
	if err := validateImpact([]string{"docs/quality/baseline.md", "docs/architecture/contract-impact.yaml"}, impact, "docs/architecture/contract-impact.yaml"); err == nil || !strings.Contains(err.Error(), "substantive") {
		t.Fatalf("expected weak impact description rejection, got %v", err)
	}
	impact.Entries[0].Change = "Updated the architecture decision for campaign testing."
	impact.Entries = append(impact.Entries, impactEntry{ID: "GOV-03", Paths: []string{"docs/quality/baseline.md"}, Change: "Changed certification policy text and references.", Impact: "Keeps quality guidance aligned with current acceptance rules."})
	if err := validateImpact([]string{"docs/quality/baseline.md", "docs/architecture/contract-impact.yaml"}, impact, "docs/architecture/contract-impact.yaml"); err == nil || !strings.Contains(err.Error(), "duplicate entries") {
		t.Fatalf("expected duplicate impact path rejection, got %v", err)
	}
}
