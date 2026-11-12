-- Durable delivery-event claims.
--
-- A retry claims one event before any GitHub write. The claim token fences
-- completion: a writer whose lease expired can never overwrite the receipt of
-- the writer that took over. The marker lookup in the delivery service remains
-- the recovery path for a write that landed before its local receipt.

ALTER TABLE issue_delivery_events ADD COLUMN claim_token TEXT;
ALTER TABLE issue_delivery_events ADD COLUMN claim_expires_at TEXT;

CREATE INDEX IF NOT EXISTS issue_delivery_events_claim_idx
  ON issue_delivery_events(claim_expires_at)
  WHERE claim_token IS NOT NULL;
