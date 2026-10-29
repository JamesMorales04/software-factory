-- Repair migration for the shared inference ledger (0035 was already released
-- to development databases and must never be edited in place). This migration
-- is additive except for replacing the reservation state-transition trigger,
-- which is intentional and done with DROP/CREATE under the same name.
--
-- Every object here was introduced by the repair review: one-to-one campaign
-- binding, trusted pricing quotes, immutable dispatch identity, legal state
-- transitions, V1/V2 policy discriminators, and per-request provider-turn
-- ledger mappings.

-- One envelope may back exactly one Campaign. The original 0035 created a
-- non-unique lookup index with this name, so replace it with the constraint.
DROP INDEX IF EXISTS inference_campaign_bindings_envelope_idx;
CREATE UNIQUE INDEX IF NOT EXISTS inference_campaign_bindings_envelope_idx
  ON inference_campaign_bindings(envelope_id);

-- Frozen per-request price ceiling. Rows are created by Core from provider
-- settings and the adapter's trusted billing-coverage declaration. Callers
-- cannot supply a bound, coverage flags or a snapshot.
CREATE TABLE IF NOT EXISTS inference_pricing_quotes (
  quote_id TEXT PRIMARY KEY,
  provider TEXT NOT NULL CHECK (length(provider) > 0),
  model TEXT NOT NULL CHECK (length(model) > 0),
  revision INTEGER NOT NULL CHECK (revision > 0),
  currency TEXT NOT NULL CHECK (currency = 'USD'),
  upper_bound_micros INTEGER NOT NULL CHECK (upper_bound_micros > 0),
  coverage_sha256 TEXT NOT NULL CHECK (length(coverage_sha256) = 64),
  coverage_json TEXT NOT NULL CHECK (length(coverage_json) > 0),
  snapshot_json TEXT NOT NULL CHECK (length(snapshot_json) > 0),
  evidence_sha256 TEXT NOT NULL CHECK (length(evidence_sha256) = 64),
  created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
  UNIQUE (provider, model, revision)
);

CREATE INDEX IF NOT EXISTS inference_pricing_quotes_route_idx
  ON inference_pricing_quotes(provider, model, revision DESC);

CREATE TRIGGER IF NOT EXISTS inference_pricing_quotes_immutable_update
BEFORE UPDATE ON inference_pricing_quotes
BEGIN
  SELECT RAISE(ABORT, 'inference pricing quote is immutable');
END;

CREATE TRIGGER IF NOT EXISTS inference_pricing_quotes_immutable_delete
BEFORE DELETE ON inference_pricing_quotes
BEGIN
  SELECT RAISE(ABORT, 'inference pricing quote is immutable');
END;

-- Replace the original transition trigger with an allow-list transition
-- matrix. SETTLED and RELEASED are terminal; AMBIGUOUS may only settle or be
-- reconciled as released.
DROP TRIGGER IF EXISTS inference_reservations_terminal_state;
CREATE TRIGGER IF NOT EXISTS inference_reservations_terminal_state
BEFORE UPDATE ON inference_reservations
WHEN (OLD.state IN ('SETTLED', 'RELEASED') AND NEW.state <> OLD.state)
  OR (OLD.state = 'SETTLED' AND
      (NEW.settled_micros IS NOT OLD.settled_micros
       OR NEW.settlement_sha256 IS NOT OLD.settlement_sha256))
  OR (OLD.state = 'AMBIGUOUS' AND NEW.state = 'RESERVED')
  OR (OLD.state = 'RESERVED' AND NEW.state = 'RESERVED'
      AND OLD.dispatched_at_ms IS NOT NULL AND NEW.dispatched_at_ms IS NULL)
  OR (NEW.state <> OLD.state AND NOT (
       (OLD.state = 'RESERVED' AND NEW.state IN ('SETTLED', 'RELEASED', 'AMBIGUOUS'))
       OR (OLD.state = 'AMBIGUOUS' AND NEW.state IN ('SETTLED', 'RELEASED'))
     ))
BEGIN
  SELECT RAISE(ABORT, 'inference reservation state transition is not allowed');
END;

-- Dispatch identity is recorded once, before any upstream effect, and is never
-- rewritten. Worker/generation may be set at reservation time and must match
-- dispatch; they cannot be replaced later.
CREATE TRIGGER IF NOT EXISTS inference_reservations_dispatch_immutable
BEFORE UPDATE ON inference_reservations
WHEN (OLD.dispatched_at_ms IS NOT NULL AND (
        NEW.dispatched_at_ms IS NOT OLD.dispatched_at_ms
        OR NEW.worker_id IS NOT OLD.worker_id
        OR NEW.generation IS NOT OLD.generation
        OR NEW.provider_request_id IS NOT OLD.provider_request_id))
  OR (OLD.worker_id IS NOT NULL AND NEW.worker_id IS NOT OLD.worker_id)
  OR (OLD.generation IS NOT NULL AND NEW.generation IS NOT OLD.generation)
BEGIN
  SELECT RAISE(ABORT, 'inference dispatch identity is immutable');
END;

-- One provider turn may group multiple billable egress requests. Each request
-- has its own ledger hold; this table maps the legacy turn/sequence to the
-- authoritative reservation without double charging.
CREATE TABLE IF NOT EXISTS provider_turn_ledger_requests (
  reservation_id TEXT NOT NULL REFERENCES provider_turn_reservations(reservation_id) ON DELETE RESTRICT,
  sequence INTEGER NOT NULL CHECK (sequence >= 0),
  ledger_reservation_id TEXT NOT NULL UNIQUE REFERENCES inference_reservations(id) ON DELETE RESTRICT,
  created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
  PRIMARY KEY (reservation_id, sequence)
);

-- Explicit per-Campaign V2 classification. Absence is historical V1 only
-- while the global policy remains V1; a global V2 cutover makes absence fail
-- closed. Binding an envelope inserts the one-way V2 classification.
CREATE TABLE IF NOT EXISTS inference_campaign_modes (
  campaign_id TEXT PRIMARY KEY REFERENCES campaigns(id) ON DELETE RESTRICT,
  mode TEXT NOT NULL CHECK (mode IN ('V1', 'V2')),
  classified_at_ms INTEGER NOT NULL CHECK (classified_at_ms > 0),
  evidence_sha256 TEXT NOT NULL CHECK (length(evidence_sha256) = 64)
);

CREATE TRIGGER IF NOT EXISTS inference_campaign_modes_one_way
BEFORE UPDATE ON inference_campaign_modes
WHEN OLD.mode = 'V2' AND NEW.mode <> 'V2'
BEGIN
  SELECT RAISE(ABORT, 'campaign inference mode cannot return to V1');
END;

CREATE TRIGGER IF NOT EXISTS inference_campaign_modes_immutable_delete
BEFORE DELETE ON inference_campaign_modes
BEGIN
  SELECT RAISE(ABORT, 'campaign inference mode is never deleted');
END;

-- Global execution cutover discriminator. V1 is the historical mode; once the
-- Director promotes execution to V2, a billable call without a bound envelope
-- fails closed everywhere.
CREATE TABLE IF NOT EXISTS inference_ledger_policy (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  mode TEXT NOT NULL CHECK (mode IN ('V1', 'V2')),
  actor TEXT NOT NULL,
  evidence_sha256 TEXT NOT NULL CHECK (length(evidence_sha256) = 64),
  updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0)
);

INSERT OR IGNORE INTO inference_ledger_policy(id,mode,actor,evidence_sha256,updated_at_ms)
VALUES (1, 'V1', 'migration:0037',
        '0000000000000000000000000000000000000000000000000000000000000000',
        CAST(strftime('%s','now') AS INTEGER) * 1000);

CREATE TRIGGER IF NOT EXISTS inference_ledger_policy_one_way
BEFORE UPDATE ON inference_ledger_policy
WHEN OLD.mode = 'V2' AND NEW.mode <> 'V2'
BEGIN
  SELECT RAISE(ABORT, 'inference ledger policy cannot return to V1');
END;

-- Model-call dispatch intent. The link column was added by 0035; these two
-- columns make the legacy evidence row carry the same pre-send intent.
ALTER TABLE model_call_reservations ADD COLUMN dispatched_at_ms INTEGER
  CHECK (dispatched_at_ms IS NULL OR dispatched_at_ms > 0);
ALTER TABLE model_call_reservations ADD COLUMN provider_request_id TEXT;
