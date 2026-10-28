-- Versioned, Core-owned Execution Profiles.
--
-- A profile is operator configuration. It is not an authorization, budget
-- grant or capability. Every edit writes an immutable revision row; the
-- profile row points at its current revision. Defaults bind a profile, not a
-- revision, so the effective resolution always reports the current revision.
-- At most one global default and one default per project are enforced by
-- partial unique indexes.

CREATE TABLE IF NOT EXISTS execution_profiles (
  id TEXT PRIMARY KEY,
  current_revision INTEGER NOT NULL CHECK (current_revision > 0),
  content_hash TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS execution_profile_revisions (
  revision_id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL REFERENCES execution_profiles(id),
  revision INTEGER NOT NULL CHECK (revision > 0),
  content_json TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  actor TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (profile_id, revision)
);
CREATE INDEX IF NOT EXISTS execution_profile_revisions_profile_idx
  ON execution_profile_revisions(profile_id, revision);

CREATE TRIGGER IF NOT EXISTS execution_profile_revisions_immutable_update
BEFORE UPDATE ON execution_profile_revisions
BEGIN SELECT RAISE(ABORT, 'ExecutionProfileRevision is immutable'); END;

CREATE TRIGGER IF NOT EXISTS execution_profile_revisions_immutable_delete
BEFORE DELETE ON execution_profile_revisions
BEGIN SELECT RAISE(ABORT, 'ExecutionProfileRevision is immutable'); END;

CREATE TABLE IF NOT EXISTS execution_profile_defaults (
  scope TEXT NOT NULL CHECK (scope IN ('global', 'project')),
  project_id TEXT REFERENCES projects(id),
  profile_id TEXT NOT NULL REFERENCES execution_profiles(id),
  actor TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (
    (scope = 'global' AND project_id IS NULL)
    OR (scope = 'project' AND project_id IS NOT NULL)
  )
);
CREATE UNIQUE INDEX IF NOT EXISTS execution_profile_defaults_global_idx
  ON execution_profile_defaults(scope) WHERE scope = 'global';
CREATE UNIQUE INDEX IF NOT EXISTS execution_profile_defaults_project_idx
  ON execution_profile_defaults(project_id) WHERE scope = 'project';
CREATE INDEX IF NOT EXISTS execution_profile_defaults_profile_idx
  ON execution_profile_defaults(profile_id);

CREATE TABLE IF NOT EXISTS execution_profile_audit (
  audit_id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL,
  revision INTEGER,
  action TEXT NOT NULL,
  actor TEXT NOT NULL,
  detail TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS execution_profile_audit_profile_idx
  ON execution_profile_audit(profile_id, created_at);
