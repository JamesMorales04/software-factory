-- Durable owner charter and reserved-matter decisions.
-- A charter can delegate execution, repair and delivery separately. Policy
-- autonomy is never delegated. Reserved matters remain owner decisions.
CREATE TABLE IF NOT EXISTS agentic_owner_charters (
  id TEXT PRIMARY KEY,
  owner TEXT NOT NULL,
  project_id TEXT NOT NULL REFERENCES projects(id),
  domains_json TEXT NOT NULL,
  risk_appetite TEXT NOT NULL CHECK (risk_appetite IN ('R0','R1','R2','R3')),
  execution_autonomy INTEGER NOT NULL CHECK (execution_autonomy IN (0,1)),
  repair_autonomy INTEGER NOT NULL CHECK (repair_autonomy IN (0,1)),
  delivery_autonomy INTEGER NOT NULL CHECK (delivery_autonomy IN (0,1)),
  policy_autonomy INTEGER NOT NULL DEFAULT 0 CHECK (policy_autonomy = 0),
  reserved_matters_json TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  revoked_at TEXT,
  revocation_reason TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS agentic_owner_charters_project_idx
  ON agentic_owner_charters(project_id, revoked_at, expires_at);

CREATE TABLE IF NOT EXISTS agentic_owner_decisions (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id),
  request_id TEXT REFERENCES intake_requests(id),
  campaign_id TEXT REFERENCES campaigns(id),
  matter TEXT NOT NULL,
  risk TEXT NOT NULL CHECK (risk IN ('R0','R1','R2','R3')),
  status TEXT NOT NULL CHECK (status IN ('PENDING','APPROVED','DENIED','EXPIRED')),
  options_json TEXT NOT NULL,
  evidence_json TEXT NOT NULL,
  recommendation TEXT NOT NULL,
  deadline TEXT NOT NULL,
  selected_option TEXT,
  rationale TEXT,
  resolved_by TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  resolved_at TEXT
);

CREATE INDEX IF NOT EXISTS agentic_owner_decisions_status_idx
  ON agentic_owner_decisions(status, deadline);
