-- Browser and paired-device sessions. Raw bearer material is never persisted.
CREATE TABLE IF NOT EXISTS device_sessions (
  id TEXT PRIMARY KEY,
  device_name TEXT NOT NULL,
  session_token_sha256 TEXT NOT NULL UNIQUE,
  csrf_token_sha256 TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  last_activity_at_ms INTEGER NOT NULL,
  expires_at_ms INTEGER NOT NULL,
  revoked_at_ms INTEGER,
  created_by TEXT NOT NULL,
  CHECK (expires_at_ms > created_at_ms)
);
CREATE INDEX IF NOT EXISTS device_sessions_active_expiry
  ON device_sessions(revoked_at_ms, expires_at_ms);

CREATE TABLE IF NOT EXISTS device_session_audit (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  actor TEXT NOT NULL,
  action TEXT NOT NULL,
  recorded_at_ms INTEGER NOT NULL,
  FOREIGN KEY(session_id) REFERENCES device_sessions(id)
);

CREATE TABLE IF NOT EXISTS owner_bootstrap_claim (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  session_id TEXT NOT NULL UNIQUE,
  claimed_at_ms INTEGER NOT NULL,
  FOREIGN KEY(session_id) REFERENCES device_sessions(id)
);
