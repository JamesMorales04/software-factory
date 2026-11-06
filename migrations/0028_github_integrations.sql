-- GitHub issue integration: encrypted connection, project repository binding,
-- bounded issue snapshots and truthful sync state.
--
-- Tokens are stored only as AES-256-GCM ciphertext produced by the existing
-- ProviderSecretKey mechanism. These tables never contain a plaintext token.
-- Issue bodies are untrusted text and are stored verbatim (bounded) for display;
-- they are not instructions for any agent.

CREATE TABLE IF NOT EXISTS github_connections (
  id TEXT PRIMARY KEY CHECK (id = 'default'),
  login TEXT NOT NULL,
  scopes_json TEXT NOT NULL,
  scopes_reported INTEGER NOT NULL CHECK (scopes_reported IN (0, 1)),
  credential_ciphertext TEXT NOT NULL,
  credential_nonce TEXT NOT NULL,
  credential_fingerprint TEXT NOT NULL,
  enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
  revision INTEGER NOT NULL CHECK (revision > 0),
  verified_at TEXT NOT NULL,
  revoked_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS github_connection_audit (
  audit_id TEXT PRIMARY KEY,
  connection_id TEXT NOT NULL,
  revision INTEGER NOT NULL,
  action TEXT NOT NULL,
  actor TEXT NOT NULL,
  detail TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS github_connection_audit_created_idx
  ON github_connection_audit(created_at);

CREATE TABLE IF NOT EXISTS project_github_bindings (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  owner TEXT NOT NULL,
  repo TEXT NOT NULL,
  repo_full_name TEXT NOT NULL,
  default_branch TEXT NOT NULL,
  bound_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS project_github_bindings_repo_idx
  ON project_github_bindings(repo_full_name);

CREATE TABLE IF NOT EXISTS github_issues (
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  repo_full_name TEXT NOT NULL,
  number INTEGER NOT NULL CHECK (number > 0),
  github_id INTEGER NOT NULL,
  title TEXT NOT NULL,
  body TEXT NOT NULL,
  body_truncated INTEGER NOT NULL CHECK (body_truncated IN (0, 1)),
  state TEXT NOT NULL CHECK (state IN ('open', 'closed')),
  state_reason TEXT,
  labels_json TEXT NOT NULL,
  author TEXT,
  html_url TEXT NOT NULL,
  comments INTEGER NOT NULL CHECK (comments >= 0),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  closed_at TEXT,
  synced_at TEXT NOT NULL,
  sync_mark TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  PRIMARY KEY (project_id, repo_full_name, number)
);

CREATE INDEX IF NOT EXISTS github_issues_project_state_idx
  ON github_issues(project_id, state, updated_at DESC);

CREATE TABLE IF NOT EXISTS github_sync_state (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  repo_full_name TEXT NOT NULL,
  status TEXT NOT NULL CHECK (
    status IN (
      'NEVER_SYNCED',
      'OK',
      'STALE',
      'RATE_LIMITED',
      'DENIED',
      'NOT_FOUND',
      'ERROR',
      'REVOKED'
    )
  ),
  detail TEXT NOT NULL,
  etag TEXT,
  last_synced_at TEXT,
  last_attempt_at TEXT,
  next_allowed_at TEXT,
  last_status_code INTEGER,
  fetched INTEGER NOT NULL DEFAULT 0 CHECK (fetched >= 0),
  created_count INTEGER NOT NULL DEFAULT 0 CHECK (created_count >= 0),
  updated_count INTEGER NOT NULL DEFAULT 0 CHECK (updated_count >= 0),
  removed_count INTEGER NOT NULL DEFAULT 0 CHECK (removed_count >= 0),
  total_known INTEGER NOT NULL DEFAULT 0 CHECK (total_known >= 0),
  truncated INTEGER NOT NULL DEFAULT 0 CHECK (truncated IN (0, 1)),
  stale INTEGER NOT NULL DEFAULT 1 CHECK (stale IN (0, 1)),
  updated_at TEXT NOT NULL
);
