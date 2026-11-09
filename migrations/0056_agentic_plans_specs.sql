-- Frozen Planner/Spec Manager records. Core owns the plan and specification;
-- GitHub issues and the kanban are projections and never a second authority.
CREATE TABLE IF NOT EXISTS agentic_work_graphs (
  request_id TEXT PRIMARY KEY REFERENCES intake_requests(id),
  graph_json TEXT NOT NULL,
  graph_sha256 TEXT NOT NULL,
  topological_order_json TEXT NOT NULL,
  planner_provider TEXT NOT NULL,
  planner_model TEXT NOT NULL,
  schema_sha256 TEXT NOT NULL,
  request_sha256 TEXT NOT NULL,
  input_tokens INTEGER NOT NULL,
  output_tokens INTEGER NOT NULL,
  total_tokens INTEGER NOT NULL,
  cost_micros INTEGER NOT NULL,
  usage_known INTEGER NOT NULL CHECK (usage_known IN (0, 1)),
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS agentic_specs (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  revision INTEGER NOT NULL CHECK (revision > 0),
  state TEXT NOT NULL CHECK (state IN ('DRAFT', 'FROZEN', 'SUPERSEDED')),
  draft_json TEXT NOT NULL,
  frozen_json TEXT,
  spec_sha256 TEXT,
  graph_json TEXT,
  graph_sha256 TEXT,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  schema_sha256 TEXT NOT NULL,
  request_sha256 TEXT NOT NULL,
  input_tokens INTEGER NOT NULL,
  output_tokens INTEGER NOT NULL,
  total_tokens INTEGER NOT NULL,
  cost_micros INTEGER NOT NULL,
  usage_known INTEGER NOT NULL CHECK (usage_known IN (0, 1)),
  created_at TEXT NOT NULL,
  frozen_at TEXT,
  UNIQUE (request_id, revision),
  CHECK (state != 'FROZEN' OR (frozen_json IS NOT NULL AND spec_sha256 IS NOT NULL))
);

CREATE UNIQUE INDEX IF NOT EXISTS agentic_specs_frozen_idx
  ON agentic_specs(request_id) WHERE state = 'FROZEN';

CREATE UNIQUE INDEX IF NOT EXISTS agentic_specs_draft_idx
  ON agentic_specs(request_id) WHERE state = 'DRAFT';

CREATE TABLE IF NOT EXISTS agentic_spec_publications (
  id TEXT PRIMARY KEY,
  spec_id TEXT NOT NULL UNIQUE REFERENCES agentic_specs(id),
  project_id TEXT NOT NULL REFERENCES projects(id),
  provider TEXT NOT NULL CHECK (provider IN ('GITHUB', 'LOCAL')),
  repository TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('PENDING', 'PUBLISHED', 'FAILED', 'LOCAL')),
  drift_status TEXT NOT NULL CHECK (drift_status IN ('IN_SYNC', 'DRIFTED', 'UNKNOWN')),
  last_error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (spec_id, provider)
);

CREATE TABLE IF NOT EXISTS agentic_spec_issues (
  spec_id TEXT NOT NULL REFERENCES agentic_specs(id),
  unit_id TEXT NOT NULL,
  marker TEXT NOT NULL,
  external_number INTEGER,
  external_url TEXT,
  state TEXT NOT NULL CHECK (state IN ('PENDING', 'PUBLISHED', 'FAILED')),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (spec_id, unit_id)
);

CREATE INDEX IF NOT EXISTS agentic_spec_issues_marker_idx
  ON agentic_spec_issues(marker);
