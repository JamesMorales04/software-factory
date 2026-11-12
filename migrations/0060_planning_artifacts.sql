-- Quality-first planning artifacts and durable workflow records.
-- Core remains the authority; these tables store typed proposals, frozen
-- revisions, evidence and projections. JSON columns are validated by Rust.

CREATE TABLE planning_artifacts (
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  kind TEXT NOT NULL CHECK (kind IN (
    'PLAN','STACK','ARCHITECTURE','DOCUMENTATION','SKILL','DOD','OPENSPEC'
  )),
  revision INTEGER NOT NULL CHECK (revision > 0),
  content_json TEXT NOT NULL,
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
  created_at TEXT NOT NULL,
  PRIMARY KEY (request_id, kind, revision)
);

CREATE TABLE plan_reviews (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  plan_revision INTEGER NOT NULL CHECK (plan_revision > 0),
  status TEXT NOT NULL CHECK (status IN (
    'PENDING','AUTO_APPROVED','OWNER_APPROVED','CHANGES_REQUESTED','REJECTED'
  )),
  principal_id TEXT NOT NULL,
  rationale TEXT NOT NULL,
  points INTEGER NOT NULL DEFAULT 0 CHECK (points >= 0),
  reviewed_at TEXT NOT NULL
);
CREATE INDEX plan_reviews_request_idx ON plan_reviews(request_id, reviewed_at DESC);

CREATE TABLE stack_decisions (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  revision INTEGER NOT NULL CHECK (revision > 0),
  source TEXT NOT NULL CHECK (source IN ('USER','EXISTING','MODEL')),
  selected TEXT NOT NULL,
  content_json TEXT NOT NULL,
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
  created_at TEXT NOT NULL,
  UNIQUE (request_id, revision)
);

CREATE TABLE architecture_plans (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  revision INTEGER NOT NULL CHECK (revision > 0),
  content_json TEXT NOT NULL,
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
  created_at TEXT NOT NULL,
  UNIQUE (request_id, revision)
);

CREATE TABLE documentation_plans (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  revision INTEGER NOT NULL CHECK (revision > 0),
  content_json TEXT NOT NULL,
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
  created_at TEXT NOT NULL,
  UNIQUE (request_id, revision)
);

CREATE TABLE skill_packs (
  id TEXT PRIMARY KEY,
  technology TEXT NOT NULL,
  version TEXT NOT NULL,
  content_json TEXT NOT NULL,
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
  source_urls_json TEXT NOT NULL,
  last_verified_at TEXT NOT NULL,
  review_after TEXT NOT NULL,
  license TEXT NOT NULL,
  provenance TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (technology, version, content_sha256)
);

CREATE TABLE skill_plans (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  revision INTEGER NOT NULL CHECK (revision > 0),
  content_json TEXT NOT NULL,
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
  created_at TEXT NOT NULL,
  UNIQUE (request_id, revision)
);

CREATE TABLE definition_of_done (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  revision INTEGER NOT NULL CHECK (revision > 0),
  content_json TEXT NOT NULL,
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
  created_at TEXT NOT NULL,
  UNIQUE (request_id, revision)
);

CREATE TABLE reviewer_plans (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  revision INTEGER NOT NULL CHECK (revision > 0),
  content_json TEXT NOT NULL,
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
  created_at TEXT NOT NULL,
  UNIQUE (request_id, revision)
);

CREATE TABLE finding_dispositions (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  candidate_id TEXT,
  finding_id TEXT NOT NULL,
  route TEXT NOT NULL CHECK (route IN (
    'REPAIRER','TEST_PLANNER','PLANNER','SPEC_MANAGER','SECURITY_REVIEWER','DOCUMENTATION'
  )),
  status TEXT NOT NULL CHECK (status IN ('OPEN','RESOLVED','REFUTED')),
  resolved_by TEXT NOT NULL DEFAULT '',
  evidence_refs_json TEXT NOT NULL DEFAULT '[]',
  owner_decision TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (request_id, finding_id)
);

CREATE TABLE dependency_plans (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  revision INTEGER NOT NULL CHECK (revision > 0),
  strategy TEXT NOT NULL CHECK (strategy IN (
    'NOT_REQUIRED','VENDOR_CACHE','BROKERED_FETCH','BOTH'
  )),
  content_json TEXT NOT NULL,
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
  sbom_ref TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  UNIQUE (request_id, revision)
);

CREATE TABLE openspec_projections (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  change_id TEXT NOT NULL,
  revision INTEGER NOT NULL CHECK (revision > 0),
  proposal_sha256 TEXT NOT NULL CHECK (length(proposal_sha256) = 64),
  specs_sha256 TEXT NOT NULL CHECK (length(specs_sha256) = 64),
  design_sha256 TEXT NOT NULL DEFAULT '',
  tasks_sha256 TEXT NOT NULL CHECK (length(tasks_sha256) = 64),
  validated INTEGER NOT NULL DEFAULT 0 CHECK (validated IN (0,1)),
  archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (request_id, revision)
);

CREATE TABLE workflow_runs (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  campaign_id TEXT,
  state TEXT NOT NULL,
  revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
  plan_sha256 TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX workflow_runs_request_idx ON workflow_runs(request_id);

CREATE TABLE workflow_steps (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES workflow_runs(id),
  kind TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN (
    'PENDING','RUNNING','SUCCEEDED','FAILED','BLOCKED'
  )),
  attempt INTEGER NOT NULL DEFAULT 0 CHECK (attempt >= 0),
  input_sha256 TEXT NOT NULL DEFAULT '',
  output_sha256 TEXT NOT NULL DEFAULT '',
  detail TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX workflow_steps_run_idx ON workflow_steps(run_id, created_at);

CREATE TABLE workflow_checkpoints (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES workflow_runs(id),
  unit_id TEXT NOT NULL DEFAULT '',
  state_json TEXT NOT NULL,
  state_sha256 TEXT NOT NULL CHECK (length(state_sha256) = 64),
  created_at TEXT NOT NULL
);
CREATE INDEX workflow_checkpoints_run_idx ON workflow_checkpoints(run_id, created_at);

CREATE TABLE idempotency_keys (
  key TEXT PRIMARY KEY,
  operation TEXT NOT NULL,
  result_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  expires_at TEXT NOT NULL
);

CREATE TABLE outbox (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  payload_sha256 TEXT NOT NULL CHECK (length(payload_sha256) = 64),
  status TEXT NOT NULL CHECK (status IN ('PENDING','SENT','FAILED','RECONCILED')),
  attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  last_error TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX outbox_status_idx ON outbox(status, created_at);

CREATE TABLE external_effects (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  external_id TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('INTENDED','APPLIED','RECONCILED','FAILED')),
  payload_sha256 TEXT NOT NULL CHECK (length(payload_sha256) = 64),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (kind, external_id)
);
