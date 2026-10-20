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
