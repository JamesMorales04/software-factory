-- Validate the historical campaign bindings that migration 0037 classified as
-- V2. A binding is only V2 when the Campaign's action project equals the
-- envelope project and the Campaign's owning lead/intake request equals the
-- envelope request. Unverifiable bindings are quarantined and fail closed;
-- their rows and original binding evidence are preserved.

CREATE TABLE IF NOT EXISTS inference_campaign_binding_quarantine (
  campaign_id TEXT PRIMARY KEY REFERENCES campaigns(id) ON DELETE RESTRICT,
  envelope_id TEXT NOT NULL REFERENCES inference_envelopes(id) ON DELETE RESTRICT,
  reason TEXT NOT NULL CHECK (length(reason) > 0),
  detail_json TEXT NOT NULL CHECK (length(detail_json) > 0),
  quarantined_at_ms INTEGER NOT NULL CHECK (quarantined_at_ms > 0)
);

CREATE INDEX IF NOT EXISTS inference_campaign_binding_quarantine_envelope_idx
  ON inference_campaign_binding_quarantine(envelope_id);

INSERT OR IGNORE INTO inference_campaign_binding_quarantine
  (campaign_id,envelope_id,reason,detail_json,quarantined_at_ms)
SELECT b.campaign_id,
       b.envelope_id,
       CASE WHEN a.project_id <> e.project_id
            THEN 'PROJECT_MISMATCH'
            ELSE 'REQUEST_MISMATCH'
       END,
       json_object(
         'campaignProject', a.project_id,
         'envelopeProject', e.project_id,
         'envelopeRequest', e.request_id
       ),
       CAST(strftime('%s','now') AS INTEGER) * 1000
FROM inference_campaign_bindings b
JOIN campaigns c ON c.id = b.campaign_id
JOIN actions a ON a.id = c.action_id
JOIN inference_envelopes e ON e.id = b.envelope_id
WHERE a.project_id <> e.project_id
   OR NOT (
     EXISTS (
       SELECT 1 FROM lead_start_evidence l
       LEFT JOIN intake_requests ir ON ir.lead_request_id = l.request_id
       WHERE l.campaign_id = b.campaign_id
         AND e.request_id IN (l.request_id, ir.id)
     )
     OR EXISTS (
       SELECT 1 FROM intake_requests ir2
       WHERE ir2.campaign_id = b.campaign_id
         AND ir2.id = e.request_id
     )
   );

-- Reassert V2 only for bindings that passed the ownership/request check.
INSERT OR IGNORE INTO inference_campaign_modes
  (campaign_id,mode,classified_at_ms,evidence_sha256)
SELECT b.campaign_id,'V2',CAST(strftime('%s','now') AS INTEGER) * 1000,
       '0000000000000000000000000000000000000000000000000000000000000000'
FROM inference_campaign_bindings b
WHERE NOT EXISTS (
  SELECT 1 FROM inference_campaign_binding_quarantine q
  WHERE q.campaign_id = b.campaign_id
);

CREATE TRIGGER IF NOT EXISTS inference_campaign_binding_quarantine_immutable_update
BEFORE UPDATE ON inference_campaign_binding_quarantine
BEGIN
  SELECT RAISE(ABORT, 'inference campaign binding quarantine is immutable');
END;

CREATE TRIGGER IF NOT EXISTS inference_campaign_binding_quarantine_immutable_delete
BEFORE DELETE ON inference_campaign_binding_quarantine
BEGIN
  SELECT RAISE(ABORT, 'inference campaign binding quarantine is immutable');
END;
