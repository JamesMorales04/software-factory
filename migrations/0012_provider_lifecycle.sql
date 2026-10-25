-- Extend provider-session states for active ownership, cancellation and recovery.
PRAGMA foreign_keys = OFF;
CREATE TABLE provider_sessions_new (
  session_record_id TEXT PRIMARY KEY,
  attempt_id TEXT NOT NULL REFERENCES attempts(id),
  worker_id TEXT NOT NULL,
  generation INTEGER NOT NULL CHECK (generation > 0),
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  provider_mode TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN
    ('INTENT','ACTIVE','CANCEL_REQUESTED','CANCELLED','CLEANED','COMPLETED','FAILED','ORPHANED')),
  provider_session_id TEXT,
  provider_directory TEXT,
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
INSERT INTO provider_sessions_new
  (session_record_id,attempt_id,worker_id,generation,provider,model,provider_mode,state,
   provider_session_id,provider_directory,turn_id,input_tokens,output_tokens,total_tokens,cost_micros,
   usage_known,cleanup_receipt,error,created_at,updated_at)
SELECT session_record_id,attempt_id,worker_id,generation,provider,model,provider_mode,state,
       provider_session_id,NULL,turn_id,input_tokens,output_tokens,total_tokens,cost_micros,
       usage_known,cleanup_receipt,error,created_at,updated_at
FROM provider_sessions;
DROP TABLE provider_sessions;
ALTER TABLE provider_sessions_new RENAME TO provider_sessions;
CREATE INDEX IF NOT EXISTS provider_sessions_attempt_idx
  ON provider_sessions(attempt_id, generation);
PRAGMA foreign_keys = ON;
