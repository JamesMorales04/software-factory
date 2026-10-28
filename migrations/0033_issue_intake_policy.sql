-- Governed issue-intake policy, finite batch acknowledgements and a durable
-- reconciliation queue.
--
-- Everything here is Core-owned and default OFF. A repository binding, labels,
-- a profile `autoStart` flag, a synced snapshot or a queued row never grants
-- execution authority. A batch acknowledgement authorizes only deterministic
-- reconciliation of the exact listed issue snapshots into ordinary Lead/intake
-- proposals; starting a Campaign remains a separate explicit operator act.

ALTER TABLE project_github_bindings ADD COLUMN repository_github_id INTEGER;

ALTER TABLE github_issues ADD COLUMN repository_github_id INTEGER;
ALTER TABLE github_issues ADD COLUMN author_id INTEGER;
ALTER TABLE github_issues ADD COLUMN author_association TEXT;
ALTER TABLE github_issues ADD COLUMN is_pull_request INTEGER NOT NULL DEFAULT 0
  CHECK (is_pull_request IN (0, 1));

CREATE INDEX IF NOT EXISTS github_issues_identity_idx
  ON github_issues(project_id, repository_github_id, number);

-- Immutable, versioned project issue-intake policy. The absence of a current
-- head means disabled. Enabling, editing and disabling all create a new
-- revision; old revisions are never rewritten.
CREATE TABLE IF NOT EXISTS issue_intake_policy_revisions (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  revision INTEGER NOT NULL CHECK (revision > 0),
  enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
  repository_full_name TEXT NOT NULL,
  repository_github_id INTEGER CHECK (repository_github_id IS NULL OR repository_github_id > 0),
  connection_revision INTEGER NOT NULL CHECK (connection_revision > 0),
  required_labels_json TEXT NOT NULL,
  excluded_labels_json TEXT NOT NULL,
  author_ids_json TEXT NOT NULL,
  author_associations_json TEXT NOT NULL,
  profile_id TEXT NOT NULL REFERENCES execution_profiles(id),
  profile_revision_id TEXT NOT NULL REFERENCES execution_profile_revisions(revision_id),
  profile_revision INTEGER NOT NULL CHECK (profile_revision > 0),
  profile_hash TEXT NOT NULL,
  project_config_hash TEXT NOT NULL,
  allowed_paths_json TEXT NOT NULL,
  capabilities_json TEXT NOT NULL,
  max_risk_class TEXT NOT NULL CHECK (max_risk_class = 'R0'),
  max_issues INTEGER NOT NULL CHECK (max_issues BETWEEN 1 AND 20),
  max_concurrent INTEGER NOT NULL CHECK (max_concurrent = 1),
  max_cost_micros_per_issue INTEGER NOT NULL CHECK (max_cost_micros_per_issue > 0),
  max_total_cost_micros INTEGER NOT NULL CHECK (max_total_cost_micros > 0),
  max_attempts INTEGER NOT NULL CHECK (max_attempts BETWEEN 1 AND 5),
  max_sessions INTEGER NOT NULL CHECK (max_sessions > 0),
  max_turns INTEGER NOT NULL CHECK (max_turns > 0),
  max_wall_clock_secs INTEGER NOT NULL CHECK (max_wall_clock_secs > 0),
  expires_at TEXT NOT NULL,
  acknowledged INTEGER NOT NULL CHECK (acknowledged = 1),
  principal_id TEXT NOT NULL,
  principal_label TEXT,
  reason TEXT NOT NULL,
  idempotency_key TEXT NOT NULL UNIQUE,
  content_hash TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (project_id, revision)
);

CREATE INDEX IF NOT EXISTS issue_intake_policy_project_idx
  ON issue_intake_policy_revisions(project_id, revision DESC);

CREATE TRIGGER IF NOT EXISTS issue_intake_policy_revisions_immutable_update
BEFORE UPDATE ON issue_intake_policy_revisions
BEGIN
  SELECT RAISE(ABORT, 'issue intake policy revision is immutable');
END;

CREATE TRIGGER IF NOT EXISTS issue_intake_policy_revisions_immutable_delete
BEFORE DELETE ON issue_intake_policy_revisions
BEGIN
  SELECT RAISE(ABORT, 'issue intake policy revision is immutable');
END;

CREATE TABLE IF NOT EXISTS issue_intake_policy_heads (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  policy_id TEXT NOT NULL UNIQUE REFERENCES issue_intake_policy_revisions(id),
  revision INTEGER NOT NULL CHECK (revision > 0),
  updated_at TEXT NOT NULL
);

-- One acknowledged, finite batch. It is a preauthorization for proposal
-- reconciliation only. `delegation_id` is a non-autonomous standing
-- delegation so ordinary intake records can reference a concrete policy;
-- `autonomous_start` is always false and is never inferred here.
CREATE TABLE IF NOT EXISTS issue_intake_batches (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  policy_id TEXT NOT NULL REFERENCES issue_intake_policy_revisions(id),
  policy_revision INTEGER NOT NULL CHECK (policy_revision > 0),
  policy_hash TEXT NOT NULL,
  delegation_id TEXT NOT NULL UNIQUE REFERENCES standing_delegations(id),
  repository_full_name TEXT NOT NULL,
  repository_github_id INTEGER NOT NULL CHECK (repository_github_id > 0),
  connection_revision INTEGER NOT NULL CHECK (connection_revision > 0),
  profile_id TEXT NOT NULL REFERENCES execution_profiles(id),
  profile_revision_id TEXT NOT NULL REFERENCES execution_profile_revisions(revision_id),
  profile_revision INTEGER NOT NULL CHECK (profile_revision > 0),
  profile_hash TEXT NOT NULL,
  project_config_hash TEXT NOT NULL,
  max_issues INTEGER NOT NULL CHECK (max_issues BETWEEN 1 AND 20),
  max_concurrent INTEGER NOT NULL CHECK (max_concurrent = 1),
  max_cost_micros_per_issue INTEGER NOT NULL CHECK (max_cost_micros_per_issue > 0),
  max_total_cost_micros INTEGER NOT NULL CHECK (max_total_cost_micros > 0),
  reserved_cost_micros INTEGER NOT NULL DEFAULT 0 CHECK (reserved_cost_micros >= 0),
  reserved_issues INTEGER NOT NULL DEFAULT 0 CHECK (reserved_issues >= 0),
  status TEXT NOT NULL CHECK (status IN ('ACTIVE', 'REVOKED', 'EXPIRED', 'EXHAUSTED')),
  acknowledged INTEGER NOT NULL CHECK (acknowledged = 1),
  principal_id TEXT NOT NULL,
  principal_label TEXT,
  reason TEXT NOT NULL,
  idempotency_key TEXT NOT NULL UNIQUE,
  batch_hash TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  revoked_at TEXT
);

CREATE INDEX IF NOT EXISTS issue_intake_batches_project_idx
  ON issue_intake_batches(project_id, created_at DESC);

CREATE TRIGGER IF NOT EXISTS issue_intake_batches_immutable_content
BEFORE UPDATE ON issue_intake_batches
WHEN OLD.id <> NEW.id
  OR OLD.project_id <> NEW.project_id
  OR OLD.policy_id <> NEW.policy_id
  OR OLD.policy_revision <> NEW.policy_revision
  OR OLD.policy_hash <> NEW.policy_hash
  OR OLD.delegation_id <> NEW.delegation_id
  OR OLD.repository_full_name <> NEW.repository_full_name
  OR OLD.repository_github_id <> NEW.repository_github_id
  OR OLD.connection_revision <> NEW.connection_revision
  OR OLD.profile_id <> NEW.profile_id
  OR OLD.profile_revision_id <> NEW.profile_revision_id
  OR OLD.profile_revision <> NEW.profile_revision
  OR OLD.profile_hash <> NEW.profile_hash
  OR OLD.project_config_hash <> NEW.project_config_hash
  OR OLD.max_issues <> NEW.max_issues
  OR OLD.max_concurrent <> NEW.max_concurrent
  OR OLD.max_cost_micros_per_issue <> NEW.max_cost_micros_per_issue
  OR OLD.max_total_cost_micros <> NEW.max_total_cost_micros
  OR OLD.acknowledged <> NEW.acknowledged
  OR OLD.principal_id <> NEW.principal_id
  OR OLD.principal_label IS NOT NEW.principal_label
  OR OLD.reason <> NEW.reason
  OR OLD.idempotency_key <> NEW.idempotency_key
  OR OLD.batch_hash <> NEW.batch_hash
  OR OLD.expires_at <> NEW.expires_at
  OR OLD.created_at <> NEW.created_at
BEGIN
  SELECT RAISE(ABORT, 'issue intake batch content is immutable');
END;

CREATE TRIGGER IF NOT EXISTS issue_intake_batches_immutable_delete
BEFORE DELETE ON issue_intake_batches
BEGIN
  SELECT RAISE(ABORT, 'issue intake batch is immutable');
END;

-- Exact issue snapshot acknowledgement. The stored digest and immutable
-- GitHub identities are frozen at batch creation; later issue edits make the
-- item stale and require a new explicit batch.
CREATE TABLE IF NOT EXISTS issue_intake_batch_items (
  id TEXT PRIMARY KEY,
  batch_id TEXT NOT NULL REFERENCES issue_intake_batches(id),
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  repository_full_name TEXT NOT NULL,
  repository_github_id INTEGER NOT NULL CHECK (repository_github_id > 0),
  issue_number INTEGER NOT NULL CHECK (issue_number > 0),
  github_issue_id INTEGER NOT NULL,
  content_hash TEXT NOT NULL,
  issue_updated_at TEXT NOT NULL,
  author_login TEXT,
  author_id INTEGER,
  author_association TEXT,
  labels_json TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN
    ('PENDING', 'RECONCILED', 'INELIGIBLE', 'STALE', 'REVOKED')),
  eligibility_reasons_json TEXT NOT NULL DEFAULT '[]',
  reserved_cost_micros INTEGER NOT NULL DEFAULT 0 CHECK (reserved_cost_micros >= 0),
  intake_request_id TEXT REFERENCES intake_requests(id),
  queue_item_id TEXT UNIQUE,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (batch_id, repository_github_id, issue_number)
);

CREATE INDEX IF NOT EXISTS issue_intake_batch_items_project_idx
  ON issue_intake_batch_items(project_id, state, created_at DESC);

CREATE TRIGGER IF NOT EXISTS issue_intake_batch_items_immutable_content
BEFORE UPDATE ON issue_intake_batch_items
WHEN OLD.id <> NEW.id
  OR OLD.batch_id <> NEW.batch_id
  OR OLD.project_id <> NEW.project_id
  OR OLD.repository_full_name <> NEW.repository_full_name
  OR OLD.repository_github_id <> NEW.repository_github_id
  OR OLD.issue_number <> NEW.issue_number
  OR OLD.github_issue_id <> NEW.github_issue_id
  OR OLD.content_hash <> NEW.content_hash
  OR OLD.issue_updated_at <> NEW.issue_updated_at
  OR OLD.author_login IS NOT NEW.author_login
  OR OLD.author_id IS NOT NEW.author_id
  OR OLD.author_association IS NOT NEW.author_association
  OR OLD.labels_json <> NEW.labels_json
  OR OLD.created_at <> NEW.created_at
BEGIN
  SELECT RAISE(ABORT, 'issue intake batch item snapshot is immutable');
END;

CREATE TRIGGER IF NOT EXISTS issue_intake_batch_items_immutable_delete
BEFORE DELETE ON issue_intake_batch_items
BEGIN
  SELECT RAISE(ABORT, 'issue intake batch item is immutable');
END;

-- Durable Core ingestion queue. There is no second scheduler: authenticated
-- dispatch leases one due row, performs the direct freshness check, and links
-- the ordinary intake request before completion. Leases are recovered on
-- Core restart.
CREATE TABLE IF NOT EXISTS issue_intake_queue (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  batch_id TEXT NOT NULL REFERENCES issue_intake_batches(id),
  batch_item_id TEXT NOT NULL UNIQUE REFERENCES issue_intake_batch_items(id),
  repository_full_name TEXT NOT NULL,
  repository_github_id INTEGER NOT NULL CHECK (repository_github_id > 0),
  issue_number INTEGER NOT NULL CHECK (issue_number > 0),
  github_issue_id INTEGER NOT NULL,
  content_hash TEXT NOT NULL,
  connection_revision INTEGER NOT NULL CHECK (connection_revision > 0),
  policy_revision INTEGER NOT NULL CHECK (policy_revision > 0),
  status TEXT NOT NULL CHECK (status IN
    ('PENDING', 'LEASED', 'RECONCILED', 'BLOCKED', 'STALE', 'INELIGIBLE', 'REVOKED')),
  lease_token TEXT,
  lease_expires_at TEXT,
  attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  max_attempts INTEGER NOT NULL DEFAULT 3 CHECK (max_attempts BETWEEN 1 AND 3),
  next_attempt_at TEXT,
  reason TEXT,
  reasons_json TEXT NOT NULL DEFAULT '[]',
  intake_request_id TEXT REFERENCES intake_requests(id),
  lead_request_id TEXT REFERENCES lead_requests(id),
  action_id TEXT REFERENCES actions(id),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  completed_at TEXT
);

CREATE INDEX IF NOT EXISTS issue_intake_queue_project_status_idx
  ON issue_intake_queue(project_id, status, next_attempt_at, created_at);

CREATE INDEX IF NOT EXISTS issue_intake_queue_batch_idx
  ON issue_intake_queue(batch_id, status);

-- Append-only policy, batch and queue audit. Bodies, titles and credentials
-- are never recorded here.
CREATE TABLE IF NOT EXISTS issue_intake_audit (
  audit_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  policy_id TEXT,
  batch_id TEXT,
  queue_item_id TEXT,
  action TEXT NOT NULL,
  actor TEXT NOT NULL,
  principal_id TEXT NOT NULL,
  reason TEXT NOT NULL,
  detail TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS issue_intake_audit_project_created_idx
  ON issue_intake_audit(project_id, created_at DESC, audit_id DESC);

CREATE TRIGGER IF NOT EXISTS issue_intake_audit_immutable_update
BEFORE UPDATE ON issue_intake_audit
BEGIN
  SELECT RAISE(ABORT, 'issue intake audit is append-only');
END;

CREATE TRIGGER IF NOT EXISTS issue_intake_audit_immutable_delete
BEFORE DELETE ON issue_intake_audit
BEGIN
  SELECT RAISE(ABORT, 'issue intake audit is append-only');
END;
