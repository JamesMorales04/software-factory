package telemetry

import (
	"net/url"
	"os"
	"strings"
)

// otlpEndpoint reads the optional OTLP HTTP endpoint. Empty means tracing
// stays fully local (no-op). Values are intentionally not logged raw:
// endpoint URLs may embed credentials or sensitive query parameters.
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

// sanitizeEndpoint reduces an OTLP endpoint to scheme://host for log
// lines, stripping userinfo credentials, paths and query parameters —
// any of which may carry secrets. When the value does not parse, a fixed
// placeholder is returned so the raw secret can never reach the logs
// through this path. The host alone identifies the exporter for
// diagnostics; nothing more is needed.
func sanitizeEndpoint(v string) string {
	u, err := url.Parse(v)
	if err != nil || u.Host == "" {
		return "unparseable-otlp-endpoint"
	}
	return u.Scheme + "://" + u.Host
}
