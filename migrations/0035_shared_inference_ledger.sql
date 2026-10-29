-- Shared campaign inference ledger (Execution Policy V2, Lane L).
--
-- One monetary envelope is created for an operator request before the first
-- billable assessment. Campaigns, subagents, repairs and reviewers inherit that
-- envelope; a second allowance is never created by campaign attachment. All
-- amounts are integer USD micros. Identity fields, request digests, pricing
-- quotes and original bounds are immutable; authorization and event rows are
-- append-only. A reservation state transition is the only mutable financial
-- record.
--
-- Existing model-call and provider-turn reservations remain evidence rows. A
-- nullable unique link records the authoritative ledger reservation for new,
-- envelope-bound operations. Legacy rows are never financially backfilled.

CREATE TABLE IF NOT EXISTS inference_envelopes (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL UNIQUE,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
  currency TEXT NOT NULL CHECK (currency = 'USD'),
  state TEXT NOT NULL CHECK (state IN ('ACTIVE', 'REVOKED', 'CLOSED', 'OVERDRAWN')),
  authorized_micros INTEGER NOT NULL CHECK (authorized_micros >= 0),
  authorization_revision INTEGER NOT NULL CHECK (authorization_revision > 0),
  created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
  revoked_at_ms INTEGER CHECK (revoked_at_ms IS NULL OR revoked_at_ms > 0)
);

CREATE INDEX IF NOT EXISTS inference_envelopes_project_idx
  ON inference_envelopes(project_id, created_at_ms DESC);

CREATE TRIGGER IF NOT EXISTS inference_envelopes_immutable_identity
BEFORE UPDATE ON inference_envelopes
WHEN OLD.id <> NEW.id
  OR OLD.request_id <> NEW.request_id
  OR OLD.project_id <> NEW.project_id
  OR OLD.currency <> NEW.currency
  OR OLD.created_at_ms <> NEW.created_at_ms
  OR (OLD.state IN ('REVOKED', 'CLOSED') AND NEW.state <> OLD.state)
  OR (OLD.revoked_at_ms IS NOT NULL AND NEW.revoked_at_ms IS NULL)
BEGIN
  SELECT RAISE(ABORT, 'inference envelope identity is immutable');
END;

CREATE TRIGGER IF NOT EXISTS inference_envelopes_immutable_delete
BEFORE DELETE ON inference_envelopes
BEGIN
  SELECT RAISE(ABORT, 'inference envelope is never deleted');
END;

CREATE TABLE IF NOT EXISTS inference_campaign_bindings (
  campaign_id TEXT PRIMARY KEY REFERENCES campaigns(id) ON DELETE RESTRICT,
  envelope_id TEXT NOT NULL REFERENCES inference_envelopes(id) ON DELETE RESTRICT,
  bound_at_ms INTEGER NOT NULL CHECK (bound_at_ms > 0)
);

CREATE INDEX IF NOT EXISTS inference_campaign_bindings_envelope_idx
  ON inference_campaign_bindings(envelope_id);

CREATE TRIGGER IF NOT EXISTS inference_campaign_bindings_immutable_update
BEFORE UPDATE ON inference_campaign_bindings
BEGIN
  SELECT RAISE(ABORT, 'inference campaign binding is immutable');
END;

CREATE TRIGGER IF NOT EXISTS inference_campaign_bindings_immutable_delete
BEFORE DELETE ON inference_campaign_bindings
BEGIN
  SELECT RAISE(ABORT, 'inference campaign binding is never deleted');
END;

CREATE TABLE IF NOT EXISTS inference_authorizations (
  id TEXT PRIMARY KEY,
  envelope_id TEXT NOT NULL REFERENCES inference_envelopes(id) ON DELETE RESTRICT,
  revision INTEGER NOT NULL CHECK (revision > 0),
  limit_micros INTEGER NOT NULL CHECK (limit_micros >= 0),
  actor TEXT NOT NULL CHECK (length(actor) > 0),
  scope_sha256 TEXT NOT NULL CHECK (length(scope_sha256) = 64),
  evidence_sha256 TEXT NOT NULL CHECK (length(evidence_sha256) = 64),
  idempotency_key TEXT NOT NULL UNIQUE CHECK (length(idempotency_key) > 0),
  created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
  UNIQUE (envelope_id, revision)
);

CREATE INDEX IF NOT EXISTS inference_authorizations_envelope_idx
  ON inference_authorizations(envelope_id, revision DESC);

CREATE TRIGGER IF NOT EXISTS inference_authorizations_immutable_update
BEFORE UPDATE ON inference_authorizations
BEGIN
  SELECT RAISE(ABORT, 'inference authorization is append-only');
END;

CREATE TRIGGER IF NOT EXISTS inference_authorizations_immutable_delete
BEFORE DELETE ON inference_authorizations
BEGIN
  SELECT RAISE(ABORT, 'inference authorization is append-only');
END;

CREATE TABLE IF NOT EXISTS inference_reservations (
  id TEXT PRIMARY KEY,
  envelope_id TEXT NOT NULL REFERENCES inference_envelopes(id) ON DELETE RESTRICT,
  operation_id TEXT NOT NULL CHECK (length(operation_id) > 0),
  idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) > 0),
  request_sha256 TEXT NOT NULL CHECK (length(request_sha256) = 64),
  role TEXT NOT NULL CHECK
    (role IN ('LEAD', 'IMPLEMENTER', 'SEMANTIC_REVIEWER', 'EVALUATION', 'AUXILIARY')),
  principal_id TEXT NOT NULL CHECK (length(principal_id) > 0),
  parent_operation_id TEXT,
  provider TEXT NOT NULL CHECK (length(provider) > 0),
  model TEXT NOT NULL CHECK (length(model) > 0),
  pricing_revision TEXT NOT NULL CHECK (length(pricing_revision) > 0),
  pricing_snapshot_json TEXT NOT NULL CHECK (length(pricing_snapshot_json) > 0),
  upper_bound_micros INTEGER NOT NULL CHECK (upper_bound_micros >= 0),
  settled_micros INTEGER CHECK (settled_micros IS NULL OR settled_micros >= 0),
  state TEXT NOT NULL CHECK (state IN ('RESERVED', 'SETTLED', 'RELEASED', 'AMBIGUOUS')),
  worker_id TEXT,
  generation INTEGER CHECK (generation IS NULL OR generation > 0),
  dispatched_at_ms INTEGER CHECK (dispatched_at_ms IS NULL OR dispatched_at_ms > 0),
  provider_request_id TEXT,
  expires_at_ms INTEGER NOT NULL CHECK (expires_at_ms > 0),
  settlement_sha256 TEXT CHECK (settlement_sha256 IS NULL OR length(settlement_sha256) = 64),
  created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
  updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
  CHECK ((state = 'SETTLED') = (settled_micros IS NOT NULL)),
  UNIQUE (envelope_id, idempotency_key),
  UNIQUE (envelope_id, operation_id)
);

CREATE INDEX IF NOT EXISTS inference_reservations_envelope_state_idx
  ON inference_reservations(envelope_id, state);

CREATE INDEX IF NOT EXISTS inference_reservations_state_expiry_idx
  ON inference_reservations(state, expires_at_ms);

CREATE TRIGGER IF NOT EXISTS inference_reservations_identity_immutable
BEFORE UPDATE ON inference_reservations
WHEN OLD.id <> NEW.id
  OR OLD.envelope_id <> NEW.envelope_id
  OR OLD.operation_id <> NEW.operation_id
  OR OLD.idempotency_key <> NEW.idempotency_key
  OR OLD.request_sha256 <> NEW.request_sha256
  OR OLD.role <> NEW.role
  OR OLD.principal_id <> NEW.principal_id
  OR OLD.parent_operation_id IS NOT NEW.parent_operation_id
  OR OLD.provider <> NEW.provider
  OR OLD.model <> NEW.model
  OR OLD.pricing_revision <> NEW.pricing_revision
  OR OLD.pricing_snapshot_json <> NEW.pricing_snapshot_json
  OR OLD.upper_bound_micros <> NEW.upper_bound_micros
  OR OLD.expires_at_ms <> NEW.expires_at_ms
  OR OLD.created_at_ms <> NEW.created_at_ms
BEGIN
  SELECT RAISE(ABORT, 'inference reservation identity and bound are immutable');
END;

CREATE TRIGGER IF NOT EXISTS inference_reservations_terminal_state
BEFORE UPDATE ON inference_reservations
WHEN (OLD.state IN ('SETTLED', 'RELEASED') AND NEW.state <> OLD.state)
  OR (OLD.state = 'SETTLED' AND
      (NEW.settled_micros IS NOT OLD.settled_micros
       OR NEW.settlement_sha256 IS NOT OLD.settlement_sha256))
  OR (OLD.state = 'AMBIGUOUS' AND NEW.state = 'RESERVED')
  OR (OLD.state = 'RESERVED' AND NEW.state = 'RESERVED'
      AND OLD.dispatched_at_ms IS NOT NULL AND NEW.dispatched_at_ms IS NULL)
BEGIN
  SELECT RAISE(ABORT, 'inference reservation state transition is not allowed');
END;

CREATE TRIGGER IF NOT EXISTS inference_reservations_immutable_delete
BEFORE DELETE ON inference_reservations
BEGIN
  SELECT RAISE(ABORT, 'inference reservation is never deleted');
END;

CREATE TABLE IF NOT EXISTS inference_ledger_events (
  id TEXT PRIMARY KEY,
  envelope_id TEXT NOT NULL REFERENCES inference_envelopes(id) ON DELETE RESTRICT,
  reservation_id TEXT REFERENCES inference_reservations(id) ON DELETE RESTRICT,
  kind TEXT NOT NULL CHECK (length(kind) > 0),
  actor TEXT NOT NULL CHECK (length(actor) > 0),
  evidence_ref TEXT NOT NULL CHECK (length(evidence_ref) > 0),
  payload_sha256 TEXT NOT NULL CHECK (length(payload_sha256) = 64),
  created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0)
);

CREATE INDEX IF NOT EXISTS inference_ledger_events_envelope_idx
  ON inference_ledger_events(envelope_id, created_at_ms, id);

CREATE TRIGGER IF NOT EXISTS inference_ledger_events_immutable_update
BEFORE UPDATE ON inference_ledger_events
BEGIN
  SELECT RAISE(ABORT, 'inference ledger event is append-only');
END;

CREATE TRIGGER IF NOT EXISTS inference_ledger_events_immutable_delete
BEFORE DELETE ON inference_ledger_events
BEGIN
  SELECT RAISE(ABORT, 'inference ledger event is append-only');
END;

-- Link the existing per-call and per-turn reservation evidence to the shared
-- ledger. NULL keeps legacy-only rows exactly as they were; the partial unique
-- index guarantees one legacy row cannot consume the same ledger hold twice.
ALTER TABLE model_call_reservations ADD COLUMN ledger_reservation_id TEXT
  REFERENCES inference_reservations(id) ON DELETE RESTRICT;

CREATE UNIQUE INDEX IF NOT EXISTS model_call_ledger_reservation_idx
  ON model_call_reservations(ledger_reservation_id)
  WHERE ledger_reservation_id IS NOT NULL;

ALTER TABLE provider_turn_reservations ADD COLUMN ledger_reservation_id TEXT
  REFERENCES inference_reservations(id) ON DELETE RESTRICT;

CREATE UNIQUE INDEX IF NOT EXISTS provider_turn_ledger_reservation_idx
  ON provider_turn_reservations(ledger_reservation_id)
  WHERE ledger_reservation_id IS NOT NULL;
