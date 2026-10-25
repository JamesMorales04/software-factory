-- Bind a durable model-review receipt to the exact Core-authorized model
-- request identity. An empty value is allowed only for deterministic receipts.
ALTER TABLE quality_receipts ADD COLUMN model_request_sha256 TEXT NOT NULL DEFAULT '';
CREATE UNIQUE INDEX IF NOT EXISTS quality_receipt_model_request_idx
  ON quality_receipts(model_request_sha256)
  WHERE model_request_sha256 <> '';
