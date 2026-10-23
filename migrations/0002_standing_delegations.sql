-- Reusable, scoped authorization snapshots and atomic cumulative usage.
-- Issuance does not activate autonomous execution; Core policy remains closed.

CREATE TABLE IF NOT EXISTS standing_delegations (
  id TEXT PRIMARY KEY,
  principal_id TEXT NOT NULL,
  audit_id TEXT NOT NULL,
  project_id TEXT NOT NULL REFERENCES projects(id),
  revision INTEGER NOT NULL CHECK (revision > 0),
  content_hash TEXT NOT NULL,
  content_json TEXT NOT NULL,
  revoked_at TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS standing_delegations_project_idx
  ON standing_delegations(project_id);

CREATE TRIGGER IF NOT EXISTS standing_delegations_immutable_update
BEFORE UPDATE ON standing_delegations
WHEN OLD.id != NEW.id
  OR OLD.principal_id != NEW.principal_id
  OR OLD.audit_id != NEW.audit_id
  OR OLD.project_id != NEW.project_id
  OR OLD.revision != NEW.revision
  OR OLD.content_hash != NEW.content_hash
  OR OLD.content_json != NEW.content_json
  OR OLD.created_at != NEW.created_at
BEGIN
  SELECT RAISE(ABORT, 'standing delegation content is immutable');
END;

CREATE TABLE IF NOT EXISTS delegation_usage (
  delegation_id TEXT PRIMARY KEY REFERENCES standing_delegations(id),
  requests INTEGER NOT NULL DEFAULT 0 CHECK (requests >= 0),
  sessions INTEGER NOT NULL DEFAULT 0 CHECK (sessions >= 0),
  attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0)
);
