-- Core-owned provider and model configuration with encrypted credentials.
-- Secrets are stored only as AES-256-GCM ciphertext under a master key held in
-- a separate protected file. Audit rows never contain credential material.

CREATE TABLE IF NOT EXISTS provider_settings (
  provider TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('opencode','codex')),
  model TEXT NOT NULL,
  base_url TEXT NOT NULL,
  auth_mode TEXT NOT NULL CHECK (auth_mode IN ('api_key')),
  credential_ciphertext TEXT NOT NULL,
  credential_nonce TEXT NOT NULL,
  credential_fingerprint TEXT NOT NULL,
  enabled INTEGER NOT NULL CHECK (enabled IN (0,1)),
  is_default INTEGER NOT NULL CHECK (is_default IN (0,1)),
  max_turns INTEGER NOT NULL CHECK (max_turns > 0),
  max_input_tokens INTEGER NOT NULL CHECK (max_input_tokens > 0),
  max_output_tokens INTEGER NOT NULL CHECK (max_output_tokens > 0),
  max_cost_micros INTEGER NOT NULL CHECK (max_cost_micros > 0),
  input_micros_per_million INTEGER NOT NULL CHECK (input_micros_per_million >= 0),
  output_micros_per_million INTEGER NOT NULL CHECK (output_micros_per_million >= 0),
  revision INTEGER NOT NULL CHECK (revision > 0),
  updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS provider_settings_default_idx
  ON provider_settings(is_default) WHERE is_default=1;

CREATE TABLE IF NOT EXISTS provider_settings_audit (
  audit_id TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  revision INTEGER NOT NULL,
  action TEXT NOT NULL,
  actor TEXT NOT NULL,
  detail TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS provider_settings_audit_provider_idx
  ON provider_settings_audit(provider, created_at);
