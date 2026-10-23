-- Explicit, risk-classed Campaign execution authorization.
-- Only deterministic fixture execution is enabled by policy in this slice.
-- Provider and isolated modes remain unavailable until their gates exist.

CREATE TABLE IF NOT EXISTS campaign_authorizations (
  campaign_id TEXT PRIMARY KEY REFERENCES campaigns(id),
  authorization_id TEXT NOT NULL UNIQUE,
  risk_class TEXT NOT NULL CHECK (risk_class = 'R0'),
  execution_mode TEXT NOT NULL CHECK (execution_mode IN
    ('FIXTURE','TRUSTED_LOCAL_PROVIDER','ISOLATED')),
  actor TEXT NOT NULL,
  rationale TEXT NOT NULL,
  authorized_paths_json TEXT NOT NULL,
  max_attempts INTEGER NOT NULL CHECK (max_attempts BETWEEN 1 AND 5),
  created_at TEXT NOT NULL
);
CREATE TRIGGER IF NOT EXISTS campaign_authorizations_immutable_update
BEFORE UPDATE ON campaign_authorizations
BEGIN SELECT RAISE(ABORT, 'campaign authorization is immutable'); END;
CREATE TRIGGER IF NOT EXISTS campaign_authorizations_immutable_delete
BEFORE DELETE ON campaign_authorizations
BEGIN SELECT RAISE(ABORT, 'campaign authorization is immutable'); END;
