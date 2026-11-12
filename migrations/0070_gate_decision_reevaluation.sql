-- Disposition-driven Gate re-evaluation.
--
-- The first Gate decision stays immutable in `gate_decisions`. A deterministic
-- re-evaluation after an authenticated finding disposition appends one row here
-- only when the recomputed outcome or reasons differ. `gate_decisions_current`
-- is the effective projection read by Core, Campaign and Office queries.

CREATE TABLE IF NOT EXISTS gate_decision_revisions (
  revision_id INTEGER PRIMARY KEY AUTOINCREMENT,
  candidate_id TEXT NOT NULL REFERENCES candidates(id),
  outcome TEXT NOT NULL CHECK (outcome IN (
    'ACCEPTED', 'REPAIR_REQUIRED', 'BLOCKED', 'HUMAN_REQUIRED', 'NOT_TESTED'
  )),
  reasons_json TEXT NOT NULL,
  decided_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS gate_decision_revisions_candidate_idx
  ON gate_decision_revisions(candidate_id, revision_id DESC);

CREATE TRIGGER IF NOT EXISTS gate_decision_revisions_immutable_update
BEFORE UPDATE ON gate_decision_revisions
BEGIN SELECT RAISE(ABORT, 'gate decision revision is immutable'); END;

CREATE TRIGGER IF NOT EXISTS gate_decision_revisions_immutable_delete
BEFORE DELETE ON gate_decision_revisions
BEGIN SELECT RAISE(ABORT, 'gate decision revision is immutable'); END;

CREATE VIEW IF NOT EXISTS gate_decisions_current AS
SELECT r.candidate_id, r.outcome, r.reasons_json, r.decided_at
FROM gate_decision_revisions r
WHERE r.revision_id = (
  SELECT MAX(r2.revision_id)
  FROM gate_decision_revisions r2
  WHERE r2.candidate_id = r.candidate_id
)
UNION ALL
SELECT g.candidate_id, g.outcome, g.reasons_json, g.decided_at
FROM gate_decisions g
WHERE NOT EXISTS (
  SELECT 1 FROM gate_decision_revisions r
  WHERE r.candidate_id = g.candidate_id
);
