-- Exact, versioned Lead licence decisions under an active Owner delegation.
-- The delegation is an agentic_owner_decisions row; the Lead actor is bound by
-- Core to the exact request, SETTLED license_evidence_review reservation and
-- model-output digest. A decision is immutable; revocation is recorded.
CREATE TABLE IF NOT EXISTS license_decisions (
  id TEXT PRIMARY KEY,
  delegation_id TEXT NOT NULL REFERENCES agentic_owner_decisions(id),
  project_id TEXT NOT NULL REFERENCES projects(id),
  lead_request_id TEXT NOT NULL,
  lead_reservation_id TEXT NOT NULL,
  lead_actor_sha256 TEXT NOT NULL,
  identity_json TEXT NOT NULL,
  license_expression TEXT NOT NULL,
  selected_branch TEXT NOT NULL,
  documentation_license TEXT,
  evidence_sha256 TEXT NOT NULL,
  provenance_json TEXT NOT NULL,
  use_context TEXT NOT NULL,
  commercial_use INTEGER NOT NULL CHECK (commercial_use IN (0,1)),
  payment_required INTEGER NOT NULL CHECK (payment_required IN (0,1)),
  obligations_json TEXT NOT NULL,
  decision TEXT NOT NULL CHECK (decision IN ('ACCEPT','NEEDS_EVIDENCE','DENY')),
  revision INTEGER NOT NULL CHECK (revision > 0),
  idempotency_key TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL,
  revoked_at TEXT,
  revocation_reason TEXT
);

CREATE INDEX IF NOT EXISTS license_decisions_project_idx
  ON license_decisions(project_id, created_at DESC);

CREATE INDEX IF NOT EXISTS license_decisions_delegation_idx
  ON license_decisions(delegation_id, revoked_at);
