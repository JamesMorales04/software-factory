-- Owner-authorized revocation of a licence/identity/evidence delegation.
-- The underlying agentic_owner_decisions row is immutable; this append-only
-- record fences new Lead decisions without rewriting the approved decision.
CREATE TABLE IF NOT EXISTS license_delegation_revocations (
  delegation_id TEXT PRIMARY KEY REFERENCES agentic_owner_decisions(id),
  reason TEXT NOT NULL,
  revoked_by TEXT NOT NULL,
  created_at TEXT NOT NULL
);
