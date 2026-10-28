-- Unified intake requests, append-only operator messages, frozen execution
-- contexts and explicit execution-policy grants.
--
-- Intake does not own a second Lead or Campaign lifecycle. State is derived
-- from lead_requests, lead_withdrawals, campaigns and gate decisions. The
-- intake row stores only the immutable submission snapshot and links to the
-- authoritative records it created through the shared Lead boundary.

CREATE TABLE IF NOT EXISTS intake_requests (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id),
  objective TEXT NOT NULL,
  mode TEXT NOT NULL CHECK (mode IN ('REVIEW_PLAN', 'AUTONOMOUS')),
  profile_id TEXT NOT NULL REFERENCES execution_profiles(id),
  profile_revision_id TEXT NOT NULL REFERENCES execution_profile_revisions(revision_id),
  profile_revision INTEGER NOT NULL CHECK (profile_revision > 0),
  profile_hash TEXT NOT NULL,
  profile_name TEXT NOT NULL,
  action_id TEXT NOT NULL UNIQUE REFERENCES actions(id),
  delegation_id TEXT NOT NULL REFERENCES standing_delegations(id),
  lead_request_id TEXT NOT NULL UNIQUE REFERENCES lead_requests(id),
  campaign_id TEXT REFERENCES campaigns(id),
  submission_json TEXT NOT NULL,
  submission_hash TEXT NOT NULL,
  idempotency_key TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS intake_requests_project_created_idx
  ON intake_requests(project_id, created_at DESC);

CREATE INDEX IF NOT EXISTS intake_requests_lead_idx
  ON intake_requests(lead_request_id);

-- The submission snapshot is immutable. Only the authoritative Campaign link
-- and updated_at may change after creation, and the link is written by the
-- shared Lead start transaction.
CREATE TRIGGER IF NOT EXISTS intake_requests_immutable_submission
BEFORE UPDATE ON intake_requests
WHEN OLD.id <> NEW.id
  OR OLD.project_id <> NEW.project_id
  OR OLD.objective <> NEW.objective
  OR OLD.mode <> NEW.mode
  OR OLD.profile_id <> NEW.profile_id
  OR OLD.profile_revision_id <> NEW.profile_revision_id
  OR OLD.profile_revision <> NEW.profile_revision
  OR OLD.profile_hash <> NEW.profile_hash
  OR OLD.profile_name <> NEW.profile_name
  OR OLD.action_id <> NEW.action_id
  OR OLD.delegation_id <> NEW.delegation_id
  OR OLD.lead_request_id <> NEW.lead_request_id
  OR (OLD.campaign_id IS NOT NULL AND (NEW.campaign_id IS NULL OR OLD.campaign_id <> NEW.campaign_id))
  OR OLD.submission_json <> NEW.submission_json
  OR OLD.submission_hash <> NEW.submission_hash
  OR OLD.idempotency_key <> NEW.idempotency_key
  OR OLD.created_at <> NEW.created_at
BEGIN
  SELECT RAISE(ABORT, 'intake request submission is immutable');
END;

CREATE TRIGGER IF NOT EXISTS intake_requests_immutable_delete
BEFORE DELETE ON intake_requests
BEGIN
  SELECT RAISE(ABORT, 'intake request is immutable');
END;

-- Append-only operator messages. A message never mutates a frozen plan. The
-- endpoint refuses messages once a plan is frozen; this table only records
-- pre-freeze operator context and is never execution evidence.
CREATE TABLE IF NOT EXISTS intake_messages (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  role TEXT NOT NULL CHECK (role = 'operator'),
  body TEXT NOT NULL,
  body_sha256 TEXT NOT NULL,
  idempotency_key TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS intake_messages_request_created_idx
  ON intake_messages(request_id, created_at, id);

CREATE TRIGGER IF NOT EXISTS intake_messages_immutable_update
BEFORE UPDATE ON intake_messages
BEGIN
  SELECT RAISE(ABORT, 'intake message is append-only');
END;

CREATE TRIGGER IF NOT EXISTS intake_messages_immutable_delete
BEFORE DELETE ON intake_messages
BEGIN
  SELECT RAISE(ABORT, 'intake message is append-only');
END;

-- Exact profile snapshot for a unified-intake Lead request. This is the
-- request execution context; later profile edits never substitute a newer
-- revision into an existing request. Historical Lead requests have no row.
CREATE TABLE IF NOT EXISTS lead_execution_context (
  request_id TEXT PRIMARY KEY REFERENCES lead_requests(id),
  project_id TEXT NOT NULL REFERENCES projects(id),
  profile_id TEXT NOT NULL REFERENCES execution_profiles(id),
  profile_revision_id TEXT NOT NULL REFERENCES execution_profile_revisions(revision_id),
  profile_revision INTEGER NOT NULL CHECK (profile_revision > 0),
  profile_hash TEXT NOT NULL,
  profile_name TEXT NOT NULL,
  project_config_hash TEXT NOT NULL,
  delegation_hash TEXT NOT NULL,
  context_json TEXT NOT NULL,
  context_hash TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TRIGGER IF NOT EXISTS lead_execution_context_immutable_update
BEFORE UPDATE ON lead_execution_context
BEGIN
  SELECT RAISE(ABORT, 'lead execution context is immutable');
END;

CREATE TRIGGER IF NOT EXISTS lead_execution_context_immutable_delete
BEFORE DELETE ON lead_execution_context
BEGIN
  SELECT RAISE(ABORT, 'lead execution context is immutable');
END;

-- One explicit operator-authorized execution policy grant. The standing
-- delegation remains the authority record; this row records why it exists,
-- which exact profile revision and project configuration it was derived from,
-- and that the operator acknowledged a non-autonomous grant. Rows are
-- immutable. Re-enabling creates a new delegation after a new explicit
-- acknowledgement; an existing grant is never widened or replenished.
CREATE TABLE IF NOT EXISTS execution_policy_grants (
  delegation_id TEXT PRIMARY KEY REFERENCES standing_delegations(id),
  project_id TEXT NOT NULL REFERENCES projects(id),
  principal_id TEXT NOT NULL,
  profile_id TEXT NOT NULL REFERENCES execution_profiles(id),
  profile_revision_id TEXT NOT NULL REFERENCES execution_profile_revisions(revision_id),
  profile_revision INTEGER NOT NULL CHECK (profile_revision > 0),
  profile_hash TEXT NOT NULL,
  project_config_hash TEXT NOT NULL,
  acknowledged INTEGER NOT NULL CHECK (acknowledged = 1),
  idempotency_key TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS execution_policy_grants_project_idx
  ON execution_policy_grants(project_id, profile_id, profile_revision_id);
CREATE INDEX IF NOT EXISTS execution_policy_grants_profile_idx
  ON execution_policy_grants(profile_id, profile_revision_id);

CREATE TRIGGER IF NOT EXISTS execution_policy_grants_immutable_update
BEFORE UPDATE ON execution_policy_grants
BEGIN
  SELECT RAISE(ABORT, 'execution policy grant is immutable');
END;

CREATE TRIGGER IF NOT EXISTS execution_policy_grants_immutable_delete
BEFORE DELETE ON execution_policy_grants
BEGIN
  SELECT RAISE(ABORT, 'execution policy grant is immutable');
END;

-- Immutable link from a Campaign to the exact frozen profile revision that
-- authorized its implementer route. Historical Campaigns have no row.
CREATE TABLE IF NOT EXISTS campaign_execution_profile_bindings (
  campaign_id TEXT PRIMARY KEY REFERENCES campaigns(id),
  request_id TEXT NOT NULL REFERENCES lead_requests(id),
  profile_revision_id TEXT NOT NULL REFERENCES execution_profile_revisions(revision_id),
  profile_revision INTEGER NOT NULL CHECK (profile_revision > 0),
  profile_hash TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TRIGGER IF NOT EXISTS campaign_execution_profile_bindings_immutable_update
BEFORE UPDATE ON campaign_execution_profile_bindings
BEGIN
  SELECT RAISE(ABORT, 'campaign profile binding is immutable');
END;

CREATE TRIGGER IF NOT EXISTS campaign_execution_profile_bindings_immutable_delete
BEFORE DELETE ON campaign_execution_profile_bindings
BEGIN
  SELECT RAISE(ABORT, 'campaign profile binding is immutable');
END;
