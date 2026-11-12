-- Issue-linked delivery lifecycle.
--
-- Issue links are frozen when a request is created. They map the exact
-- repository/issue selection to the request, then to the Campaign, candidate,
-- pull request and merge facts that follow. State is a Core projection over
-- those facts; no model or Office client writes it.
--
-- The delivery ledger is an idempotency record for GitHub issue effects. A
-- retry of the same logical event reuses the row instead of creating a second
-- comment, label application or close.

CREATE TABLE IF NOT EXISTS issue_links (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  request_id TEXT NOT NULL REFERENCES intake_requests(id) ON DELETE CASCADE,
  campaign_id TEXT REFERENCES campaigns(id),
  candidate_id TEXT REFERENCES candidates(id),
  repository_owner TEXT NOT NULL,
  repository_name TEXT NOT NULL,
  repository_full_name TEXT NOT NULL,
  issue_number INTEGER NOT NULL CHECK (issue_number > 0),
  github_issue_id INTEGER,
  issue_title TEXT NOT NULL DEFAULT '',
  role TEXT NOT NULL CHECK (role IN ('PRIMARY', 'RELATED')),
  state TEXT NOT NULL CHECK (state IN (
    'READY', 'QUEUED', 'IN_PROGRESS', 'NEEDS_ATTENTION', 'IN_REVIEW', 'DONE'
  )),
  delivery_state TEXT NOT NULL DEFAULT 'NONE' CHECK (delivery_state IN (
    'NONE', 'PENDING', 'COMMENTED', 'CLOSED', 'PARTIAL', 'FAILED', 'BLOCKED'
  )),
  resolution TEXT NOT NULL DEFAULT 'UNRESOLVED' CHECK (resolution IN (
    'UNRESOLVED', 'RESOLVED', 'PARTIAL'
  )),
  work_unit_id TEXT,
  pull_request_number INTEGER CHECK (pull_request_number > 0),
  pull_request_url TEXT,
  merge_sha TEXT,
  merged_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (request_id, repository_full_name, issue_number)
);

CREATE INDEX IF NOT EXISTS issue_links_request_idx
  ON issue_links(request_id, created_at);
CREATE INDEX IF NOT EXISTS issue_links_campaign_idx
  ON issue_links(campaign_id, created_at);
CREATE INDEX IF NOT EXISTS issue_links_project_issue_idx
  ON issue_links(project_id, repository_full_name, issue_number, created_at DESC);
CREATE INDEX IF NOT EXISTS issue_links_request_campaign_idx
  ON issue_links(request_id, campaign_id);

CREATE TABLE IF NOT EXISTS issue_delivery_events (
  id TEXT PRIMARY KEY,
  issue_link_id TEXT NOT NULL REFERENCES issue_links(id) ON DELETE CASCADE,
  campaign_id TEXT,
  candidate_id TEXT,
  action TEXT NOT NULL CHECK (action IN ('COMMENT', 'LABEL', 'CLOSE', 'FAILURE')),
  event_key TEXT NOT NULL CHECK (length(event_key) BETWEEN 1 AND 200),
  marker TEXT,
  body TEXT,
  label_name TEXT,
  status TEXT NOT NULL CHECK (status IN ('PENDING', 'APPLIED', 'FAILED', 'BLOCKED')),
  attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  external_id TEXT,
  error_kind TEXT,
  error_detail TEXT,
  last_attempt_at TEXT,
  applied_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (issue_link_id, event_key)
);

CREATE INDEX IF NOT EXISTS issue_delivery_events_link_idx
  ON issue_delivery_events(issue_link_id, created_at);
CREATE INDEX IF NOT EXISTS issue_delivery_events_status_idx
  ON issue_delivery_events(status, updated_at DESC);
CREATE INDEX IF NOT EXISTS issue_delivery_events_campaign_idx
  ON issue_delivery_events(campaign_id, created_at);
