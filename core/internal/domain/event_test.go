package domain

import "testing"

func TestEventProjectionDropsLegacySourceExcerpt(t *testing.T) {
	event := Event{
		EventType: EventRunCompleted,
		Payload: map[string]any{
			"attemptId": "attempt-1",
			"output": map[string]any{
				"diffBytes":   float64(12),
				"diffExcerpt": "source body must not cross the read boundary",
			},
			"items": []any{
				map[string]any{"diffExcerpt": "nested", "safe": "kept"},
			},
		},
	}
	projected := event.Projected()
	if _, ok := projected.Payload["diffExcerpt"]; ok {
		t.Fatal("top-level diffExcerpt must be projected out")
	}
	output := projected.Payload["output"].(map[string]any)
	if _, ok := output["diffExcerpt"]; ok {
		t.Fatal("nested diffExcerpt must be projected out")
	}
	if output["diffBytes"] != float64(12) {
		t.Fatalf("safe evidence lost: %+v", output)
	}
	items := projected.Payload["items"].([]any)
	if _, ok := items[0].(map[string]any)["diffExcerpt"]; ok {
		t.Fatal("list-nested diffExcerpt must be projected out")
	}
	if items[0].(map[string]any)["safe"] != "kept" {
		t.Fatalf("safe list value lost: %+v", items)
	}
	// The durable event in memory must stay untouched: projection filters
	// the read copy, never the recorded ledger fact.
	if event.Payload["output"].(map[string]any)["diffExcerpt"] == nil {
		t.Fatal("projection mutated the source event")
	}
}

func TestLeadEventRequiresClosedAggregateAndSanitizedPayload(t *testing.T) {
	event := Event{
		EventID: "event-1", EventType: EventLeadRequestCreated, SchemaVersion: EventSchemaVersion,
		AggregateType: AggregateLeadRequest, AggregateID: "request-1", Sequence: 1,
		Payload: map[string]any{
			"requestId": "request-1", "projectId": "project-1", "requestHash": "hash",
			"delegationId": "grant-1", "projectConfigHash": "config-hash", "status": "submitted",
		},
	}
	if err := event.Validate(); err != nil {
		t.Fatalf("valid Lead event rejected: %v", err)
	}
	event.AggregateType = AggregateProject
	if err := event.Validate(); err == nil {
		t.Fatal("Lead event accepted on the wrong aggregate")
	}
	event.AggregateType = AggregateLeadRequest
	event.Payload["objective"] = "raw objective must not enter event history"
	if err := event.Validate(); err == nil {
		t.Fatal("Lead event accepted an unapproved payload field")
	}
}
