-- Versioned issue-intake policy writes.
--
-- The frozen 0033 revision table has NOT NULL legacy attempt/session/turn/
-- wall-clock columns. They remain the V1 evidence representation. New V2 policy
-- revisions omit these inputs from their API and store the fixed storage
-- placeholders below; the V2 execution delegation is derived from the frozen
-- Execution Profile operation/progress policy instead. A trigger makes the
-- placeholders explicit and prevents a V2 row from smuggling a legacy cap.
--
-- A future FK-safe rebuild (outside SQLx's implicit migration transaction) may
-- drop the columns; until then this is an explicit, constrained storage shim.

ALTER TABLE issue_intake_policy_revisions
  ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1
  CHECK (schema_version IN (1, 2));

CREATE TRIGGER IF NOT EXISTS issue_intake_policy_v2_placeholders
BEFORE INSERT ON issue_intake_policy_revisions
WHEN NEW.schema_version = 2
  AND (NEW.max_attempts <> 1
       OR NEW.max_sessions <> 1
       OR NEW.max_turns <> 1
       OR NEW.max_wall_clock_secs <> 1)
BEGIN
  SELECT RAISE(ABORT, 'V2 issue policy must not carry legacy campaign caps');
END;
