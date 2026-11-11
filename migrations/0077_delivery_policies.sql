-- Project delivery policy revisions.
--
-- Gate ACCEPTED is not delivery authority. A Project must explicitly opt in to
-- risk-tiered delivery, and every policy update appends an immutable revision.
-- Auto-merge always targets the Project's protected default branch and uses
-- squash merges; R2/R3 and unknown risk stay manual.

CREATE TABLE IF NOT EXISTS delivery_policies (
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  revision INTEGER NOT NULL CHECK (revision > 0),
  schema_version INTEGER NOT NULL CHECK (schema_version = 1),
  policy_sha256 TEXT NOT NULL,
  mode TEXT NOT NULL CHECK (mode IN ('MANUAL', 'RISK_TIERED')),
  r1_enabled INTEGER NOT NULL CHECK (r1_enabled IN (0, 1)),
  auto_merge_base_branch TEXT NOT NULL,
  manual_base_branches_json TEXT NOT NULL,
  required_checks_json TEXT NOT NULL,
  merge_method TEXT NOT NULL CHECK (merge_method = 'squash'),
  post_merge_check_required INTEGER NOT NULL CHECK (post_merge_check_required = 1),
  post_merge_check_id TEXT NOT NULL,
  post_merge_timeout_secs INTEGER NOT NULL CHECK (post_merge_timeout_secs > 0),
  kill_switch INTEGER NOT NULL CHECK (kill_switch IN (0, 1)),
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT,
  PRIMARY KEY (project_id, revision),
  CHECK (r1_enabled = 0 OR mode = 'RISK_TIERED')
);

CREATE INDEX IF NOT EXISTS delivery_policies_project_revision_idx
  ON delivery_policies(project_id, revision DESC);

CREATE TRIGGER IF NOT EXISTS delivery_policies_append_only_update
BEFORE UPDATE ON delivery_policies
BEGIN SELECT RAISE(ABORT, 'delivery policy revisions are append-only'); END;

CREATE TRIGGER IF NOT EXISTS delivery_policies_append_only_delete
BEFORE DELETE ON delivery_policies
BEGIN SELECT RAISE(ABORT, 'delivery policy revisions are append-only'); END;
