-- Native Rust Factory persistence schema.
-- Adopts the experimental bootstrap schema into versioned migrations without
-- discarding existing rows. All identifiers are Core-assigned opaque strings.

CREATE TABLE IF NOT EXISTS projects (
  id TEXT PRIMARY KEY,
  repository TEXT NOT NULL UNIQUE,
  base_commit TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS actions (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id),
  objective TEXT NOT NULL,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  risk TEXT NOT NULL,
  status TEXT NOT NULL,
  objective_sha256 TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS actions_project_idx ON actions(project_id);

CREATE TABLE IF NOT EXISTS action_specs (
  action_id TEXT PRIMARY KEY REFERENCES actions(id),
  spec_json TEXT NOT NULL,
  spec_sha256 TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS action_specs_immutable_update
BEFORE UPDATE ON action_specs BEGIN SELECT RAISE(ABORT, 'ActionSpec is immutable'); END;
CREATE TRIGGER IF NOT EXISTS action_specs_immutable_delete
BEFORE DELETE ON action_specs BEGIN SELECT RAISE(ABORT, 'ActionSpec is immutable'); END;

CREATE TABLE IF NOT EXISTS campaigns (
  id TEXT PRIMARY KEY,
  action_id TEXT NOT NULL REFERENCES actions(id),
  status TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS campaigns_action_idx ON campaigns(action_id);

CREATE TABLE IF NOT EXISTS runs (
  id TEXT PRIMARY KEY,
  campaign_id TEXT NOT NULL REFERENCES campaigns(id),
  status TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS runs_campaign_idx ON runs(campaign_id);

CREATE TABLE IF NOT EXISTS attempts (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id),
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  sandbox_ready INTEGER NOT NULL DEFAULT 0 CHECK (sandbox_ready IN (0, 1))
);
CREATE INDEX IF NOT EXISTS attempts_run_idx ON attempts(run_id);

CREATE TABLE IF NOT EXISTS worker_leases (
  attempt_id TEXT PRIMARY KEY REFERENCES attempts(id),
  worker_id TEXT NOT NULL,
  generation INTEGER NOT NULL CHECK (generation > 0),
  expires_at_ms INTEGER NOT NULL CHECK (expires_at_ms > 0)
);
CREATE INDEX IF NOT EXISTS worker_leases_expiry_idx ON worker_leases(expires_at_ms);

CREATE TABLE IF NOT EXISTS worker_receipts (
  receipt_id TEXT PRIMARY KEY,
  request_digest TEXT NOT NULL,
  attempt_id TEXT NOT NULL REFERENCES attempts(id),
  recorded_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS worker_receipts_attempt_idx ON worker_receipts(attempt_id);

CREATE TABLE IF NOT EXISTS worker_sessions (
  worker_id TEXT PRIMARY KEY,
  token_sha256 TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS candidates (
  id TEXT PRIMARY KEY,
  attempt_id TEXT NOT NULL UNIQUE REFERENCES attempts(id),
  action_spec_sha256 TEXT NOT NULL,
  commit_sha TEXT NOT NULL,
  tree_sha TEXT NOT NULL,
  implementer_id TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS candidates_immutable_update
BEFORE UPDATE ON candidates BEGIN SELECT RAISE(ABORT, 'candidate is immutable'); END;
CREATE TRIGGER IF NOT EXISTS candidates_immutable_delete
BEFORE DELETE ON candidates BEGIN SELECT RAISE(ABORT, 'candidate is immutable'); END;

CREATE TABLE IF NOT EXISTS quality_receipts (
  receipt_id TEXT PRIMARY KEY,
  candidate_id TEXT NOT NULL REFERENCES candidates(id),
  role TEXT NOT NULL CHECK (role IN ('verifier', 'reviewer')),
  principal_id TEXT NOT NULL,
  action_spec_sha256 TEXT NOT NULL,
  commit_sha TEXT NOT NULL,
  tree_sha TEXT NOT NULL,
  result TEXT NOT NULL CHECK (result IN ('PASS', 'FAIL', 'CHANGES_REQUESTED', 'NOT_TESTED')),
  evidence TEXT NOT NULL CHECK (evidence IN ('EXECUTED', 'MOCKED', 'UNKNOWN')),
  received_at TEXT NOT NULL,
  UNIQUE(candidate_id, role)
);
CREATE TRIGGER IF NOT EXISTS quality_receipts_immutable_update
BEFORE UPDATE ON quality_receipts BEGIN SELECT RAISE(ABORT, 'quality receipt is immutable'); END;
CREATE TRIGGER IF NOT EXISTS quality_receipts_immutable_delete
BEFORE DELETE ON quality_receipts BEGIN SELECT RAISE(ABORT, 'quality receipt is immutable'); END;

CREATE TABLE IF NOT EXISTS gate_decisions (
  candidate_id TEXT PRIMARY KEY REFERENCES candidates(id),
  outcome TEXT NOT NULL CHECK (outcome IN ('ACCEPTED', 'REPAIR_REQUIRED', 'BLOCKED', 'HUMAN_REQUIRED', 'NOT_TESTED')),
  reasons_json TEXT NOT NULL,
  decided_at TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS gate_decisions_immutable_update
BEFORE UPDATE ON gate_decisions BEGIN SELECT RAISE(ABORT, 'gate decision is immutable'); END;
CREATE TRIGGER IF NOT EXISTS gate_decisions_immutable_delete
BEFORE DELETE ON gate_decisions BEGIN SELECT RAISE(ABORT, 'gate decision is immutable'); END;

CREATE TABLE IF NOT EXISTS events (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  subject_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  detail TEXT NOT NULL,
  recorded_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS events_subject_idx ON events(subject_id, sequence);

CREATE TABLE IF NOT EXISTS idempotency (
  key TEXT PRIMARY KEY,
  operation TEXT NOT NULL,
  request_digest TEXT NOT NULL,
  resource_id TEXT NOT NULL
);

PRAGMA user_version = 2;
