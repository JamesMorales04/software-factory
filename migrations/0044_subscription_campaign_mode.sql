-- Subscription campaigns have no monetary envelope. Extend the Campaign
-- inference classification without changing V1/V2 rows.
CREATE TABLE inference_campaign_modes_subscription (
  campaign_id TEXT PRIMARY KEY REFERENCES campaigns(id) ON DELETE RESTRICT,
  mode TEXT NOT NULL CHECK (mode IN ('V1', 'V2', 'SUBSCRIPTION')),
  classified_at_ms INTEGER NOT NULL CHECK (classified_at_ms > 0),
  evidence_sha256 TEXT NOT NULL CHECK (length(evidence_sha256) = 64)
);

INSERT INTO inference_campaign_modes_subscription
  (campaign_id, mode, classified_at_ms, evidence_sha256)
SELECT campaign_id, mode, classified_at_ms, evidence_sha256
FROM inference_campaign_modes;

DROP TABLE inference_campaign_modes;
ALTER TABLE inference_campaign_modes_subscription RENAME TO inference_campaign_modes;
