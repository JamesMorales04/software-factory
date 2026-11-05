-- Codex App Server review receipts are model-backed but do not use Core's
-- chat gateway or a model-call reservation. Extend the mechanism constraint
-- without changing existing deterministic/model rows.
CREATE TABLE quality_receipts_codex_review (
  receipt_id TEXT PRIMARY KEY,
  candidate_id TEXT NOT NULL REFERENCES candidates(id),
  role TEXT NOT NULL CHECK (role IN ('verifier', 'reviewer')),
  principal_id TEXT NOT NULL,
  action_spec_sha256 TEXT NOT NULL,
  commit_sha TEXT NOT NULL,
  tree_sha TEXT NOT NULL,
  result TEXT NOT NULL CHECK (result IN ('PASS', 'FAIL', 'CHANGES_REQUESTED', 'NOT_TESTED')),
  evidence TEXT NOT NULL CHECK (evidence IN ('EXECUTED', 'MOCKED', 'UNKNOWN')),
  received_at TEXT NOT NULL,
  findings_json TEXT NOT NULL DEFAULT '[]',
  mechanism TEXT NOT NULL DEFAULT 'DETERMINISTIC'
    CHECK (mechanism IN ('DETERMINISTIC','MODEL','CODEX_APP_SERVER')),
  model_provider TEXT NOT NULL DEFAULT '',
  model_name TEXT NOT NULL DEFAULT '',
  model_reservation_id TEXT NOT NULL DEFAULT '',
  input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
  output_tokens INTEGER NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
  total_tokens INTEGER NOT NULL DEFAULT 0 CHECK (total_tokens >= 0),
  cost_micros INTEGER NOT NULL DEFAULT 0 CHECK (cost_micros >= 0),
  usage_known INTEGER NOT NULL DEFAULT 0 CHECK (usage_known IN (0,1)),
  model_request_sha256 TEXT NOT NULL DEFAULT '',
  UNIQUE(candidate_id, role),
  CHECK (
    mechanism <> 'CODEX_APP_SERVER'
    OR (
      role = 'reviewer'
      AND model_provider = 'codex'
      AND model_name = 'gpt-6-luna'
      AND model_reservation_id = ''
      AND length(model_request_sha256) = 64
      AND usage_known = 1
    )
  )
);

INSERT INTO quality_receipts_codex_review
  (receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,
   result,evidence,received_at,findings_json,mechanism,model_provider,model_name,
   model_reservation_id,input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
   model_request_sha256)
SELECT receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,tree_sha,
       result,evidence,received_at,findings_json,mechanism,model_provider,model_name,
       model_reservation_id,input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
       model_request_sha256
FROM quality_receipts;

DROP TABLE quality_receipts;
ALTER TABLE quality_receipts_codex_review RENAME TO quality_receipts;

CREATE UNIQUE INDEX IF NOT EXISTS quality_receipt_model_reservation_idx
  ON quality_receipts(model_reservation_id)
  WHERE model_reservation_id <> '';
CREATE TRIGGER IF NOT EXISTS quality_receipts_immutable_update
BEFORE UPDATE ON quality_receipts BEGIN SELECT RAISE(ABORT, 'quality receipt is immutable'); END;
CREATE TRIGGER IF NOT EXISTS quality_receipts_immutable_delete
BEFORE DELETE ON quality_receipts BEGIN SELECT RAISE(ABORT, 'quality receipt is immutable'); END;
