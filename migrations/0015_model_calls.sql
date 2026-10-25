-- Core-owned model-call authorization, reservation and settlement.
-- A reservation is durable before any upstream effect. A settled response may
-- be replayed by exact request identity; an interrupted or unknown call is
-- never silently retried.

CREATE TABLE IF NOT EXISTS model_call_reservations (
  reservation_id TEXT PRIMARY KEY,
  role TEXT NOT NULL CHECK (role IN ('LEAD','SEMANTIC_REVIEWER','EVALUATION')),
  principal_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  objective_id TEXT NOT NULL,
  scope_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  purpose TEXT NOT NULL,
  request_sha256 TEXT NOT NULL,
  max_input_tokens INTEGER NOT NULL CHECK (max_input_tokens > 0),
  max_output_tokens INTEGER NOT NULL CHECK (max_output_tokens > 0),
  max_total_tokens INTEGER NOT NULL CHECK (max_total_tokens > 0),
  max_cost_micros INTEGER NOT NULL CHECK (max_cost_micros > 0),
  input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
  output_tokens INTEGER NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
  total_tokens INTEGER NOT NULL DEFAULT 0 CHECK (total_tokens >= 0),
  cost_micros INTEGER NOT NULL DEFAULT 0 CHECK (cost_micros >= 0),
  usage_known INTEGER NOT NULL DEFAULT 0 CHECK (usage_known IN (0,1)),
  response_text TEXT,
  state TEXT NOT NULL CHECK
    (state IN ('RESERVED','SETTLED','RELEASED','AMBIGUOUS','UNKNOWN')),
  error TEXT,
  expires_at_ms INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS model_call_settled_identity_idx
  ON model_call_reservations(role, objective_id, request_sha256)
  WHERE state='SETTLED';
CREATE UNIQUE INDEX IF NOT EXISTS model_call_unresolved_identity_idx
  ON model_call_reservations(role, objective_id, request_sha256)
  WHERE state IN ('RESERVED','AMBIGUOUS','UNKNOWN');
CREATE INDEX IF NOT EXISTS model_call_scope_idx
  ON model_call_reservations(scope_id, state);
CREATE INDEX IF NOT EXISTS model_call_objective_idx
  ON model_call_reservations(objective_id, created_at);
