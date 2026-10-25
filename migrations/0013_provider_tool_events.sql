-- Durable provider tool requests and Factory permission decisions.
CREATE TABLE IF NOT EXISTS provider_tool_events (
  session_record_id TEXT NOT NULL REFERENCES provider_sessions(session_record_id),
  sequence INTEGER NOT NULL,
  call_id TEXT NOT NULL,
  tool TEXT NOT NULL,
  decision TEXT NOT NULL CHECK (decision IN ('REQUEST','ALLOW','DENY','RESULT','ERROR')),
  detail TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (session_record_id, sequence)
);
