-- Durable Lead request, assessment, proposal and frozen-plan identity.
-- Recording a request or plan does not authorize Campaign start.

CREATE TABLE IF NOT EXISTS lead_requests (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id),
  action_id TEXT NOT NULL UNIQUE REFERENCES actions(id),
  delegation_id TEXT NOT NULL REFERENCES standing_delegations(id),
  objective TEXT NOT NULL,
  request_hash TEXT NOT NULL,
  project_config_hash TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN
    ('SUBMITTED','ASSESSING','PLANNED','FROZEN','READY','RUNNING','BLOCKED','FAILED')),
  failure_reason TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS lead_requests_state_idx ON lead_requests(state);

CREATE TRIGGER IF NOT EXISTS lead_requests_immutable_content
BEFORE UPDATE ON lead_requests
WHEN OLD.id != NEW.id
  OR OLD.project_id != NEW.project_id
  OR OLD.action_id != NEW.action_id
  OR OLD.delegation_id != NEW.delegation_id
  OR OLD.objective != NEW.objective
  OR OLD.request_hash != NEW.request_hash
  OR OLD.project_config_hash != NEW.project_config_hash
  OR OLD.created_at != NEW.created_at
BEGIN
  SELECT RAISE(ABORT, 'lead request content is immutable');
END;

CREATE TABLE IF NOT EXISTS lead_assessments (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL UNIQUE REFERENCES lead_requests(id),
  mechanism TEXT NOT NULL,
  assessment_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS lead_assessments_immutable_update
BEFORE UPDATE ON lead_assessments BEGIN SELECT RAISE(ABORT, 'lead assessment is immutable'); END;
CREATE TRIGGER IF NOT EXISTS lead_assessments_immutable_delete
BEFORE DELETE ON lead_assessments BEGIN SELECT RAISE(ABORT, 'lead assessment is immutable'); END;

CREATE TABLE IF NOT EXISTS lead_proposals (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL UNIQUE REFERENCES lead_requests(id),
  assessment_id TEXT NOT NULL REFERENCES lead_assessments(id),
  proposal_json TEXT NOT NULL,
  proposal_hash TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS lead_proposals_immutable_update
BEFORE UPDATE ON lead_proposals BEGIN SELECT RAISE(ABORT, 'lead proposal is immutable'); END;
CREATE TRIGGER IF NOT EXISTS lead_proposals_immutable_delete
BEFORE DELETE ON lead_proposals BEGIN SELECT RAISE(ABORT, 'lead proposal is immutable'); END;

CREATE TABLE IF NOT EXISTS frozen_lead_plans (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL UNIQUE REFERENCES lead_requests(id),
  proposal_id TEXT NOT NULL REFERENCES lead_proposals(id),
  plan_hash TEXT NOT NULL,
  config_hash TEXT NOT NULL,
  delegation_hash TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS frozen_lead_plans_immutable_update
BEFORE UPDATE ON frozen_lead_plans BEGIN SELECT RAISE(ABORT, 'frozen lead plan is immutable'); END;
CREATE TRIGGER IF NOT EXISTS frozen_lead_plans_immutable_delete
BEFORE DELETE ON frozen_lead_plans
BEGIN SELECT RAISE(ABORT, 'frozen lead plan is immutable'); END;
