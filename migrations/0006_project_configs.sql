-- Immutable project configuration revisions used to bind Lead planning.
CREATE TABLE IF NOT EXISTS project_configs (
  project_id TEXT PRIMARY KEY REFERENCES projects(id),
  revision INTEGER NOT NULL CHECK (revision > 0),
  config_hash TEXT NOT NULL,
  repository TEXT NOT NULL,
  base_commit TEXT NOT NULL,
  allowed_paths_json TEXT NOT NULL,
  capabilities_json TEXT NOT NULL,
  quality_profile TEXT NOT NULL,
  active INTEGER NOT NULL CHECK (active IN (0, 1)),
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS project_configs_hash_idx ON project_configs(config_hash);
