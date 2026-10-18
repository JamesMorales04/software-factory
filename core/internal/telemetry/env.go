package telemetry

import (
	"os"
	"strings"
)

// otlpEndpoint reads the optional OTLP HTTP endpoint. Empty means tracing
// stays fully local (no-op). Values are intentionally not logged to avoid
// leaking embedded credentials in endpoint URLs.
func otlpEndpoint() string {
	v := strings.TrimSpace(os.Getenv("OTEL_EXPORTER_OTLP_ENDPOINT"))
	if v == "" {
		return ""
	}
	// Guard against accidental boolean-style flags.
	if v == "1" || strings.EqualFold(v, "true") {
		return ""
	}
	return v
}
