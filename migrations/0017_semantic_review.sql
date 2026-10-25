-- Independent semantic review provenance and findings.
--
-- Deterministic receipts keep explicit defaults. A MODEL PASS receipt must
-- match a settled SEMANTIC_REVIEWER reservation in the Core-owned model-call
-- ledger for the same candidate scope, provider/model and measured usage.

ALTER TABLE quality_receipts ADD COLUMN findings_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE quality_receipts ADD COLUMN mechanism TEXT NOT NULL DEFAULT 'DETERMINISTIC'
  CHECK (mechanism IN ('DETERMINISTIC','MODEL'));
ALTER TABLE quality_receipts ADD COLUMN model_provider TEXT NOT NULL DEFAULT '';
ALTER TABLE quality_receipts ADD COLUMN model_name TEXT NOT NULL DEFAULT '';
ALTER TABLE quality_receipts ADD COLUMN model_reservation_id TEXT NOT NULL DEFAULT '';
ALTER TABLE quality_receipts ADD COLUMN input_tokens INTEGER NOT NULL DEFAULT 0
  CHECK (input_tokens >= 0);
ALTER TABLE quality_receipts ADD COLUMN output_tokens INTEGER NOT NULL DEFAULT 0
  CHECK (output_tokens >= 0);
ALTER TABLE quality_receipts ADD COLUMN total_tokens INTEGER NOT NULL DEFAULT 0
  CHECK (total_tokens >= 0);
ALTER TABLE quality_receipts ADD COLUMN cost_micros INTEGER NOT NULL DEFAULT 0
  CHECK (cost_micros >= 0);
ALTER TABLE quality_receipts ADD COLUMN usage_known INTEGER NOT NULL DEFAULT 0
  CHECK (usage_known IN (0,1));

CREATE UNIQUE INDEX IF NOT EXISTS quality_receipt_model_reservation_idx
  ON quality_receipts(model_reservation_id)
  WHERE model_reservation_id <> '';
