-- Durable Owner declaration that the upstream OpenCode Go Extra Usage setting
-- is disabled for one exact provider configuration.
--
-- The binding is the real provider configuration identity: provider, canonical
-- base URL and the existing credential fingerprint (never the credential).
-- Settings revision is deliberately not part of the key: model, timeout or
-- display changes do not change the upstream Extra Usage setting. A changed
-- credential fingerprint or base URL, or an explicit revocation, invalidates
-- the declaration and requires a new Owner confirmation.
CREATE TABLE IF NOT EXISTS provider_extra_usage_declarations (
  declaration_id TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  base_url TEXT NOT NULL,
  credential_fingerprint TEXT NOT NULL,
  principal_id TEXT NOT NULL,
  statement TEXT NOT NULL,
  extra_usage TEXT NOT NULL CHECK (extra_usage IN ('DISABLED')),
  declared_at TEXT NOT NULL,
  revoked_at TEXT,
  revoked_by TEXT,
  idempotency_key TEXT NOT NULL,
  UNIQUE (provider, idempotency_key)
);

CREATE UNIQUE INDEX IF NOT EXISTS provider_extra_usage_active_idx
  ON provider_extra_usage_declarations(provider, base_url, credential_fingerprint)
  WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS provider_extra_usage_provider_idx
  ON provider_extra_usage_declarations(provider, declared_at);
