-- Project onboarding and evidence-based toolchain discovery.
--
-- A Project keeps its canonical repository identity while recording an
-- explicit, human-readable source. `source_key` is the duplicate-identity
-- guard for a GitHub owner/repo pair or a canonical managed local path; it is
-- unique when present so the same source cannot be registered twice with
-- different idempotency keys. Existing rows keep empty defaults until Core
-- re-registers or an operator inspects them.

ALTER TABLE projects ADD COLUMN name TEXT NOT NULL DEFAULT '';
ALTER TABLE projects ADD COLUMN source_kind TEXT NOT NULL DEFAULT 'local'
  CHECK (source_kind IN ('local', 'git', 'github'));
ALTER TABLE projects ADD COLUMN source_url TEXT NOT NULL DEFAULT '';
ALTER TABLE projects ADD COLUMN source_key TEXT NOT NULL DEFAULT '';
ALTER TABLE projects ADD COLUMN source_visibility TEXT NOT NULL DEFAULT ''
  CHECK (source_visibility IN ('', 'public', 'private'));
ALTER TABLE projects ADD COLUMN default_branch TEXT NOT NULL DEFAULT '';
ALTER TABLE projects ADD COLUMN remote_origin TEXT NOT NULL DEFAULT '';

CREATE UNIQUE INDEX IF NOT EXISTS projects_source_key_unique
  ON projects(source_key) WHERE source_key <> '';

-- Persisted discovery evidence is bound to the exact commit that was scanned.
-- The JSON payload is advisory metadata; it never grants a capability or check
-- authority beyond the Project configuration an operator explicitly saves.
CREATE TABLE IF NOT EXISTS project_discoveries (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  base_commit TEXT NOT NULL,
  discovery_json TEXT NOT NULL,
  scanned_at TEXT NOT NULL
);
