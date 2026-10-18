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

// validOTLPEndpoint reports whether an endpoint is well-formed enough to
// hand to the OTLP SDK. The SDK's internal logger can echo malformed
// endpoint values verbatim, bypassing sanitizeEndpoint (which only runs on
// the exporter's own error path). Malformed values are therefore rejected
// before the SDK ever sees them, and tracing stays local-only. The raw
// value is never logged here.
func validOTLPEndpoint(v string) bool {
	u, err := url.Parse(v)
	if err != nil {
		return false
	}
	if u.Scheme != "http" && u.Scheme != "https" {
		return false
	}
	return u.Host != ""
}
