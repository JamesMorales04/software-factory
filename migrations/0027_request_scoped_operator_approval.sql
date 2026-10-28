-- Request-scoped operator approval, deferred executable ActionSpecs and
-- authoritative Lead withdrawal fencing.
--
-- This migration adds immutable evidence only. It does not authorize any
-- existing Action, Campaign or Lead request, and it does not change the
-- deployment-wide autonomous-start policy. Profile revision/hash columns are
-- nullable so the later versioned-profile lane can bind them without a
-- destructive rewrite. No profile foreign key is added here.

-- An intake Action is created with an assessment draft and no action_specs row.
-- The first executable ActionSpec is derived and inserted once at freeze; the
-- draft remains immutable provenance. Existing ActionSpec rows are untouched.
CREATE TABLE IF NOT EXISTS action_drafts (
  action_id TEXT PRIMARY KEY REFERENCES actions(id),
  assessment_input_json TEXT NOT NULL,
  input_sha256 TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TRIGGER IF NOT EXISTS action_drafts_immutable_update
BEFORE UPDATE ON action_drafts
BEGIN SELECT RAISE(ABORT, 'action draft is immutable'); END;

CREATE TRIGGER IF NOT EXISTS action_drafts_immutable_delete
BEFORE DELETE ON action_drafts
BEGIN SELECT RAISE(ABORT, 'action draft is immutable'); END;

-- A draft may be created for an Action that has no executable spec. It may
-- never be created beside an existing spec; freeze is the only one-way
-- draft-to-spec transition and is performed by Core in the same transaction
-- as the frozen Lead plan.
CREATE TRIGGER IF NOT EXISTS action_drafts_require_absent_spec
BEFORE INSERT ON action_drafts
WHEN EXISTS (SELECT 1 FROM action_specs WHERE action_id = NEW.action_id)
BEGIN
  SELECT RAISE(ABORT, 'action draft cannot coexist with an initial ActionSpec');
END;

-- Attempt specs were already immutable in practice (execution reads them by
-- hash); make that byte-level guarantee explicit for upgraded databases.
CREATE TRIGGER IF NOT EXISTS attempt_specs_immutable_update
BEFORE UPDATE ON attempt_specs
BEGIN SELECT RAISE(ABORT, 'attempt spec is immutable'); END;

CREATE TRIGGER IF NOT EXISTS attempt_specs_immutable_delete
BEFORE DELETE ON attempt_specs
BEGIN SELECT RAISE(ABORT, 'attempt spec is immutable'); END;

-- The Campaign is permanently bound to the root ActionSpec of its frozen Lead
-- plan. Attempt specs remain the exact execution payloads.
CREATE TABLE IF NOT EXISTS campaign_execution_bindings (
  campaign_id TEXT PRIMARY KEY REFERENCES campaigns(id),
  request_id TEXT NOT NULL REFERENCES lead_requests(id),
  plan_id TEXT NOT NULL UNIQUE REFERENCES frozen_lead_plans(id),
  spec_sha256 TEXT NOT NULL,
  envelope_hash TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS campaign_execution_bindings_request_idx
  ON campaign_execution_bindings(request_id);

CREATE TRIGGER IF NOT EXISTS campaign_execution_bindings_immutable_update
BEFORE UPDATE ON campaign_execution_bindings
BEGIN SELECT RAISE(ABORT, 'campaign execution binding is immutable'); END;

CREATE TRIGGER IF NOT EXISTS campaign_execution_bindings_immutable_delete
BEFORE DELETE ON campaign_execution_bindings
BEGIN SELECT RAISE(ABORT, 'campaign execution binding is immutable'); END;

-- Immutable approval/start evidence. One Lead request can produce exactly one
-- Campaign, and one idempotency key can identify exactly one start. The exact
-- request/plan/spec/config/delegation binding is stored so replay never spends
-- again and changed bindings or a different principal conflict.
CREATE TABLE IF NOT EXISTS lead_start_evidence (
  request_id TEXT PRIMARY KEY REFERENCES lead_requests(id),
  campaign_id TEXT NOT NULL UNIQUE REFERENCES campaigns(id),
  mode TEXT NOT NULL CHECK (mode IN ('AUTONOMOUS', 'OPERATOR')),
  principal_id TEXT NOT NULL,
  principal_label TEXT,
  rationale TEXT NOT NULL,
  idempotency_key TEXT NOT NULL UNIQUE,
  request_hash TEXT NOT NULL,
  plan_hash TEXT NOT NULL,
  profile_revision_id TEXT,
  profile_hash TEXT,
  spec_sha256 TEXT NOT NULL,
  config_hash TEXT NOT NULL,
  delegation_hash TEXT NOT NULL,
  policy_hash TEXT NOT NULL,
  authorization_digest TEXT NOT NULL,
  created_at TEXT NOT NULL,
  CHECK (
    (profile_revision_id IS NULL AND profile_hash IS NULL)
    OR (profile_revision_id IS NOT NULL AND profile_hash IS NOT NULL)
  )
);
CREATE INDEX IF NOT EXISTS lead_start_evidence_campaign_idx
  ON lead_start_evidence(campaign_id);

CREATE TRIGGER IF NOT EXISTS lead_start_evidence_immutable_update
BEFORE UPDATE ON lead_start_evidence
BEGIN SELECT RAISE(ABORT, 'lead start evidence is immutable'); END;

CREATE TRIGGER IF NOT EXISTS lead_start_evidence_immutable_delete
BEFORE DELETE ON lead_start_evidence
BEGIN SELECT RAISE(ABORT, 'lead start evidence is immutable'); END;

-- Authoritative withdrawal fact. It is not a second lifecycle and never
-- cancels an already-authorized Campaign. Withdrawal and start serialize on
-- SQLite's write transaction; Campaign/gate outcomes take projection
-- precedence over this fact.
CREATE TABLE IF NOT EXISTS lead_withdrawals (
  request_id TEXT PRIMARY KEY REFERENCES lead_requests(id),
  principal_id TEXT NOT NULL,
  principal_label TEXT,
  reason TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TRIGGER IF NOT EXISTS lead_withdrawals_immutable_update
BEFORE UPDATE ON lead_withdrawals
BEGIN SELECT RAISE(ABORT, 'lead withdrawal is immutable'); END;

CREATE TRIGGER IF NOT EXISTS lead_withdrawals_immutable_delete
BEFORE DELETE ON lead_withdrawals
BEGIN SELECT RAISE(ABORT, 'lead withdrawal is immutable'); END;
