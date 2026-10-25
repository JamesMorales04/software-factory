-- Per-turn provider authorization and usage reconciliation.
CREATE TABLE IF NOT EXISTS provider_turn_reservations (
  reservation_id TEXT PRIMARY KEY,
  session_record_id TEXT NOT NULL REFERENCES provider_sessions(session_record_id),
  attempt_id TEXT NOT NULL REFERENCES attempts(id),
  generation INTEGER NOT NULL CHECK (generation > 0),
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  max_input_tokens INTEGER NOT NULL CHECK (max_input_tokens > 0),
  max_output_tokens INTEGER NOT NULL CHECK (max_output_tokens > 0),
  max_total_tokens INTEGER NOT NULL CHECK (max_total_tokens > 0),
  max_cost_micros INTEGER NOT NULL CHECK (max_cost_micros > 0),
  request_sha256 TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('RESERVED','SETTLED','RELEASED')),
  input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
  output_tokens INTEGER NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
  total_tokens INTEGER NOT NULL DEFAULT 0 CHECK (total_tokens >= 0),
  cost_micros INTEGER NOT NULL DEFAULT 0 CHECK (cost_micros >= 0),
  usage_known INTEGER NOT NULL DEFAULT 0 CHECK (usage_known IN (0,1)),
  error TEXT,
  expires_at_ms INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS provider_turn_one_reserved_idx
  ON provider_turn_reservations(session_record_id) WHERE state='RESERVED';
CREATE INDEX IF NOT EXISTS provider_turn_campaign_idx
  ON provider_turn_reservations(attempt_id,state);
