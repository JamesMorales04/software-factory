-- Auditable provenance for how a Candidate was finalized.
--
-- MODEL_FINAL is the normal provider final path. STAGNATION_SALVAGE is the
-- deterministic Worker completion after Core returned Diagnose for an unchanged
-- Implementer that had already written the workspace and where every frozen
-- check passes. The migration never rewrites history: existing candidates keep
-- the safe MODEL_FINAL default because no durable salvage evidence exists for
-- them.
ALTER TABLE candidates
  ADD COLUMN final_source TEXT NOT NULL DEFAULT 'MODEL_FINAL'
  CHECK (final_source IN ('MODEL_FINAL', 'STAGNATION_SALVAGE'));

CREATE INDEX IF NOT EXISTS candidates_final_source_idx
  ON candidates(final_source, created_at);

-- Durable salvage prerequisites. Existing receipts/checkpoints keep empty/zero
-- values and therefore can never satisfy a salvage claim.
ALTER TABLE execution_check_receipts ADD COLUMN check_id TEXT NOT NULL DEFAULT '';
ALTER TABLE execution_check_receipts ADD COLUMN candidate_commit_sha TEXT NOT NULL DEFAULT '';
ALTER TABLE execution_check_receipts ADD COLUMN candidate_tree_sha TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS execution_check_receipts_candidate_idx
  ON execution_check_receipts(candidate_commit_sha, candidate_tree_sha, attempt_id);

ALTER TABLE progress_checkpoints ADD COLUMN attempt_id TEXT NOT NULL DEFAULT '';
ALTER TABLE progress_checkpoints ADD COLUMN stagnant_checkpoints INTEGER NOT NULL DEFAULT 0
  CHECK (stagnant_checkpoints >= 0);
ALTER TABLE progress_checkpoints ADD COLUMN unsuccessful_replans INTEGER NOT NULL DEFAULT 0
  CHECK (unsuccessful_replans >= 0);

CREATE INDEX IF NOT EXISTS progress_checkpoints_attempt_idx
  ON progress_checkpoints(attempt_id, work_item_id, decision);
