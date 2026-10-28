-- Short-lived, one-use enrollment challenges. Only token hashes are stored.
CREATE TABLE IF NOT EXISTS device_pairing_challenges (
  id TEXT PRIMARY KEY,
  poll_token_sha256 TEXT UNIQUE,
  verification_code TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('AVAILABLE','PENDING','APPROVED','REJECTED','CONSUMED','EXPIRED')),
  device_name TEXT,
  device_session_id TEXT REFERENCES device_sessions(id),
  created_by TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  expires_at_ms INTEGER NOT NULL,
  claimed_at_ms INTEGER,
  decided_at_ms INTEGER,
  consumed_at_ms INTEGER,
  CHECK (expires_at_ms > created_at_ms)
);
CREATE INDEX IF NOT EXISTS device_pairing_status_idx ON device_pairing_challenges(status, expires_at_ms);
CREATE TABLE IF NOT EXISTS device_pairing_audit (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  challenge_id TEXT NOT NULL REFERENCES device_pairing_challenges(id),
  action TEXT NOT NULL CHECK (action IN ('created','claimed','approved','rejected','consumed','revocation_requested')),
  actor TEXT NOT NULL,
  occurred_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS device_pairing_audit_challenge_idx ON device_pairing_audit(challenge_id, sequence);
