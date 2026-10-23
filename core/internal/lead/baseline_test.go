package lead

import (
	"strings"
	"testing"
)

func TestS4DBaselineFrozenFixtures(t *testing.T) {
	cases := []struct {
		name       string
		request    string
		intent     string
		route      string
		unresolved bool
		effort     string
		evidence   map[string]string
	}{
		{name: "S4D-01", request: "In docs/quickstart.md, change the displayed local start command from make serve to make dev. Do not change code or other docs.", intent: "change", route: "DIRECT"},
		{name: "S4D-02", request: "Review whether the new API key handling could expose credentials. Do not edit anything; give me findings and evidence.", intent: "audit"},
		{name: "S4D-03", request: "Make the project production-ready before launch.", intent: "change", unresolved: true},
		{name: "S4D-04", request: "Add a GET /healthz endpoint that reports whether the service can reach its configured database. Keep the response free of credentials. Add tests and update the operator docs.", intent: "change", route: "DIRECT"},
		{name: "S4D-05", request: "Estimate the effort to replace the in-memory job queue with durable SQLite storage, including restart recovery and tests.", intent: "change", effort: "M,XL"},
		{name: "S4D-06", request: "Remove the legacy /v1/export endpoint immediately, but keep all existing integrations working without changes.", intent: "change", unresolved: true},
		{name: "S4D-07", request: "Estimate the effort to add pagination to the existing GET /projects endpoint, with a default page size of 50, a maximum of 200, stable ordering by project ID, and tests.", intent: "change", effort: "S,M"},
		{name: "S4D-08", request: "The last run stopped making progress. Diagnose the likely cause and recommend the next safe observation.", intent: "audit"},
		{name: "S4D-09", request: "Refactor request processing so HTTP handlers, storage, retries, and event delivery share a common pipeline across three services.", intent: "change", route: "FORMAL_SDD"},
		{name: "S4D-10", request: "Add a configurable cache TTL to reduce response latency.", intent: "change", unresolved: true, evidence: map[string]string{"E-10a": "The shared response cache serves user-specific data. The cache key contains URL and locale but no tenant or user identifier.", "E-10b": "Authorization occurs after the cache lookup. No runtime trace is supplied."}},
		{name: "S4D-11", request: "Estimate the effort to update all 1,024 generated icons to use the standard style.", intent: "change", effort: "S,M"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			evidence := tc.evidence
			if evidence == nil {
				evidence = map[string]string{"E-01": "Synthetic evidence."}
			}
			result := EvaluateS4DBaseline(BaselineInput{Request: tc.request, Evidence: evidence})
			if result.BaselineID != S4DBaselineID || result.Intent != tc.intent || result.Route != tc.route || result.Unresolved != tc.unresolved {
				t.Fatalf("unexpected baseline classification: %+v", result)
			}
			gotEffort := strings.Join(result.EffortBands, ",")
			if gotEffort != tc.effort {
				t.Fatalf("effort bands = %q, want %q", gotEffort, tc.effort)
			}
			if result.Authority != "advisory" || len(result.EvidenceRefs) != len(evidence) {
				t.Fatalf("baseline lost authority or evidence bounds: %+v", result)
			}
		})
	}
}
