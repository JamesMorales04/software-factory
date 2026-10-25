-- Explicit trusted-local provider authorization and durable provider-session state.
-- Provider dispatch remains disabled unless Core validates every field below.

ALTER TABLE campaign_authorizations ADD COLUMN provider TEXT NOT NULL DEFAULT '';
ALTER TABLE campaign_authorizations ADD COLUMN model TEXT NOT NULL DEFAULT '';
ALTER TABLE campaign_authorizations ADD COLUMN provider_mode TEXT NOT NULL DEFAULT '';
ALTER TABLE campaign_authorizations ADD COLUMN delegation_id TEXT NOT NULL DEFAULT '';
ALTER TABLE campaign_authorizations ADD COLUMN max_sessions INTEGER NOT NULL DEFAULT 0;
ALTER TABLE campaign_authorizations ADD COLUMN max_turns INTEGER NOT NULL DEFAULT 0;
ALTER TABLE campaign_authorizations ADD COLUMN max_input_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE campaign_authorizations ADD COLUMN max_output_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE campaign_authorizations ADD COLUMN max_cost_micros INTEGER NOT NULL DEFAULT 0;
ALTER TABLE campaign_authorizations ADD COLUMN deadline_secs INTEGER NOT NULL DEFAULT 0;
ALTER TABLE campaign_authorizations ADD COLUMN allowed_tools_json TEXT NOT NULL DEFAULT '[]';

CREATE TABLE IF NOT EXISTS provider_sessions (
  session_record_id TEXT PRIMARY KEY,
  attempt_id TEXT NOT NULL REFERENCES attempts(id),
  worker_id TEXT NOT NULL,
  generation INTEGER NOT NULL CHECK (generation > 0),
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  provider_mode TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN
    ('INTENT','ACTIVE','COMPLETED','FAILED','CANCELLED','CLEANED')),
  provider_session_id TEXT,
  turn_id TEXT,
  input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
  output_tokens INTEGER NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
  total_tokens INTEGER NOT NULL DEFAULT 0 CHECK (total_tokens >= 0),
  cost_micros INTEGER NOT NULL DEFAULT 0 CHECK (cost_micros >= 0),
  usage_known INTEGER NOT NULL DEFAULT 0 CHECK (usage_known IN (0, 1)),
  cleanup_receipt TEXT,
  error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(attempt_id,generation)
);
CREATE INDEX IF NOT EXISTS provider_sessions_attempt_idx
  ON provider_sessions(attempt_id, generation);
