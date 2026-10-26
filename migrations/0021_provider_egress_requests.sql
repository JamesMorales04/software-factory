-- Bounded per-turn provider egress request binding.
ALTER TABLE provider_turn_reservations ADD COLUMN max_requests INTEGER NOT NULL DEFAULT 1;
ALTER TABLE provider_turn_reservations ADD COLUMN requests_bound INTEGER NOT NULL DEFAULT 0;
CREATE TABLE IF NOT EXISTS provider_turn_egress_requests (
  request_id TEXT PRIMARY KEY,
  reservation_id TEXT NOT NULL REFERENCES provider_turn_reservations(reservation_id),
  sequence INTEGER NOT NULL CHECK (sequence >= 0),
  actual_request_sha256 TEXT NOT NULL,
  model TEXT NOT NULL,
  path TEXT NOT NULL,
  body_bytes INTEGER NOT NULL CHECK (body_bytes >= 0),
  created_at TEXT NOT NULL,
  UNIQUE(reservation_id, sequence)
);
CREATE INDEX IF NOT EXISTS provider_turn_egress_reservation_idx
  ON provider_turn_egress_requests(reservation_id, sequence);
