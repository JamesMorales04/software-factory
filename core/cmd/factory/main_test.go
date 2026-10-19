package main

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

func TestLoadOrCreateTokenStoresPrivateReusableToken(t *testing.T) {
	tokenPath := filepath.Join(t.TempDir(), "api.token")
	t.Setenv("FACTORY_API_TOKEN", "")
	first, err := loadOrCreateToken(tokenPath)
	if err != nil {
		t.Fatal(err)
	}
	if len(first) != 64 {
		t.Fatalf("generated token length = %d, want 64 hex characters", len(first))
	}
	info, err := os.Stat(tokenPath)
	if err != nil {
		t.Fatal(err)
	}
	if got := info.Mode().Perm(); got != 0o600 {
		t.Fatalf("token file mode = %04o, want 0600", got)
	}
	second, err := loadOrCreateToken(tokenPath)
	if err != nil {
		t.Fatal(err)
	}
	if second != first {
		t.Fatal("token changed across starts")
	}
}

func TestLoadOrCreateTokenPersistsExplicitEnvironmentTokenPrivately(t *testing.T) {
	tokenPath := filepath.Join(t.TempDir(), "api.token")
	t.Setenv("FACTORY_API_TOKEN", "one-time-test-token")
	token, err := loadOrCreateToken(tokenPath)
	if err != nil {
		t.Fatal(err)
	}
	if token != "one-time-test-token" {
		t.Fatalf("token = %q, want supplied value", token)
	}
	info, err := os.Stat(tokenPath)
	if err != nil {
		t.Fatal(err)
	}
	if got := info.Mode().Perm(); got != 0o600 {
		t.Fatalf("token file mode = %04o, want 0600", got)
	}
	t.Setenv("FACTORY_API_TOKEN", "")
	reloaded, err := loadOrCreateToken(tokenPath)
	if err != nil {
		t.Fatal(err)
	}
	if reloaded != token {
		t.Fatal("saved token differs from supplied token")
	}
}

func TestLoadOrCreateTokenRejectsEmptyExistingFile(t *testing.T) {
	tokenPath := filepath.Join(t.TempDir(), "api.token")
	if err := os.WriteFile(tokenPath, nil, 0o600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("FACTORY_API_TOKEN", "")
	if _, err := loadOrCreateToken(tokenPath); err == nil {
		t.Fatal("expected an empty token file to fail")
	}
}

func TestOfficeURLUsesLoopbackForWildcardAndFormatsIPv6(t *testing.T) {
	tests := []struct {
		addr string
		want string
	}{
		{addr: "0.0.0.0:8471", want: "http://127.0.0.1:8471"},
		{addr: ":8471", want: "http://127.0.0.1:8471"},
		{addr: "[::1]:8471", want: "http://[::1]:8471"},
	}
	for _, test := range tests {
		if got := officeURL(test.addr); got != test.want {
			t.Errorf("officeURL(%q) = %q, want %q", test.addr, got, test.want)
		}
	}
}

func TestLoadConfigRejectsNonLoopbackAddress(t *testing.T) {
	t.Setenv("FACTORY_ADDR", "0.0.0.0:8471")
	if _, err := loadConfig(); err == nil {
		t.Fatal("expected non-loopback address to be rejected")
	}
}

func TestTailWritesOnlyRequestedRecentLines(t *testing.T) {
	path := filepath.Join(t.TempDir(), "factory.log")
	if err := os.WriteFile(path, []byte("one\ntwo\nthree\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	file, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = file.Close() }()

	var out bytes.Buffer
	if err := tail(file, 2, &out); err != nil {
		t.Fatal(err)
	}
	if got, want := out.String(), "two\nthree\n"; got != want {
		t.Fatalf("tail output = %q, want %q", got, want)
	}
}
