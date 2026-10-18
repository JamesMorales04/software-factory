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

// sanitizeEndpoint reduces an OTLP endpoint to scheme://host/path for log
// lines, stripping any userinfo credentials and query parameters. When the
// value does not parse, a fixed placeholder is returned so the raw secret
// can never reach the logs through this path.
func sanitizeEndpoint(v string) string {
	u, err := url.Parse(v)
	if err != nil || u.Host == "" {
		return "unparseable-otlp-endpoint"
	}
	p := u.EscapedPath()
	if p == "" || p == "/" {
		return u.Scheme + "://" + u.Host
	}
	return u.Scheme + "://" + u.Host + p
}
