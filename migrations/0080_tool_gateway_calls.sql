-- Durable Core-owned audit for mediated MCP tool calls.
--
-- The gateway decides policy, roles and bounds; Core persists every allowed or
-- denied call with the Worker/Attempt/generation binding and the argument
-- digest. Raw arguments, MCP endpoints, credentials and session tokens are
-- never stored. Rows are append-only evidence and intentionally do not cascade
-- with runtime cleanup.

CREATE TABLE IF NOT EXISTS tool_gateway_calls (
  call_id TEXT PRIMARY KEY,
  worker_id TEXT NOT NULL,
  attempt_id TEXT NOT NULL,
  generation INTEGER NOT NULL CHECK (generation > 0),
  role TEXT NOT NULL,
  tool TEXT NOT NULL,
  args_sha256 TEXT NOT NULL CHECK (length(args_sha256) = 64),
  decision TEXT NOT NULL CHECK (decision IN ('ALLOW', 'DENY')),
  response_bytes INTEGER NOT NULL CHECK (response_bytes >= 0),
  duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0),
  error TEXT,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS tool_gateway_calls_created_idx
  ON tool_gateway_calls(created_at DESC, call_id DESC);

CREATE INDEX IF NOT EXISTS tool_gateway_calls_attempt_idx
  ON tool_gateway_calls(attempt_id, created_at DESC);

CREATE TRIGGER IF NOT EXISTS tool_gateway_calls_no_update
BEFORE UPDATE ON tool_gateway_calls
BEGIN
  SELECT RAISE(ABORT, 'tool gateway audit rows are immutable');
END;

CREATE TRIGGER IF NOT EXISTS tool_gateway_calls_no_delete
BEFORE DELETE ON tool_gateway_calls
BEGIN
  SELECT RAISE(ABORT, 'tool gateway audit rows are immutable');
END;
