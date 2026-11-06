-- Repair the subscription funding recovery-state constraint (0041 was already
-- applied to development databases and is never edited in place).
--
-- 0041 required `reason` exactly for `PAUSED_QUOTA`. That made the decoder's
-- documented `RECOVERABLE { reason }` state unrepresentable: a valid recovery
-- row could never satisfy both the CHECK and the decoder. The corrected rule is:
--
--   * `DISABLED`         -> reason is absent, no reset observation.
--   * `PAUSED_QUOTA`     -> reason is required, optional future reset.
--   * `RECOVERABLE`      -> reason is required, no reset observation.
--
-- This is a pure table rebuild because SQLite cannot alter a CHECK constraint.
-- The object set is otherwise unchanged: the same columns, primary key,
-- uniqueness, foreign key, JSON discriminator and the three triggers from 0041
-- are recreated under their original names. Existing rows are preserved and the
-- repair never invents a reason or reset timestamp.
--
-- Quota recovery remains unavailable: this migration only makes the durable
-- `RECOVERABLE` evidence state decodable. No code transitions a paused row to
-- it, and the identity trigger still refuses any transition out of a durable
-- `PAUSED_QUOTA` hold (strengthened below from only `PAUSED_QUOTA`->`DISABLED`).

CREATE TABLE subscription_funding_policies_repair (
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
  -- A non-disabled recovery state must carry an operator-readable reason, and
  -- only a quota pause may carry a reset observation.
  CHECK ((state = 'DISABLED') = (reason IS NULL)),
  CHECK (state = 'PAUSED_QUOTA' OR resets_at_ms IS NULL),
  CHECK (reason IS NULL OR length(reason) <= 512)
);

INSERT INTO subscription_funding_policies_repair
  (campaign_id, authorization_id, policy_json, policy_sha256, state, reason,
   observed_at_ms, resets_at_ms, created_at_ms, updated_at_ms)
SELECT campaign_id, authorization_id, policy_json, policy_sha256, state, reason,
       observed_at_ms, resets_at_ms, created_at_ms, updated_at_ms
FROM subscription_funding_policies;

-- This trigger lives on another table but references the rebuilt table, so it
-- is dropped before the rebuild and recreated with the same definition below.
DROP TRIGGER IF EXISTS campaign_authorization_excludes_subscription_proposal;
DROP TABLE subscription_funding_policies;
ALTER TABLE subscription_funding_policies_repair RENAME TO subscription_funding_policies;

CREATE TRIGGER subscription_funding_policy_identity_immutable
BEFORE UPDATE ON subscription_funding_policies
WHEN OLD.campaign_id <> NEW.campaign_id
  OR OLD.authorization_id <> NEW.authorization_id
  OR OLD.policy_json <> NEW.policy_json
  OR OLD.policy_sha256 <> NEW.policy_sha256
  OR OLD.created_at_ms <> NEW.created_at_ms
  -- A durable quota pause is terminal until a reviewed recovery contract
  -- exists. No transition out of PAUSED_QUOTA is admitted.
  OR (OLD.state = 'PAUSED_QUOTA' AND NEW.state <> 'PAUSED_QUOTA')
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
