-- Intake follow-up lineage.
--
-- A terminal unified-intake request may receive one additional authenticated
-- operator message that starts a new follow-up request. The new request is a
-- fresh Action/Lead/Campaign path; it never reuses the parent frozen plan,
-- specification, candidate or Campaign. The lineage is recorded on the child
-- only, with the exact source message, so replay can return the same follow-up
-- instead of creating another one.
--
-- Existing rows keep NULL lineage and remain valid historical requests.

ALTER TABLE intake_requests ADD COLUMN parent_request_id TEXT REFERENCES intake_requests(id);
ALTER TABLE intake_requests ADD COLUMN source_message_id TEXT REFERENCES intake_messages(id);

CREATE INDEX IF NOT EXISTS intake_requests_parent_idx
  ON intake_requests(parent_request_id);

-- A terminal request may start at most one follow-up. The follow-up itself can
-- later complete and start its own follow-up, forming a linear thread. The
-- source message is unique so an idempotent message replay always resolves to
-- the same child.
CREATE UNIQUE INDEX IF NOT EXISTS intake_requests_source_message_idx
  ON intake_requests(source_message_id)
  WHERE source_message_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS intake_requests_parent_follow_up_idx
  ON intake_requests(parent_request_id)
  WHERE parent_request_id IS NOT NULL;

-- Parent/source lineage is immutable like the rest of the submission snapshot.
CREATE TRIGGER IF NOT EXISTS intake_requests_immutable_follow_up
BEFORE UPDATE ON intake_requests
WHEN OLD.parent_request_id IS NOT NEW.parent_request_id
  OR OLD.source_message_id IS NOT NEW.source_message_id
BEGIN
  SELECT RAISE(ABORT, 'intake request follow-up lineage is immutable');
END;
