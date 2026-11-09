-- Typed semantic triage/stack assessment for an intake request. The model
-- result is advisory; Core triage floors and the frozen plan remain authority.
CREATE TABLE IF NOT EXISTS agentic_assessments (
  request_id TEXT PRIMARY KEY REFERENCES intake_requests(id),
  response_json TEXT NOT NULL,
  schema_sha256 TEXT NOT NULL,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  request_sha256 TEXT NOT NULL,
  input_tokens INTEGER NOT NULL,
  output_tokens INTEGER NOT NULL,
  total_tokens INTEGER NOT NULL,
  cost_micros INTEGER NOT NULL,
  usage_known INTEGER NOT NULL,
  created_at TEXT NOT NULL
);
