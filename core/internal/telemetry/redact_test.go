package telemetry

import (
	"log/slog"
	"strings"
	"testing"
)

func TestRedactMasksKnownSecrets(t *testing.T) {
	const canary = "SECRET_CANARY_9f8e7d6c5b"
	RegisterSecret(canary)
	out := Redact("storing token=" + canary + " in description " + canary)
	if strings.Contains(out, canary) {
		t.Fatalf("canary leaked through Redact: %q", out)
	}
}

func TestRedactMasksSecretShapes(t *testing.T) {
	out := Redact("authorization: Bearer abc123 header password= hunter2 ok")
	if strings.Contains(out, "abc123") || strings.Contains(out, "hunter2") {
		t.Fatalf("secret shape leaked: %q", out)
	}
}

func TestLoggerRedactsAttributes(t *testing.T) {
	var sb strings.Builder
	inner := slog.NewJSONHandler(&sb, nil)
	logger := slog.New(redactingHandler{inner: inner})
	const canary = "LOG_CANARY_12345abcdef"
	RegisterSecret(canary)
	logger.Info("created", "description", "holds "+canary+" inside")
	if strings.Contains(sb.String(), canary) {
		t.Fatalf("canary leaked into log output: %s", sb.String())
	}
}
