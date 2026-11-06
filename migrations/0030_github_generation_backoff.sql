-- GitHub integration hardening: credential-wide rate-limit backoff and
-- per-page issue ETags. Additive; existing connection, binding, snapshot and
-- sync state rows are preserved.

ALTER TABLE github_connections ADD COLUMN rate_limited_until TEXT;

UPDATE github_connections
SET rate_limited_until = (
  SELECT next_allowed_at FROM github_sync_state
  WHERE status = 'RATE_LIMITED' AND next_allowed_at IS NOT NULL
  ORDER BY julianday(next_allowed_at) DESC
  LIMIT 1
)
WHERE id = 'default';

CREATE TABLE IF NOT EXISTS github_issue_page_etags (
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  page INTEGER NOT NULL CHECK (page > 0),
  etag TEXT NOT NULL,
  PRIMARY KEY (project_id, page)
);
