-- Durable subscription funding evidence and quota recovery state.
-- Rows are authorization evidence only; this milestone intentionally leaves
-- provider execution disabled until admission enforcement is implemented.
CREATE TABLE subscription_funding_policies (
  campaign_id TEXT PRIMARY KEY REFERENCES campaigns(id) ON DELETE RESTRICT,
  authorization_id TEXT NOT NULL UNIQUE CHECK (length(authorization_id) > 0),
  policy_json TEXT NOT NULL CHECK (length(policy_json) > 0),
  policy_sha256 TEXT NOT NULL CHECK (length(policy_sha256) = 64),
  state TEXT NOT NULL CHECK (state IN ('DISABLED', 'PAUSED_QUOTA', 'RECOVERABLE')),
  reason TEXT,
  observed_at_ms INTEGER NOT NULL CHECK (observed_at_ms > 0),
  resets_at_ms INTEGER CHECK (resets_at_ms IS NULL OR resets_at_ms > 0),
  created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
  updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms),
  CHECK (json_valid(policy_json) AND json_extract(policy_json, '$.funding.kind') = 'SUBSCRIPTION'),
  CHECK ((state = 'PAUSED_QUOTA') = (reason IS NOT NULL))
);

CREATE TRIGGER subscription_funding_policy_identity_immutable
BEFORE UPDATE ON subscription_funding_policies
WHEN OLD.campaign_id <> NEW.campaign_id
  OR OLD.authorization_id <> NEW.authorization_id
  OR OLD.policy_json <> NEW.policy_json
  OR OLD.policy_sha256 <> NEW.policy_sha256
  OR OLD.created_at_ms <> NEW.created_at_ms
  OR (OLD.state = 'PAUSED_QUOTA' AND NEW.state = 'DISABLED')
BEGIN SELECT RAISE(ABORT, 'subscription funding policy identity/state is immutable'); END;

CREATE TRIGGER subscription_funding_policy_immutable_delete
BEFORE DELETE ON subscription_funding_policies
BEGIN SELECT RAISE(ABORT, 'subscription funding policy is append-only'); END;

-- The proposal cannot coexist with executable V2 authorization, including
-- when a second Core process races the Rust preflight checks.
CREATE TRIGGER subscription_funding_policy_requires_unapproved_campaign
BEFORE INSERT ON subscription_funding_policies
WHEN EXISTS (SELECT 1 FROM campaign_authorizations ca WHERE ca.campaign_id = NEW.campaign_id)
  OR NOT EXISTS (SELECT 1 FROM campaigns c
                 WHERE c.id = NEW.campaign_id AND c.status = 'BLOCKED_AUTHORIZATION')
BEGIN SELECT RAISE(ABORT, 'subscription funding proposal requires an unauthorized pending campaign'); END;

CREATE TRIGGER campaign_authorization_excludes_subscription_proposal
BEFORE INSERT ON campaign_authorizations
WHEN EXISTS (SELECT 1 FROM subscription_funding_policies sfp WHERE sfp.campaign_id = NEW.campaign_id)
BEGIN SELECT RAISE(ABORT, 'subscription funding proposal must be resolved before execution authorization'); END;
