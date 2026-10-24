-- Exact per-Attempt execution spec for bounded WorkGraph units.
CREATE TABLE IF NOT EXISTS attempt_specs (
  attempt_id TEXT PRIMARY KEY REFERENCES attempts(id),
  action_id TEXT NOT NULL,
  base_commit TEXT NOT NULL,
  spec_json TEXT NOT NULL,
  spec_sha256 TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS lead_tasks (
  task_id TEXT PRIMARY KEY,
  campaign_id TEXT NOT NULL REFERENCES campaigns(id),
  request_id TEXT NOT NULL REFERENCES lead_requests(id),
  unit_id TEXT NOT NULL,
  sequence INTEGER NOT NULL,
  status TEXT NOT NULL,
  base_commit TEXT NOT NULL,
  spec_json TEXT NOT NULL,
  spec_sha256 TEXT NOT NULL,
  depends_on_json TEXT NOT NULL,
  run_id TEXT,
  attempt_id TEXT,
  candidate_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(campaign_id,unit_id)
);
CREATE INDEX IF NOT EXISTS lead_tasks_campaign_idx ON lead_tasks(campaign_id,sequence);
