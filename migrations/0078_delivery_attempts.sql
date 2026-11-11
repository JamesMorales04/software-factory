-- Durable delivery attempts, decisions, checks, incidents and manual reverts.
--
-- A DeliveryAttempt is one intent to merge one exact candidate at one exact
-- expected head under one exact policy revision. The idempotency key makes a
-- duplicate Office submission or Core restart safe. Attempt identity columns
-- are immutable; operational state transitions are appended to
-- delivery_attempt_events. External success is never inferred from an HTTP
-- status alone.

CREATE TABLE IF NOT EXISTS delivery_attempts (
  attempt_id TEXT PRIMARY KEY,
  idempotency_key TEXT NOT NULL UNIQUE,
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  operation TEXT NOT NULL CHECK (operation IN ('OPEN_PR', 'MERGE')),
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  campaign_id TEXT NOT NULL REFERENCES campaigns(id),
  candidate_id TEXT NOT NULL REFERENCES candidates(id),
  candidate_sha TEXT NOT NULL,
  candidate_tree_sha TEXT NOT NULL,
  content_digest TEXT NOT NULL,
  gate_revision TEXT NOT NULL,
  gate_outcome TEXT NOT NULL,
  policy_revision INTEGER NOT NULL,
  policy_sha256 TEXT NOT NULL,
  risk_class TEXT NOT NULL CHECK (risk_class IN ('R0', 'R1', 'R2', 'R3', 'UNKNOWN')),
  authorization TEXT CHECK (authorization IS NULL OR authorization IN (
    'POLICY_R0', 'CHARTER_R1', 'OWNER_MANUAL'
  )),
  charter_grant_id TEXT,
  pull_request_number INTEGER CHECK (pull_request_number > 0),
  base_branch TEXT NOT NULL,
  expected_head_sha TEXT NOT NULL,
  observed_head_sha TEXT,
  observed_base_sha TEXT,
  merge_method TEXT NOT NULL CHECK (merge_method = 'squash'),
  state TEXT NOT NULL CHECK (state IN (
    'PENDING', 'AUTHORIZED', 'MERGING', 'MERGED', 'BLOCKED', 'NEEDS_ATTENTION', 'REVERTED'
  )),
  merge_commit_sha TEXT,
  incident_id TEXT,
  post_merge_check_id TEXT,
  blocker_code TEXT,
  blocker_detail TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  merged_at TEXT,
  UNIQUE (candidate_id, policy_revision, expected_head_sha, operation)
);

CREATE INDEX IF NOT EXISTS delivery_attempts_candidate_idx
  ON delivery_attempts(candidate_id, created_at DESC);
CREATE INDEX IF NOT EXISTS delivery_attempts_campaign_idx
  ON delivery_attempts(campaign_id, created_at DESC);
CREATE INDEX IF NOT EXISTS delivery_attempts_project_state_idx
  ON delivery_attempts(project_id, state, updated_at DESC);
CREATE INDEX IF NOT EXISTS delivery_attempts_pr_idx
  ON delivery_attempts(project_id, pull_request_number);

CREATE TRIGGER IF NOT EXISTS delivery_attempts_immutable_identity
BEFORE UPDATE ON delivery_attempts
WHEN OLD.attempt_id <> NEW.attempt_id
  OR OLD.idempotency_key <> NEW.idempotency_key
  OR OLD.schema_version <> NEW.schema_version
  OR OLD.operation <> NEW.operation
  OR OLD.project_id <> NEW.project_id
  OR OLD.campaign_id <> NEW.campaign_id
  OR OLD.candidate_id <> NEW.candidate_id
  OR OLD.candidate_sha <> NEW.candidate_sha
  OR OLD.candidate_tree_sha <> NEW.candidate_tree_sha
  OR OLD.content_digest <> NEW.content_digest
  OR OLD.gate_revision <> NEW.gate_revision
  OR OLD.gate_outcome <> NEW.gate_outcome
  OR OLD.policy_revision <> NEW.policy_revision
  OR OLD.policy_sha256 <> NEW.policy_sha256
  OR OLD.risk_class <> NEW.risk_class
  OR (OLD.authorization IS NOT NULL AND OLD.authorization <> NEW.authorization)
  OR (OLD.charter_grant_id IS NOT NULL AND OLD.charter_grant_id <> NEW.charter_grant_id)
  OR OLD.base_branch <> NEW.base_branch
  OR OLD.expected_head_sha <> NEW.expected_head_sha
  OR OLD.merge_method <> NEW.merge_method
  OR OLD.created_at <> NEW.created_at
  OR (OLD.merge_commit_sha IS NOT NULL AND OLD.merge_commit_sha <> NEW.merge_commit_sha)
  OR (OLD.merged_at IS NOT NULL AND OLD.merged_at <> NEW.merged_at)
BEGIN SELECT RAISE(ABORT, 'delivery attempt identity is immutable'); END;

CREATE TRIGGER IF NOT EXISTS delivery_attempts_immutable_delete
BEFORE DELETE ON delivery_attempts
BEGIN SELECT RAISE(ABORT, 'delivery attempts are immutable'); END;

CREATE TABLE IF NOT EXISTS delivery_attempt_events (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  event_id TEXT NOT NULL UNIQUE,
  attempt_id TEXT NOT NULL REFERENCES delivery_attempts(attempt_id),
  kind TEXT NOT NULL,
  detail TEXT NOT NULL,
  recorded_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS delivery_attempt_events_attempt_idx
  ON delivery_attempt_events(attempt_id, sequence);

CREATE TRIGGER IF NOT EXISTS delivery_attempt_events_append_only_update
BEFORE UPDATE ON delivery_attempt_events
BEGIN SELECT RAISE(ABORT, 'delivery attempt events are append-only'); END;

CREATE TRIGGER IF NOT EXISTS delivery_attempt_events_append_only_delete
BEFORE DELETE ON delivery_attempt_events
BEGIN SELECT RAISE(ABORT, 'delivery attempt events are append-only'); END;

CREATE TABLE IF NOT EXISTS delivery_decisions (
  decision_id TEXT PRIMARY KEY,
  attempt_id TEXT NOT NULL REFERENCES delivery_attempts(attempt_id),
  candidate_id TEXT NOT NULL,
  candidate_sha TEXT NOT NULL,
  gate_revision TEXT NOT NULL,
  gate_outcome TEXT NOT NULL,
  policy_revision INTEGER NOT NULL,
  policy_sha256 TEXT NOT NULL,
  charter_grant_id TEXT,
  risk_class TEXT NOT NULL CHECK (risk_class IN ('R0', 'R1', 'R2', 'R3', 'UNKNOWN')),
  expected_head_sha TEXT NOT NULL,
  base_branch TEXT NOT NULL,
  checks_read_at TEXT NOT NULL,
  branch_protection_read_at TEXT NOT NULL,
  decision TEXT NOT NULL CHECK (decision IN ('ALLOW', 'DENY')),
  reasons_json TEXT NOT NULL,
  decided_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS delivery_decisions_attempt_idx
  ON delivery_decisions(attempt_id, decided_at DESC);

CREATE TRIGGER IF NOT EXISTS delivery_decisions_immutable_update
BEFORE UPDATE ON delivery_decisions
BEGIN SELECT RAISE(ABORT, 'delivery decisions are immutable'); END;

CREATE TRIGGER IF NOT EXISTS delivery_decisions_immutable_delete
BEFORE DELETE ON delivery_decisions
BEGIN SELECT RAISE(ABORT, 'delivery decisions are immutable'); END;

CREATE TABLE IF NOT EXISTS delivery_check_receipts (
  attempt_id TEXT NOT NULL REFERENCES delivery_attempts(attempt_id),
  check_name TEXT NOT NULL,
  head_sha TEXT NOT NULL,
  state TEXT NOT NULL,
  conclusion TEXT NOT NULL,
  required_by_protection INTEGER NOT NULL CHECK (required_by_protection IN (0, 1)),
  observed_at TEXT NOT NULL,
  PRIMARY KEY (attempt_id, check_name, head_sha, observed_at)
);

CREATE TRIGGER IF NOT EXISTS delivery_check_receipts_immutable_update
BEFORE UPDATE ON delivery_check_receipts
BEGIN SELECT RAISE(ABORT, 'delivery check receipts are immutable'); END;

CREATE TRIGGER IF NOT EXISTS delivery_check_receipts_immutable_delete
BEFORE DELETE ON delivery_check_receipts
BEGIN SELECT RAISE(ABORT, 'delivery check receipts are immutable'); END;

CREATE TABLE IF NOT EXISTS delivery_post_merge_checks (
  attempt_id TEXT NOT NULL REFERENCES delivery_attempts(attempt_id),
  check_id TEXT NOT NULL,
  merge_commit_sha TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('PENDING', 'PASSED', 'FAILED', 'MISSING')),
  detail TEXT NOT NULL,
  observed_at TEXT NOT NULL,
  PRIMARY KEY (attempt_id, check_id)
);

CREATE TABLE IF NOT EXISTS delivery_incidents (
  incident_id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  attempt_id TEXT NOT NULL REFERENCES delivery_attempts(attempt_id),
  candidate_id TEXT NOT NULL,
  merge_commit_sha TEXT NOT NULL,
  kind TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN (
    'OPEN', 'ACKNOWLEDGED', 'REVERT_PENDING', 'REVERTED', 'CLOSED'
  )),
  detail TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  acknowledged_by TEXT,
  acknowledged_at TEXT,
  resolved_at TEXT
);

CREATE INDEX IF NOT EXISTS delivery_incidents_project_state_idx
  ON delivery_incidents(project_id, state, created_at DESC);
CREATE INDEX IF NOT EXISTS delivery_incidents_attempt_idx
  ON delivery_incidents(attempt_id, created_at DESC);

CREATE TRIGGER IF NOT EXISTS delivery_incidents_immutable_identity
BEFORE UPDATE ON delivery_incidents
WHEN OLD.incident_id <> NEW.incident_id
  OR OLD.project_id <> NEW.project_id
  OR OLD.attempt_id <> NEW.attempt_id
  OR OLD.candidate_id <> NEW.candidate_id
  OR OLD.merge_commit_sha <> NEW.merge_commit_sha
  OR OLD.kind <> NEW.kind
  OR OLD.created_at <> NEW.created_at
BEGIN SELECT RAISE(ABORT, 'delivery incident identity is immutable'); END;

CREATE TRIGGER IF NOT EXISTS delivery_incidents_immutable_delete
BEFORE DELETE ON delivery_incidents
BEGIN SELECT RAISE(ABORT, 'delivery incidents are immutable'); END;

CREATE TABLE IF NOT EXISTS delivery_revert_records (
  revert_id TEXT PRIMARY KEY,
  incident_id TEXT NOT NULL REFERENCES delivery_incidents(incident_id),
  revert_pr_number INTEGER NOT NULL CHECK (revert_pr_number > 0),
  revert_merge_sha TEXT NOT NULL,
  actor TEXT NOT NULL,
  rationale TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (incident_id, revert_merge_sha)
);

CREATE TRIGGER IF NOT EXISTS delivery_revert_records_immutable_update
BEFORE UPDATE ON delivery_revert_records
BEGIN SELECT RAISE(ABORT, 'delivery revert records are immutable'); END;

CREATE TRIGGER IF NOT EXISTS delivery_revert_records_immutable_delete
BEFORE DELETE ON delivery_revert_records
BEGIN SELECT RAISE(ABORT, 'delivery revert records are immutable'); END;

CREATE TABLE IF NOT EXISTS delivery_issue_receipts (
  receipt_id TEXT PRIMARY KEY,
  attempt_id TEXT NOT NULL REFERENCES delivery_attempts(attempt_id),
  issue_link_id TEXT,
  action TEXT NOT NULL,
  state TEXT NOT NULL,
  external_id TEXT,
  detail TEXT,
  recorded_at TEXT NOT NULL,
  UNIQUE (attempt_id, issue_link_id, action)
);

CREATE TRIGGER IF NOT EXISTS delivery_issue_receipts_immutable_update
BEFORE UPDATE ON delivery_issue_receipts
BEGIN SELECT RAISE(ABORT, 'delivery issue receipts are immutable'); END;

CREATE TRIGGER IF NOT EXISTS delivery_issue_receipts_immutable_delete
BEFORE DELETE ON delivery_issue_receipts
BEGIN SELECT RAISE(ABORT, 'delivery issue receipts are immutable'); END;
