package lead

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

const validInformationAssessment = `{"schemaRevision":1,"projectId":"project-x","actionId":"action-x","requestId":"request-x","evidenceRefs":["readme"],"intent":"information","complexity":"small","effortEstimate":{"status":"not_requested","unit":"","minPersonHours":0,"maxPersonHours":0,"assumptions":[],"unknownReason":""},"knowledgeGaps":[],"changeImpact":[],"validationNeeds":[],"assumptions":[],"uncertainty":[],"suggestedNextAction":"Read the supplied evidence.","proposal":{"projectId":"","actionId":"","baseRevision":"","projectConfigHash":"","objective":"","class":"","assumptions":[],"risks":[],"acceptanceCriteria":[],"negativeCases":[],"qualityProfile":"","riskClass":"","budget":{"maxAttempts":0,"timeoutSecs":0},"provider":"","model":"","paseoMode":"","maxRepairCycles":0,"maxWallClockSecs":0,"maxParallel":0,"maxSessions":0,"workUnits":[]}}`

func TestPaseoAssessorUsesAllowlistedRouteSchemaAndDisposableWorkspace(t *testing.T) {
	root := t.TempDir()
	capture := filepath.Join(root, "calls.txt")
	cli := writeAssessorCLI(t, root, capture, validInformationAssessment)
	input := validPaseoAssessorInput()

	got, observation, err := NewPaseoAssessor(WithPaseoBinary(cli)).Assess(context.Background(), input)
	if err != nil {
		t.Fatal(err)
	}
	if got.Intent != "information" || got.RequestID != input.RequestID {
		t.Fatalf("unexpected assessment: %+v", got)
	}
	if observation.SessionID != "session_lead_1" {
		t.Fatalf("expected the discovered session identity, got %+v", observation)
	}
	if observation.Mechanism != "MODEL" || observation.Provider != input.Grant.Provider || observation.Model != input.Grant.Model || observation.Mode != input.Grant.PaseoMode {
		t.Fatalf("unexpected route observation: %+v", observation)
	}
	calls, err := os.ReadFile(capture)
	if err != nil {
		t.Fatal(err)
	}
	text := string(calls)
	for _, want := range []string{
		"workspace\ncreate",
		"--isolation\nlocal\n--path",
		"run\n--provider\ncodex\n--model\ngpt-6-luna\n--mode\nfull-access",
		"--label\nfactory-lead-request=request-x",
		"--json",
		"--output-schema",
		"ls\n-g\n-a\n--json\n--label\nfactory-lead-request=request-x",
		"inspect\nsession_lead_1\n--json",
		"workspace\narchive\nws_lead_test",
		"project-x",
		"readme",
	} {
		if !strings.Contains(text, want) {
			t.Errorf("CLI invocation missing %q", want)
		}
	}
	if strings.Contains(text, "--new-workspace") {
		t.Fatal("expected the explicit disposable workspace ID to be used")
	}
	if count := strings.Count(text, "\nrun\n"); count != 1 {
		t.Fatalf("expected exactly one model run with no retry or route fallback, found %d", count)
	}
	lines := strings.Split(text, "\n")
	foundPath := false
	for index, line := range lines {
		if line == "--path" && index+1 < len(lines) {
			if _, err := os.Stat(lines[index+1]); !os.IsNotExist(err) {
				t.Fatalf("temporary workspace directory remains: %q (stat error %v)", lines[index+1], err)
			}
			foundPath = true
			break
		}
	}
	if !foundPath {
		t.Fatal("workspace path was not captured")
	}
}

func TestPaseoAssessorRejectsMalformedStructuredOutput(t *testing.T) {
	root := t.TempDir()
	capture := filepath.Join(root, "calls.txt")
	cli := writeAssessorCLI(t, root, capture, "not-json")
	_, _, err := NewPaseoAssessor(WithPaseoBinary(cli)).Assess(context.Background(), validPaseoAssessorInput())
	if err == nil || !strings.Contains(err.Error(), "malformed assessment") {
		t.Fatalf("expected malformed assessment rejection, got %v", err)
	}
	calls, readErr := os.ReadFile(capture)
	if readErr != nil || !strings.Contains(string(calls), "workspace\narchive\nws_lead_test") {
		t.Fatalf("workspace was not archived after malformed output: %v", readErr)
	}
}

func TestPaseoAssessorReconcilesSessionAfterRunFailure(t *testing.T) {
	root := t.TempDir()
	capture := filepath.Join(root, "calls.txt")
	cli := writeFailingAssessorCLI(t, root, capture)
	_, _, err := NewPaseoAssessor(WithPaseoBinary(cli)).Assess(context.Background(), validPaseoAssessorInput())
	if err == nil || !strings.Contains(err.Error(), "session_reconciled") {
		t.Fatalf("expected failed run with persisted reconciliation evidence, got %v", err)
	}
	persistedReason := "Model assessment failed: " + err.Error()
	if len(persistedReason) > 600 {
		t.Fatalf("assessment failure reason exceeds API persistence bound: %d bytes", len(persistedReason))
	}
	evidenceStart := strings.Index(err.Error(), "reconciliation=")
	if evidenceStart < 0 {
		t.Fatalf("reconciliation evidence missing from persisted error: %v", err)
	}
	var evidence assessmentRunFailureEvidence
	if err := json.Unmarshal([]byte(err.Error()[evidenceStart+len("reconciliation="):]), &evidence); err != nil {
		t.Fatalf("reconciliation evidence is not machine-readable JSON: %v", err)
	}
	if evidence.SessionReconciliation.SessionID != "session_lead_1" ||
		evidence.SessionReconciliation.ObservedProvider != "codex" ||
		evidence.SessionReconciliation.ObservedModel != "gpt-6-luna" ||
		evidence.SessionReconciliation.ObservedMode != "full-access" ||
		evidence.SessionReconciliation.Status != "session_reconciled" ||
		evidence.WorkspaceArchive != "ok" {
		t.Fatalf("unexpected reconciliation evidence: %+v", evidence)
	}
	calls, readErr := os.ReadFile(capture)
	if readErr != nil {
		t.Fatal(readErr)
	}
	text := string(calls)
	for _, want := range []string{"run\n", "ls\n-g\n-a\n--json", "inspect\nsession_lead_1", "stop\nsession_lead_1", "archive\nsession_lead_1", "workspace\narchive\nws_lead_test"} {
		if !strings.Contains(text, want) {
			t.Errorf("failure reconciliation omitted CLI action %q", want)
		}
	}
	if strings.Count(text, "\nrun\n") != 1 {
		t.Fatalf("failure reconciliation must not retry the paid run")
	}
}

func TestAssessmentRunFailureEvidenceFitsPersistenceBoundAtMaximumSessionID(t *testing.T) {
	evidence := assessmentRunFailureEvidence{
		RunCause: "timeout",
		SessionReconciliation: sessionReconciliation{
			Status: "session_reconciled", SessionID: strings.Repeat("s", domain.MaxPaseoIDLength),
			RequestedProvider: "codex", RequestedModel: "gpt-6-luna", RequestedMode: "full-access",
			ObservedProvider: "codex", ObservedModel: "gpt-6-luna", ObservedMode: "full-access",
			ObservedStatus: "active", RouteStatus: "verified", Stop: "ok", Archive: "ok",
		},
		WorkspaceArchive: "ok",
	}
	errorText := evidence.Error()
	if len(errorText) > 560 {
		t.Fatalf("assessment run error exceeds compact evidence bound: %d bytes", len(errorText))
	}
	marker := strings.Index(errorText, "reconciliation=")
	if marker < 0 {
		t.Fatal("reconciliation marker missing")
	}
	var decoded assessmentRunFailureEvidence
	if err := json.Unmarshal([]byte(errorText[marker+len("reconciliation="):]), &decoded); err != nil {
		t.Fatalf("bounded error payload is not valid JSON: %v", err)
	}
	if decoded.SessionReconciliation.SessionID != evidence.SessionReconciliation.SessionID {
		t.Fatal("bounded evidence lost the session identity")
	}
}

func TestAssessmentWorkspaceCleanupPreservesFailureEvidence(t *testing.T) {
	original := assessmentRunFailureEvidence{
		RunCause:         "timeout",
		WorkspaceArchive: "ok",
		SessionReconciliation: sessionReconciliation{
			Status: "session_reconciled", SessionID: "session_lead_1",
		},
	}
	err := applyAssessmentWorkspaceCleanup(original, errors.New("remove failed"))
	var evidence assessmentRunFailureEvidence
	if !errors.As(err, &evidence) {
		t.Fatalf("cleanup failure replaced structured evidence: %v", err)
	}
	if evidence.WorkspaceCleanup != "failed" || evidence.SessionReconciliation.SessionID != "session_lead_1" {
		t.Fatalf("unexpected cleanup evidence: %+v", evidence)
	}
	marker := strings.Index(err.Error(), "reconciliation=")
	if marker < 0 {
		t.Fatalf("reconciliation marker missing: %v", err)
	}
	var decoded assessmentRunFailureEvidence
	if err := json.Unmarshal([]byte(err.Error()[marker+len("reconciliation="):]), &decoded); err != nil {
		t.Fatalf("cleanup failure corrupted machine-readable evidence: %v", err)
	}
	if decoded.WorkspaceCleanup != "failed" {
		t.Fatalf("cleanup status did not survive encoding: %+v", decoded)
	}
	err = applyAssessmentWorkspaceCleanup(original, nil)
	if !errors.As(err, &evidence) || evidence.WorkspaceCleanup != "ok" {
		t.Fatalf("successful cleanup was not recorded: %+v", err)
	}
	sentinel := errors.New("other failure")
	err = applyAssessmentWorkspaceCleanup(sentinel, errors.New("remove failed"))
	if !errors.Is(err, sentinel) || !strings.Contains(err.Error(), "cleanup failed") {
		t.Fatalf("ordinary errors must preserve cause and cleanup failure: %v", err)
	}
	if err := applyAssessmentWorkspaceCleanup(nil, errors.New("remove failed")); err == nil || !strings.Contains(err.Error(), "cleanup failed") {
		t.Fatalf("cleanup failure was silently dropped: %v", err)
	}
}

func TestDecodeAssessmentRejectsDuplicateMissingAndNullSchemaFields(t *testing.T) {
	input := validPaseoAssessorInput()
	cases := []struct {
		name string
		raw  string
	}{
		{name: "duplicate key", raw: strings.Replace(validInformationAssessment, `"schemaRevision":1`, `"schemaRevision":1,"schemaRevision":1`, 1)},
		{name: "missing required key", raw: strings.Replace(validInformationAssessment, `,"uncertainty":[]`, ``, 1)},
		{name: "null field", raw: strings.Replace(validInformationAssessment, `"evidenceRefs":["readme"]`, `"evidenceRefs":null`, 1)},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := DecodeAssessment([]byte(tc.raw), input); err == nil {
				t.Fatal("expected strict schema rejection")
			}
		})
	}
}

func TestEffortEstimateRequiresBoundedRangeOrExplicitUnknown(t *testing.T) {
	tests := []struct {
		name     string
		estimate EffortEstimate
		wantErr  bool
	}{
		{name: "estimated interval", estimate: EffortEstimate{Status: "estimated", Unit: "person-hours", MinPersonHours: 4, MaxPersonHours: 16, Assumptions: []string{"One implementer; existing test helpers remain usable."}}},
		{name: "unknown with reason", estimate: EffortEstimate{Status: "unknown", UnknownReason: "The request does not define durability semantics."}},
		{name: "unrequested", estimate: EffortEstimate{Status: "not_requested"}},
		{name: "point estimate rejected", estimate: EffortEstimate{Status: "estimated", Unit: "person-hours", MinPersonHours: 8, MaxPersonHours: 8, Assumptions: []string{"Unsupported precision."}}, wantErr: true},
		{name: "missing assumption", estimate: EffortEstimate{Status: "estimated", Unit: "person-hours", MinPersonHours: 2, MaxPersonHours: 12}, wantErr: true},
		{name: "unbounded range", estimate: EffortEstimate{Status: "estimated", Unit: "person-hours", MinPersonHours: 1, MaxPersonHours: 10001, Assumptions: []string{"Bounded work."}}, wantErr: true},
		{name: "unknown without reason", estimate: EffortEstimate{Status: "unknown"}, wantErr: true},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			if err := tc.estimate.Validate(); (err != nil) != tc.wantErr {
				t.Fatalf("Validate() error = %v, wantErr %v", err, tc.wantErr)
			}
		})
	}
}

func TestDecodeAssessmentRequiresExplicitEffortOutcomeWhenRequested(t *testing.T) {
	input := validPaseoAssessorInput()
	input.Objective = "Estimate effort for this change."
	if _, err := DecodeAssessment([]byte(validInformationAssessment), input); err == nil || !strings.Contains(err.Error(), "omitted a requested effort") {
		t.Fatalf("expected missing effort outcome rejection, got %v", err)
	}
	unknown := strings.Replace(validInformationAssessment,
		`"status":"not_requested","unit":"","minPersonHours":0,"maxPersonHours":0,"assumptions":[],"unknownReason":""`,
		`"status":"unknown","unit":"","minPersonHours":0,"maxPersonHours":0,"assumptions":[],"unknownReason":"Durability semantics are missing."`, 1)
	assessment, err := DecodeAssessment([]byte(unknown), input)
	if err != nil || assessment.EffortEstimate.Status != "unknown" {
		t.Fatalf("expected a justified unknown estimate, got assessment=%+v err=%v", assessment.EffortEstimate, err)
	}
}

func TestChangeAssessmentRequiresEffortOutcome(t *testing.T) {
	input := validPaseoAssessorInput()
	assessment := Assessment{
		SchemaRevision:      1,
		ProjectID:           input.ProjectID,
		ActionID:            input.ActionID,
		RequestID:           input.RequestID,
		EvidenceRefs:        []string{"readme"},
		Intent:              "change",
		Complexity:          "small",
		EffortEstimate:      EffortEstimate{Status: "not_requested"},
		SuggestedNextAction: "Plan the requested change.",
	}
	if err := assessment.Validate(input); err == nil || !strings.Contains(err.Error(), "omitted a requested effort") {
		t.Fatalf("expected change assessment to require an effort outcome, got %v", err)
	}
}

func TestPaseoAssessorRejectsMismatchedObservedRoute(t *testing.T) {
	root := t.TempDir()
	capture := filepath.Join(root, "calls.txt")
	cli := writeAssessorCLI(t, root, capture, validInformationAssessment, "opencode")
	_, _, err := NewPaseoAssessor(WithPaseoBinary(cli)).Assess(context.Background(), validPaseoAssessorInput())
	if err == nil || !strings.Contains(err.Error(), `"routeStatus":"mismatch"`) {
		t.Fatalf("expected observed route mismatch evidence, got %v", err)
	}
	calls, readErr := os.ReadFile(capture)
	if readErr != nil || !strings.Contains(string(calls), "workspace\narchive\nws_lead_test") {
		t.Fatalf("workspace was not archived after route mismatch: %v", readErr)
	}
}

func TestPaseoAssessorReconcilesSuccessfulRunWithRouteMismatch(t *testing.T) {
	root := t.TempDir()
	capture := filepath.Join(root, "calls.txt")
	cli := writeAssessorCLI(t, root, capture, validInformationAssessment, "opencode")
	_, _, err := NewPaseoAssessor(WithPaseoBinary(cli)).Assess(context.Background(), validPaseoAssessorInput())
	if err == nil || !strings.Contains(err.Error(), "session_reconciled_route_mismatch") {
		t.Fatalf("expected route mismatch and cleanup evidence, got %v", err)
	}
	if len("Model assessment failed: "+err.Error()) > 600 {
		t.Fatalf("persisted identity failure exceeds 600 bytes: %d", len(err.Error()))
	}
	marker := strings.Index(err.Error(), "reconciliation=")
	if marker < 0 {
		t.Fatalf("identity failure reconciliation evidence missing: %v", err)
	}
	var evidence assessmentRunFailureEvidence
	if err := json.Unmarshal([]byte(err.Error()[marker+len("reconciliation="):]), &evidence); err != nil {
		t.Fatalf("identity failure evidence is not valid JSON: %v", err)
	}
	if evidence.RunCause != "identity_failed" || evidence.SessionReconciliation.SessionID != "session_lead_1" ||
		evidence.SessionReconciliation.RouteStatus != "mismatch" || evidence.SessionReconciliation.ObservedProvider != "" ||
		evidence.SessionReconciliation.Stop != "ok" || evidence.SessionReconciliation.Archive != "ok" {
		t.Fatalf("unexpected mismatch reconciliation evidence: %+v", evidence)
	}
	calls, readErr := os.ReadFile(capture)
	if readErr != nil {
		t.Fatal(readErr)
	}
	text := string(calls)
	for _, want := range []string{"stop\nsession_lead_1", "archive\nsession_lead_1", "workspace\narchive\nws_lead_test"} {
		if !strings.Contains(text, want) {
			t.Errorf("identity failure cleanup omitted %q", want)
		}
	}
	if strings.Count(text, "\nrun\n") != 1 {
		t.Fatalf("identity reconciliation must not retry the model run")
	}
}

func TestPaseoAssessorDoesNotTouchAmbiguousLabeledSessions(t *testing.T) {
	root := t.TempDir()
	capture := filepath.Join(root, "calls.txt")
	cli := writeAmbiguousAssessorCLI(t, root, capture)
	_, _, err := NewPaseoAssessor(WithPaseoBinary(cli)).Assess(context.Background(), validPaseoAssessorInput())
	if err == nil || !strings.Contains(err.Error(), `"status":"ambiguous_sessions"`) {
		t.Fatalf("expected ambiguous session evidence, got %v", err)
	}
	marker := strings.Index(err.Error(), "reconciliation=")
	var evidence assessmentRunFailureEvidence
	if marker < 0 || json.Unmarshal([]byte(err.Error()[marker+len("reconciliation="):]), &evidence) != nil {
		t.Fatalf("expected valid structured ambiguity evidence, got %v", err)
	}
	calls, readErr := os.ReadFile(capture)
	if readErr != nil {
		t.Fatal(readErr)
	}
	text := string(calls)
	if strings.Contains(text, "\nstop\n") || strings.Contains(text, "\narchive\nsession_") {
		t.Fatalf("ambiguous labeled sessions must not be stopped or archived by guessed identity: %s", text)
	}
	if !strings.Contains(text, "workspace\narchive\nws_lead_test") {
		t.Fatal("owned temporary workspace was not archived")
	}
}

func TestPaseoAssessorRejectsRouteOutsideAllowlistBeforeCLI(t *testing.T) {
	root := t.TempDir()
	capture := filepath.Join(root, "calls.txt")
	cli := writeAssessorCLI(t, root, capture, `{}`)
	input := validPaseoAssessorInput()
	input.Grant.PaseoMode = "build"

	_, _, err := NewPaseoAssessor(WithPaseoBinary(cli)).Assess(context.Background(), input)
	if err == nil || !strings.Contains(err.Error(), "not allowlisted") {
		t.Fatalf("expected allowlist rejection, got %v", err)
	}
	if _, err := os.Stat(capture); !os.IsNotExist(err) {
		t.Fatalf("CLI ran for a rejected route (capture stat error %v)", err)
	}
}

func TestPaseoAssessorBoundsEvidenceBeforeCLI(t *testing.T) {
	root := t.TempDir()
	capture := filepath.Join(root, "calls.txt")
	cli := writeAssessorCLI(t, root, capture, `{}`)
	input := validPaseoAssessorInput()
	input.Context["large"] = strings.Repeat("x", maxAssessmentContextValue+1)

	_, _, err := NewPaseoAssessor(WithPaseoBinary(cli)).Assess(context.Background(), input)
	if err == nil || !strings.Contains(err.Error(), "unbounded") {
		t.Fatalf("expected evidence bound rejection, got %v", err)
	}
	if _, err := os.Stat(capture); !os.IsNotExist(err) {
		t.Fatalf("CLI ran for unbounded evidence (capture stat error %v)", err)
	}
}

func validPaseoAssessorInput() Input {
	return Input{
		ProjectID: "project-x",
		ActionID:  "action-x",
		RequestID: "request-x",
		Objective: "Explain the supplied project evidence.",
		Context:   map[string]string{"readme": "Project evidence text."},
		Project: domain.Project{
			ID: "project-x", ConfigHash: "cfg-x", WorkspaceScope: ".",
			QualityProfile: domain.ProfileNodeBasic,
			Capabilities:   []domain.Capability{domain.CapRepositoryEdit, domain.CapNodeToolchain},
		},
		Grant: domain.StandingDelegation{
			ID: "delegation-x", ProjectID: "project-x",
			ObjectiveClasses: []domain.LeadObjectiveClass{domain.LeadMaintenance},
			AllowedPaths:     []string{"src"},
			Capabilities:     []domain.Capability{domain.CapRepositoryEdit, domain.CapNodeToolchain},
			RiskClasses:      []domain.RiskClass{domain.RiskR0},
			Provider:         "codex", Model: "gpt-6-luna", PaseoMode: "full-access",
			MaxAttempts: 1, MaxTimeoutSecs: 30, MaxRepairCycles: 0,
			MaxWallClockSecs: 300, MaxParallel: 1, MaxSessions: 1,
		},
		BaseRevision: strings.Repeat("a", 40),
	}
}

func writeAssessorCLI(t *testing.T, root, capture, output string, inspectProvider ...string) string {
	t.Helper()
	if strings.Contains(output, "'") {
		t.Fatal("test output fixture cannot contain a single quote")
	}
	provider := "codex"
	if len(inspectProvider) > 0 {
		provider = inspectProvider[0]
	}
	path := filepath.Join(root, "paseo-fake")
	script := `#!/bin/sh
CAPTURE='` + capture + `'
printf '%s\n' "$@" >> "$CAPTURE"
case "$1 $2" in
  "workspace create") echo '{"workspaceId":"ws_lead_test"}' ;;
  "workspace archive") exit 0 ;;
  "stop session_lead_1") exit 0 ;;
  "archive session_lead_1") exit 0 ;;
  "ls -g") echo '[{"id":"session_lead_1"}]' ;;
  "inspect session_lead_1")
    workspace_path=""
    previous=""
    while IFS= read -r arg; do
      if [ "$previous" = "--path" ]; then workspace_path="$arg"; fi
      previous="$arg"
    done < "$CAPTURE"
    printf '{"Id":"session_lead_1","Provider":"` + provider + `","Model":"gpt-6-luna","Mode":"full-access","Cwd":"%s","Status":"idle"}' "$workspace_path"
    ;;
  "run --provider")
    schema=""
    previous=""
    for arg in "$@"; do
      if [ "$previous" = "--output-schema" ]; then schema="$arg"; fi
      previous="$arg"
    done
    [ -n "$schema" ] && [ -s "$schema" ] || exit 31
    printf '%s' '` + output + `'
    ;;
  *) exit 32 ;;
esac
`
	if err := os.WriteFile(path, []byte(script), 0o700); err != nil {
		t.Fatal(err)
	}
	return path
}

func writeFailingAssessorCLI(t *testing.T, root, capture string) string {
	t.Helper()
	path := filepath.Join(root, "paseo-failing-fake")
	script := `#!/bin/sh
CAPTURE='` + capture + `'
printf '%s\n' "$@" >> "$CAPTURE"
case "$1 $2" in
  "workspace create") echo '{"workspaceId":"ws_lead_test"}' ;;
  "workspace archive") exit 0 ;;
  "run --provider") exit 23 ;;
  "ls -g") echo '[{"id":"session_lead_1"}]' ;;
  "inspect session_lead_1") echo '{"Id":"session_lead_1","Provider":"codex","Model":"gpt-6-luna","Mode":"full-access","Status":"active"}' ;;
  "stop session_lead_1") exit 0 ;;
  "archive session_lead_1") exit 0 ;;
  *) exit 32 ;;
esac
`
	if err := os.WriteFile(path, []byte(script), 0o700); err != nil {
		t.Fatal(err)
	}
	return path
}

func writeAmbiguousAssessorCLI(t *testing.T, root, capture string) string {
	t.Helper()
	path := filepath.Join(root, "paseo-ambiguous-fake")
	script := `#!/bin/sh
CAPTURE='` + capture + `'
OUTPUT='` + validInformationAssessment + `'
printf '%s\n' "$@" >> "$CAPTURE"
case "$1 $2" in
  "workspace create") echo '{"workspaceId":"ws_lead_test"}' ;;
  "workspace archive") exit 0 ;;
  "ls -g") echo '[{"id":"session_lead_1"},{"id":"session_unrelated_2"}]' ;;
  "run --provider") printf '%s' "$OUTPUT" ;;
  *) exit 32 ;;
esac
`
	if err := os.WriteFile(path, []byte(script), 0o700); err != nil {
		t.Fatal(err)
	}
	return path
}
