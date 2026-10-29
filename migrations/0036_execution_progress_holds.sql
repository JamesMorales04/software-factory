-- Durable Execution Policy V2 progress holds and work-item retirement.
--
-- A deterministic Diagnose/Escalate decision is a campaign-level control state,
-- not a generic failed attempt. While a hold is open, Core refuses to dispatch
-- another attempt for that work item. The hold is cleared only by an
-- authenticated operator decision or by verified external advancement.
--
-- A completed work item may be retired from the active progress projection so
-- a legitimate long Campaign can advance to further WorkGraph units. Retirement
-- preserves historical replay detection: a retired item cannot earn progress
-- again, and its evidence/checkpoint rows remain immutable.

CREATE TABLE IF NOT EXISTS progress_holds (
  hold_id TEXT PRIMARY KEY,
  campaign_id TEXT NOT NULL REFERENCES campaigns(id),
  work_item_id TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('DIAGNOSE', 'ESCALATE', 'BLOCKED')),
  reason TEXT NOT NULL,
  checkpoint_id TEXT REFERENCES progress_checkpoints(checkpoint_id),
  created_at TEXT NOT NULL,
  resolved_at TEXT,
  resolution TEXT CHECK (
    resolution IS NULL OR resolution IN ('OPERATOR', 'VERIFIED_CHANGE', 'CANCELLED')
  ),
  resolved_by TEXT
);

-- At most one unresolved hold per work item. Resolved holds are retained as
-- history, so a later episode inserts a new row.
CREATE UNIQUE INDEX IF NOT EXISTS progress_holds_open_unique
  ON progress_holds(campaign_id, work_item_id) WHERE resolved_at IS NULL;
CREATE INDEX IF NOT EXISTS progress_holds_campaign_idx
  ON progress_holds(campaign_id, created_at, hold_id);

CREATE TRIGGER IF NOT EXISTS progress_holds_immutable_identity
BEFORE UPDATE ON progress_holds
WHEN OLD.hold_id <> NEW.hold_id
  OR OLD.campaign_id <> NEW.campaign_id
  OR OLD.work_item_id <> NEW.work_item_id
  OR OLD.state <> NEW.state
  OR OLD.reason <> NEW.reason
  OR OLD.checkpoint_id IS NOT NEW.checkpoint_id
  OR OLD.created_at <> NEW.created_at
  OR (OLD.resolved_at IS NOT NULL AND NEW.resolved_at IS NOT OLD.resolved_at)
  OR (OLD.resolution IS NOT NULL AND NEW.resolution IS NOT OLD.resolution)
  OR (OLD.resolved_by IS NOT NULL AND NEW.resolved_by IS NOT OLD.resolved_by)
BEGIN
  SELECT RAISE(ABORT, 'progress hold identity is immutable');
END;

CREATE TRIGGER IF NOT EXISTS progress_holds_immutable_delete
BEFORE DELETE ON progress_holds
BEGIN SELECT RAISE(ABORT, 'progress hold history is append-only'); END;

CREATE TABLE IF NOT EXISTS progress_retired_work_items (
  campaign_id TEXT NOT NULL REFERENCES campaigns(id),
  work_item_id TEXT NOT NULL,
  reason TEXT NOT NULL,
  retired_at TEXT NOT NULL,
  PRIMARY KEY (campaign_id, work_item_id)
);

CREATE TRIGGER IF NOT EXISTS progress_retired_work_items_immutable_update
BEFORE UPDATE ON progress_retired_work_items
BEGIN SELECT RAISE(ABORT, 'retired progress work item is immutable'); END;

CREATE TRIGGER IF NOT EXISTS progress_retired_work_items_immutable_delete
BEFORE DELETE ON progress_retired_work_items
BEGIN SELECT RAISE(ABORT, 'retired progress work item is immutable'); END;

-- A retired work item cannot hold open work. Retirement first requires the
-- caller to resolve any hold.
CREATE TRIGGER IF NOT EXISTS progress_retired_work_items_no_open_hold
BEFORE INSERT ON progress_retired_work_items
WHEN EXISTS (
  SELECT 1 FROM progress_holds
  WHERE campaign_id = NEW.campaign_id
    AND work_item_id = NEW.work_item_id
    AND resolved_at IS NULL
)
BEGIN
  SELECT RAISE(ABORT, 'progress work item has an open hold');
END;
