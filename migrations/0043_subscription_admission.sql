-- Owner-confirmed subscription admission.
--
-- The earlier schema recognized only a disabled proposal, a durable quota
-- pause and an unrecoverable recovery state. The corrected policy admits a
-- bounded test execution after an authenticated operator records the Owner's
-- upstream Extra Usage assumption. This is not provider-issued evidence.
--
-- ENABLED is the only state that authorizes dispatch. It still requires the
-- Owner-confirmed admission basis in the policy JSON, and the trigger below
-- prevents a provider-verified-only proposal from being enabled through this
-- path. PAUSED_QUOTA remains terminal until a reviewed recovery contract.

DROP TRIGGER IF EXISTS campaign_authorization_excludes_subscription_proposal;
DROP TRIGGER IF EXISTS subscription_funding_policy_requires_unapproved_campaign;
DROP TRIGGER IF EXISTS subscription_funding_policy_identity_immutable;
DROP TRIGGER IF EXISTS subscription_funding_policy_immutable_delete;

CREATE TABLE subscription_funding_policies_admission (
  campaign_id TEXT PRIMARY KEY REFERENCES campaigns(id) ON DELETE RESTRICT,
  authorization_id TEXT NOT NULL UNIQUE CHECK (length(authorization_id) > 0),
  policy_json TEXT NOT NULL CHECK (length(policy_json) > 0),
  policy_sha256 TEXT NOT NULL CHECK (length(policy_sha256) = 64),
  state TEXT NOT NULL CHECK (state IN ('DISABLED', 'ENABLED', 'PAUSED_QUOTA', 'RECOVERABLE')),
  reason TEXT,
  observed_at_ms INTEGER NOT NULL CHECK (observed_at_ms > 0),
  resets_at_ms INTEGER CHECK (resets_at_ms IS NULL OR resets_at_ms > 0),
  created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
  updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms),
  CHECK (json_valid(policy_json) AND json_extract(policy_json, '$.funding.kind') = 'SUBSCRIPTION'),
  CHECK ((state = 'DISABLED') = (reason IS NULL)),
  CHECK (state = 'PAUSED_QUOTA' OR resets_at_ms IS NULL),
  CHECK (reason IS NULL OR length(reason) <= 512),
  CHECK (
    state <> 'ENABLED'
    OR json_extract(policy_json, '$.funding.binding.admissionBasis.basis') = 'OWNER_CONFIRMED_CONFIGURATION'
  )
);

INSERT INTO subscription_funding_policies_admission
  (campaign_id, authorization_id, policy_json, policy_sha256, state, reason,
   observed_at_ms, resets_at_ms, created_at_ms, updated_at_ms)
SELECT campaign_id, authorization_id, policy_json, policy_sha256, state, reason,
       observed_at_ms, resets_at_ms, created_at_ms, updated_at_ms
FROM subscription_funding_policies;

DROP TABLE subscription_funding_policies;
ALTER TABLE subscription_funding_policies_admission RENAME TO subscription_funding_policies;

CREATE TRIGGER subscription_funding_policy_identity_immutable
BEFORE UPDATE ON subscription_funding_policies
WHEN OLD.campaign_id <> NEW.campaign_id
  OR OLD.authorization_id <> NEW.authorization_id
  OR OLD.policy_json <> NEW.policy_json
  OR OLD.policy_sha256 <> NEW.policy_sha256
  OR OLD.created_at_ms <> NEW.created_at_ms
  OR (OLD.state IN ('ENABLED', 'PAUSED_QUOTA')
      AND NEW.state <> OLD.state
      AND NOT (OLD.state = 'ENABLED' AND NEW.state = 'PAUSED_QUOTA'))
BEGIN SELECT RAISE(ABORT, 'subscription funding policy identity/state is immutable'); END;

CREATE TRIGGER subscription_funding_policy_immutable_delete
BEFORE DELETE ON subscription_funding_policies
BEGIN SELECT RAISE(ABORT, 'subscription funding policy is append-only'); END;

CREATE TRIGGER subscription_funding_policy_requires_unapproved_campaign
BEFORE INSERT ON subscription_funding_policies
WHEN EXISTS (SELECT 1 FROM campaign_authorizations ca WHERE ca.campaign_id = NEW.campaign_id)
  OR NOT EXISTS (SELECT 1 FROM campaigns c
                 WHERE c.id = NEW.campaign_id AND c.status = 'BLOCKED_AUTHORIZATION')
BEGIN SELECT RAISE(ABORT, 'subscription funding admission requires an unauthorized pending campaign'); END;

CREATE TRIGGER campaign_authorization_excludes_subscription_proposal
BEFORE INSERT ON campaign_authorizations
WHEN EXISTS (
  SELECT 1 FROM subscription_funding_policies sfp
  WHERE sfp.campaign_id = NEW.campaign_id AND sfp.state <> 'ENABLED'
)
BEGIN SELECT RAISE(ABORT, 'subscription funding proposal must be enabled before execution authorization'); END;

-- A request-scoped admission covers the pre-Campaign Lead assessment. It binds
-- the exact intake request, Lead request, execution profile revision, configured
-- assessment/implementation routes and the Owner assumption. Approval consumes
-- it into a Campaign-scoped ENABLED policy in the same transaction that creates
-- the Campaign authorization.
CREATE TABLE IF NOT EXISTS subscription_scope_admissions (
  admission_id TEXT PRIMARY KEY,
  intake_request_id TEXT NOT NULL REFERENCES intake_requests(id) ON DELETE RESTRICT,
  lead_request_id TEXT NOT NULL REFERENCES lead_requests(id) ON DELETE RESTRICT,
  test_profile_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  settings_revision TEXT NOT NULL,
  profile_revision_id TEXT NOT NULL,
  profile_hash TEXT NOT NULL,
  assumption_json TEXT NOT NULL,
  assumption_sha256 TEXT NOT NULL,
  routes_json TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('ACTIVE', 'CONSUMED', 'REVOKED', 'EXPIRED')),
  campaign_id TEXT REFERENCES campaigns(id),
  created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
  expires_at_ms INTEGER NOT NULL CHECK (expires_at_ms > created_at_ms),
  consumed_at_ms INTEGER,
  idempotency_key TEXT NOT NULL UNIQUE,
  receipt_sha256 TEXT NOT NULL CHECK (length(receipt_sha256) = 64),
  CHECK ((state = 'CONSUMED') = (campaign_id IS NOT NULL)),
  CHECK ((state = 'CONSUMED') = (consumed_at_ms IS NOT NULL)),
  CHECK (json_valid(assumption_json)),
  CHECK (json_valid(routes_json))
);
CREATE INDEX IF NOT EXISTS subscription_scope_admissions_request_idx
  ON subscription_scope_admissions(intake_request_id, lead_request_id, state);

CREATE TRIGGER IF NOT EXISTS subscription_scope_admission_identity_immutable
BEFORE UPDATE ON subscription_scope_admissions
WHEN OLD.admission_id <> NEW.admission_id
  OR OLD.intake_request_id <> NEW.intake_request_id
  OR OLD.lead_request_id <> NEW.lead_request_id
  OR OLD.test_profile_id <> NEW.test_profile_id
  OR OLD.provider <> NEW.provider
  OR OLD.model <> NEW.model
  OR OLD.settings_revision <> NEW.settings_revision
  OR OLD.profile_revision_id <> NEW.profile_revision_id
  OR OLD.profile_hash <> NEW.profile_hash
  OR OLD.assumption_json <> NEW.assumption_json
  OR OLD.assumption_sha256 <> NEW.assumption_sha256
  OR OLD.routes_json <> NEW.routes_json
  OR OLD.created_at_ms <> NEW.created_at_ms
  OR OLD.expires_at_ms <> NEW.expires_at_ms
  OR OLD.idempotency_key <> NEW.idempotency_key
BEGIN SELECT RAISE(ABORT, 'subscription scope admission identity is immutable'); END;
