package lead

import (
	"sort"
	"strings"
)

// S4D-BASELINE-1 is the versioned deterministic comparison lane described in
// docs/quality/s4-d-semantic-evaluation-fixtures.md. It is intentionally a
// small decision table; it does not produce execution authority or claim
// semantic understanding beyond explicit request forms.
const S4DBaselineID = "S4D-RULES-1"

type BaselineInput struct {
	Request  string
	Evidence map[string]string
}

type BaselineResult struct {
	BaselineID   string   `json:"baselineId"`
	Intent       string   `json:"intent"`
	Route        string   `json:"route,omitempty"`
	Scope        string   `json:"scope"`
	Unresolved   bool     `json:"unresolved"`
	Summary      string   `json:"summary"`
	Unknowns     []string `json:"unknowns"`
	Questions    []string `json:"questions"`
	Plan         []string `json:"plan"`
	EffortBands  []string `json:"effortBands,omitempty"`
	Confidence   string   `json:"confidence"`
	EvidenceRefs []string `json:"evidenceRefs"`
	Authority    string   `json:"authority"`
}

// EvaluateS4DBaseline applies the frozen rule ordering to one request and its
// supplied evidence references. It does not inspect repository state.
func EvaluateS4DBaseline(input BaselineInput) BaselineResult {
	request := strings.ToLower(strings.TrimSpace(input.Request))
	result := BaselineResult{
		BaselineID: S4DBaselineID,
		Intent:     "uncertain",
		Scope:      strings.TrimSpace(input.Request),
		Summary:    "Request intent is not recognized by the frozen decision table.",
		Confidence: "low",
		Authority:  "advisory",
		Unknowns:   []string{},
		Questions:  []string{},
		Plan:       []string{},
	}
	for ref := range input.Evidence {
		result.EvidenceRefs = append(result.EvidenceRefs, ref)
	}
	sort.Strings(result.EvidenceRefs)

	switch {
	case containsAny(request, "estimate the effort", "estimate effort", "estimate the time", "estimate how long"):
		result.Intent = "change"
		result.Summary = "Estimate requested outcomes and name missing design or recovery drivers."
		result.Plan = []string{"Decompose named outcomes and identify missing design or recovery decisions."}
		result.EffortBands = []string{"S", "M"}
		result.Confidence = "low"
		if containsAny(request, "durable", "restart", "migration", "recovery", "replay", "transaction") {
			result.EffortBands = []string{"M", "XL"}
		}
	case containsAny(request, "diagnose", "what caused", "why did"):
		result.Intent = "audit"
		result.Summary = "Give a tentative diagnosis and request a bounded read-only observation."
		result.Confidence = "low"
	case containsAny(request, "review whether", "review if", "audit", "do not edit", "don't edit", "without edits"):
		result.Intent = "audit"
		result.Summary = "Report evidence-backed findings and distinguish unsupported conclusions."
		result.Confidence = "medium"
	case containsAny(request, "explain", "what is", "where is", "how does"):
		result.Intent = "informational"
		result.Summary = "Answer from supplied evidence only."
		result.Confidence = "medium"
	case containsAny(request, "change", "add ", "remove ", "replace ", "update ", "make ", "refactor "):
		result.Intent = "change"
		result.Summary = "Preserve the explicit requested scope and list only required work."
		result.Confidence = "medium"
	}

	if result.Intent == "change" && len(result.EffortBands) == 0 {
		result.Route = "DIRECT"
		result.Plan = []string{"Implement only the explicit requested outcome."}
		if containsAny(request, "endpoint") && containsAny(request, "test") && containsAny(request, "doc") {
			result.Plan = []string{"Define the endpoint contract and resolve stated open choices.", "Wire the requested endpoint behavior.", "Add the requested tests.", "Update the requested documentation."}
		}
		if containsAny(request, "test") && !containsAny(request, "tests") {
			result.Plan = append(result.Plan, "Add the requested tests.")
		} else if containsAny(request, "tests") && !containsAny(request, "endpoint") {
			result.Plan = append(result.Plan, "Add the requested tests.")
		}
		if containsAny(request, "docs", "documentation") && !containsAny(request, "endpoint") {
			result.Plan = append(result.Plan, "Update the requested documentation.")
		}
		if containsAny(request, "production-ready", "production ready", "before launch") {
			result.Unresolved = true
			result.Route = ""
			result.Unknowns = append(result.Unknowns, "target environment", "acceptance objectives")
			result.Questions = append(result.Questions, "Which target environment and acceptance objectives define completion?")
			result.Plan = []string{}
		}
		if containsAny(request, "across") && containsAny(request, "services", "systems") {
			result.Route = "FORMAL_SDD"
		}
		if containsAny(request, "immediately", "without changes") && containsAny(request, "keep all", "existing integrations") {
			result.Unresolved = true
			result.Route = ""
			result.Unknowns = append(result.Unknowns, "conflicting compatibility and removal requirements")
			result.Questions = append(result.Questions, "Should removal or compatibility take priority, and what replacement or migration window applies?")
			result.Plan = []string{}
		}
	}
	evidenceText := make([]string, 0, len(input.Evidence))
	for _, value := range input.Evidence {
		evidenceText = append(evidenceText, strings.ToLower(value))
	}
	allEvidence := strings.Join(evidenceText, " ")
	if result.Intent == "change" && containsAny(request, "cache") &&
		containsAny(allEvidence, "user-specific", "user specific") &&
		containsAny(allEvidence, "authorization occurs after", "auth occurs after", "no tenant", "no user identifier") {
		result.Unresolved = true
		result.Route = ""
		result.Unknowns = append(result.Unknowns, "cache isolation and authorization ordering")
		result.Questions = append(result.Questions, "Can cache isolation and authorization order be shown safe with read-only review and tests?")
		result.Plan = []string{}
		result.Summary = "Pause the requested cache change for a bounded security review of isolation and authorization."
		result.Confidence = "low"
	}
	if containsAny(allEvidence, "not specified", "not supplied", "no ", "none ") {
		result.Unknowns = append(result.Unknowns, "one or more requirements are absent from the supplied evidence")
		if result.Intent == "change" && containsAny(request, "endpoint") && containsAny(allEvidence, "timeout", "response", "readiness") {
			result.Unknowns = append(result.Unknowns, "endpoint response, timeout, or readiness behavior")
		}
	}
	return result
}

func containsAny(text string, markers ...string) bool {
	for _, marker := range markers {
		if strings.Contains(text, marker) {
			return true
		}
	}
	return false
}
