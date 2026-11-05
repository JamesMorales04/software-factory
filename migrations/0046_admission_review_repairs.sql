-- Bind the exact execution profile to request-scoped admissions created after
-- the review of migration 0043. Existing preview rows default to empty and
-- cannot be consumed by the stricter admission path.
ALTER TABLE subscription_scope_admissions
  ADD COLUMN execution_profile_id TEXT NOT NULL DEFAULT '';

-- Repair the provider-turn child tables after the 0045 parent rebuild. This is
-- idempotent for databases where 0045 already applied and preserves every
-- egress/ledger binding row.
CREATE TABLE provider_turn_egress_requests_repair AS
  SELECT * FROM provider_turn_egress_requests;
CREATE TABLE provider_turn_ledger_requests_repair AS
  SELECT * FROM provider_turn_ledger_requests;
DROP TABLE provider_turn_egress_requests;
DROP TABLE provider_turn_ledger_requests;

CREATE TABLE provider_turn_egress_requests (
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
INSERT INTO provider_turn_egress_requests
  (request_id,reservation_id,sequence,actual_request_sha256,model,path,body_bytes,created_at)
SELECT request_id,reservation_id,sequence,actual_request_sha256,model,path,body_bytes,created_at
FROM provider_turn_egress_requests_repair;
DROP TABLE provider_turn_egress_requests_repair;
CREATE INDEX IF NOT EXISTS provider_turn_egress_reservation_idx
  ON provider_turn_egress_requests(reservation_id, sequence);

CREATE TABLE provider_turn_ledger_requests (
  reservation_id TEXT NOT NULL REFERENCES provider_turn_reservations(reservation_id) ON DELETE RESTRICT,
  sequence INTEGER NOT NULL CHECK (sequence >= 0),
  ledger_reservation_id TEXT NOT NULL UNIQUE REFERENCES inference_reservations(id) ON DELETE RESTRICT,
  created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
  PRIMARY KEY (reservation_id, sequence)
);
INSERT INTO provider_turn_ledger_requests
  (reservation_id,sequence,ledger_reservation_id,created_at_ms)
SELECT reservation_id,sequence,ledger_reservation_id,created_at_ms
FROM provider_turn_ledger_requests_repair;
DROP TABLE provider_turn_ledger_requests_repair;
