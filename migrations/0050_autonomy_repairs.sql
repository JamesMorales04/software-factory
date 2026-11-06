-- Bounded autonomous repair authorization records.
--
-- Findings are advisory model output; Core recomputes canonical points,
-- critical-zero, risk budgets and reviewer independence before inserting a
-- row. Rows are append-only evidence and never grant merge/delivery authority.
CREATE TABLE IF NOT EXISTS autonomy_repairs (
  repair_id TEXT PRIMARY KEY,
  campaign_id TEXT NOT NULL REFERENCES campaigns(id),
  candidate_id TEXT NOT NULL REFERENCES candidates(id),
  work_item_id TEXT NOT NULL,
  repair_trigger TEXT NOT NULL CHECK (repair_trigger IN ('QUALITY_GATE','PROGRESS_STAGNATION')),
  risk_class TEXT NOT NULL,
  total_points INTEGER NOT NULL CHECK (total_points >= 0),
  critical_count INTEGER NOT NULL CHECK (critical_count >= 0),
  blocking_count INTEGER NOT NULL CHECK (blocking_count >= 0),
  warning_count INTEGER NOT NULL CHECK (warning_count >= 0),
  info_count INTEGER NOT NULL CHECK (info_count >= 0),
  cycle_index INTEGER NOT NULL CHECK (cycle_index >= 0),
  decision TEXT NOT NULL CHECK (decision IN ('AUTHORIZED','DENIED','HUMAN_REQUIRED')),
  reason_code TEXT NOT NULL,
  reason TEXT NOT NULL,
  policy_sha256 TEXT NOT NULL,
  reviewer_receipt_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (candidate_id, repair_trigger)
);
CREATE INDEX IF NOT EXISTS autonomy_repairs_campaign_idx
  ON autonomy_repairs(campaign_id, work_item_id, created_at);
CREATE TRIGGER IF NOT EXISTS autonomy_repairs_immutable_update
BEFORE UPDATE ON autonomy_repairs
BEGIN SELECT RAISE(ABORT, 'autonomy repair decision is immutable'); END;
CREATE TRIGGER IF NOT EXISTS autonomy_repairs_immutable_delete
BEFORE DELETE ON autonomy_repairs
BEGIN SELECT RAISE(ABORT, 'autonomy repair decision is immutable'); END;
