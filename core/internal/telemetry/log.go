// Package telemetry separates the three observability planes: durable
// domain events (owned by the store), operational traces/metrics
// (OpenTelemetry, best effort) and structured redacted logs. A failing
// exporter must degrade telemetry, never block business persistence.
package telemetry

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"log/slog"
	"os"
	"regexp"
	"strings"
	"sync"
)

var (
	// secretPatterns masks common secret shapes in free text before logging.
	secretPatterns = []*regexp.Regexp{
		regexp.MustCompile(`(?i)(bearer)\s+[^\s,;"]+`),
		regexp.MustCompile(`(?i)(token|secret|password|api[_-]?key|authorization|set-cookie)\s*[:=]\s*([^\s,;"]+)`),
	}

	mu            sync.RWMutex
	knownSecrets  = map[string]struct{}{}
	defaultLogger *slog.Logger
)

// RegisterSecret records a value that must never appear in logs. The API
// token is registered at startup; tests register synthetic canaries.
func RegisterSecret(v string) {
	if v == "" {
		return
	}
	mu.Lock()
	defer mu.Unlock()
	knownSecrets[v] = struct{}{}
}

// Redact masks known secret values and common secret key=value shapes.
func Redact(s string) string {
	mu.RLock()
	defer mu.RUnlock()
	out := s
	for _, re := range secretPatterns {
		out = re.ReplaceAllString(out, `$1[REDACTED]`)
	}
	for secret := range knownSecrets {
		if secret != "" {
			out = strings.ReplaceAll(out, secret, "[REDACTED]")
		}
	}
	return out
}

// redactingHandler wraps a slog handler and redacts string attributes.
type redactingHandler struct{ inner slog.Handler }

func (h redactingHandler) Enabled(ctx context.Context, l slog.Level) bool {
	return h.inner.Enabled(ctx, l)
}

func (h redactingHandler) Handle(ctx context.Context, r slog.Record) error {
	redacted := slog.NewRecord(r.Time, r.Level, Redact(r.Message), r.PC)
	r.Attrs(func(a slog.Attr) bool {
		redacted.AddAttrs(redactAttr(a))
		return true
	})
	return h.inner.Handle(ctx, redacted)
}

func redactAttr(a slog.Attr) slog.Attr {
	if a.Value.Kind() == slog.KindString {
		a.Value = slog.StringValue(Redact(a.Value.String()))
	}
	return a
}

func (h redactingHandler) WithAttrs(attrs []slog.Attr) slog.Handler {
	out := make([]slog.Attr, 0, len(attrs))
	for _, a := range attrs {
		out = append(out, redactAttr(a))
	}
	return redactingHandler{inner: h.inner.WithAttrs(out)}
}

func (h redactingHandler) WithGroup(name string) slog.Handler {
	return redactingHandler{inner: h.inner.WithGroup(name)}
}

// NewLogger builds the process JSON logger. Level comes from LOG_LEVEL
// (debug, info, warn, error); output is stderr so stdout stays clean.
func NewLogger() *slog.Logger {
	level := slog.LevelInfo
	switch strings.ToLower(os.Getenv("LOG_LEVEL")) {
	case "debug":
		level = slog.LevelDebug
	case "warn":
		level = slog.LevelWarn
	case "error":
		level = slog.LevelError
	}
	inner := slog.NewJSONHandler(os.Stderr, &slog.HandlerOptions{Level: level})
	defaultLogger = slog.New(redactingHandler{inner: inner})
	return defaultLogger
}

// Logger returns the process logger, creating it on first use.
func Logger() *slog.Logger {
	if defaultLogger == nil {
		return NewLogger()
	}
	return defaultLogger
}

// NewCorrelationID generates a hex correlation ID for a request or event
// chain that arrived without one.
func NewCorrelationID() string {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "corr-fallback"
	}
	return hex.EncodeToString(b[:])
}

type ctxKey string

const correlationKey ctxKey = "correlation-id"

// WithCorrelationID stores the correlation ID in the context.
func WithCorrelationID(ctx context.Context, id string) context.Context {
	return context.WithValue(ctx, correlationKey, id)
}

// CorrelationIDFrom reads the correlation ID back; empty when absent.
func CorrelationIDFrom(ctx context.Context) string {
	if v, ok := ctx.Value(correlationKey).(string); ok {
		return v
	}
	return ""
}
