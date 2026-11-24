-- Exact, versioned Lead licence decisions under an active Owner delegation.
-- The delegation is an agentic_owner_decisions row; the Lead actor is bound by
-- Core to the exact Lead request, the SETTLED `license_evidence_review`
-- reservation, the canonical request/output digests and the verified
-- identity-facts receipt. A decision is immutable except for one audited
-- revocation; every binding artefact is persisted so a builder can mark a
-- changed policy/spec/inventory/candidate as STALE instead of re-accepting it.
CREATE TABLE IF NOT EXISTS license_decisions (
  id TEXT PRIMARY KEY,
  delegation_id TEXT NOT NULL REFERENCES agentic_owner_decisions(id),
  project_id TEXT NOT NULL REFERENCES projects(id),
  lead_request_id TEXT NOT NULL,
  lead_reservation_id TEXT NOT NULL,
  lead_actor_sha256 TEXT NOT NULL,
  lead_request_sha256 TEXT NOT NULL,
  lead_output_sha256 TEXT NOT NULL,
  lead_principal TEXT NOT NULL,
  delegation_matter TEXT NOT NULL,
  delegation_option TEXT NOT NULL,
  delegation_option_sha256 TEXT NOT NULL,
  delegation_revision INTEGER NOT NULL CHECK (delegation_revision > 0),
  delegation_pin TEXT NOT NULL,
  delegation_use_context TEXT NOT NULL,
  delegation_criterion TEXT NOT NULL,
  delegation_deadline TEXT NOT NULL,
  identity_json TEXT NOT NULL,
  license_expression TEXT NOT NULL,
  selected_branch TEXT NOT NULL,
  software_obligations_json TEXT NOT NULL,
  documentation_json TEXT NOT NULL,
  documentation_obligations_json TEXT NOT NULL,
  software_verdict TEXT NOT NULL,
  evidence_sha256 TEXT NOT NULL,
  identity_facts_receipt_digest TEXT NOT NULL,
  content_sha256 TEXT NOT NULL,
  provenance_json TEXT NOT NULL,
  use_context TEXT NOT NULL,
  commercial_use INTEGER NOT NULL CHECK (commercial_use IN (0,1)),
  payment_required INTEGER NOT NULL CHECK (payment_required IN (0,1)),
  catalog_sha256 TEXT NOT NULL,
  profile_sha256 TEXT NOT NULL,
  policy_sha256 TEXT NOT NULL,
  stage_json TEXT NOT NULL,
  decision TEXT NOT NULL CHECK (decision IN ('ACCEPT','NEEDS_EVIDENCE','DENY')),
  revision INTEGER NOT NULL CHECK (revision > 0),
  idempotency_key TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL,
  revoked_at TEXT,
  revocation_reason TEXT,
  revoked_by TEXT,
  CHECK (
    (revoked_at IS NULL AND revocation_reason IS NULL AND revoked_by IS NULL)
    OR
    (revoked_at IS NOT NULL AND revocation_reason IS NOT NULL AND revoked_by IS NOT NULL)
  )
);

CREATE INDEX IF NOT EXISTS license_decisions_project_idx
  ON license_decisions(project_id, created_at DESC);

CREATE INDEX IF NOT EXISTS license_decisions_delegation_idx
  ON license_decisions(delegation_id, revoked_at);

-- Append-only revocation audit. The decision row only carries the first
-- revocation marker; this table is the durable evidence of who revoked it, for
-- which project and why. A second, different revocation is refused.
CREATE TABLE IF NOT EXISTS license_decision_revocations (
  decision_id TEXT PRIMARY KEY REFERENCES license_decisions(id),
  project_id TEXT NOT NULL REFERENCES projects(id),
  reason TEXT NOT NULL,
  revoked_by TEXT NOT NULL,
  created_at TEXT NOT NULL
);

-- Decision content is immutable. Only the single audited revocation marker may
-- change, and the append-only revocation table records it. A delete is refused.
CREATE TRIGGER IF NOT EXISTS license_decisions_immutable_content
BEFORE UPDATE ON license_decisions
WHEN OLD.id != NEW.id
  OR OLD.delegation_id != NEW.delegation_id
  OR OLD.project_id != NEW.project_id
  OR OLD.lead_request_id != NEW.lead_request_id
  OR OLD.lead_reservation_id != NEW.lead_reservation_id
  OR OLD.lead_actor_sha256 != NEW.lead_actor_sha256
  OR OLD.lead_request_sha256 != NEW.lead_request_sha256
  OR OLD.lead_output_sha256 != NEW.lead_output_sha256
  OR OLD.lead_principal != NEW.lead_principal
  OR OLD.delegation_matter != NEW.delegation_matter
  OR OLD.delegation_option != NEW.delegation_option
  OR OLD.delegation_option_sha256 != NEW.delegation_option_sha256
  OR OLD.delegation_revision != NEW.delegation_revision
  OR OLD.delegation_pin != NEW.delegation_pin
  OR OLD.delegation_use_context != NEW.delegation_use_context
  OR OLD.delegation_criterion != NEW.delegation_criterion
  OR OLD.delegation_deadline != NEW.delegation_deadline
  OR OLD.identity_json != NEW.identity_json
  OR OLD.license_expression != NEW.license_expression
  OR OLD.selected_branch != NEW.selected_branch
  OR OLD.software_obligations_json != NEW.software_obligations_json
  OR OLD.documentation_json != NEW.documentation_json
  OR OLD.documentation_obligations_json != NEW.documentation_obligations_json
  OR OLD.software_verdict != NEW.software_verdict
  OR OLD.evidence_sha256 != NEW.evidence_sha256
  OR OLD.identity_facts_receipt_digest != NEW.identity_facts_receipt_digest
  OR OLD.content_sha256 != NEW.content_sha256
  OR OLD.provenance_json != NEW.provenance_json
  OR OLD.use_context != NEW.use_context
  OR OLD.commercial_use != NEW.commercial_use
  OR OLD.payment_required != NEW.payment_required
  OR OLD.catalog_sha256 != NEW.catalog_sha256
  OR OLD.profile_sha256 != NEW.profile_sha256
  OR OLD.policy_sha256 != NEW.policy_sha256
  OR OLD.stage_json != NEW.stage_json
  OR OLD.decision != NEW.decision
  OR OLD.revision != NEW.revision
  OR OLD.idempotency_key != NEW.idempotency_key
  OR OLD.created_at != NEW.created_at
  OR (OLD.revoked_at IS NOT NULL AND OLD.revoked_at != NEW.revoked_at)
BEGIN
  SELECT RAISE(ABORT, 'licence decision content is immutable');
END;

CREATE TRIGGER IF NOT EXISTS license_decisions_immutable_delete
BEFORE DELETE ON license_decisions
BEGIN
  SELECT RAISE(ABORT, 'licence decisions are immutable');
END;
