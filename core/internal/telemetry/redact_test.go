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

func TestSanitizeEndpointStripsCredentials(t *testing.T) {
	cases := map[string]string{
		"http://user:s3cret@collector:4318/v1/traces?token=abc": "http://collector:4318/v1/traces",
		"https://collector:4318":                                "https://collector:4318",
		"not a url at all":                                      "unparseable-otlp-endpoint",
		"":                                                      "unparseable-otlp-endpoint",
	}
	for in, want := range cases {
		if got := sanitizeEndpoint(in); got != want {
			t.Fatalf("sanitizeEndpoint(%q) = %q, want %q", in, got, want)
		}
		if strings.Contains(sanitizeEndpoint(in), "s3cret") {
			t.Fatalf("credential leaked through sanitizeEndpoint(%q)", in)
		}
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
