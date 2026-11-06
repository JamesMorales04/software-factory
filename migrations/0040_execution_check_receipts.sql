-- Immutable execution receipts for frozen checks.
--
-- A progress `TestsImproved` claim is only evidence of a real check run when it
-- cites the exact receipt Core recorded for that run. The receipt binds the
-- Campaign, work item, Attempt, generation, frozen command, suite label,
-- candidate identity, result and outcome. Historical provider tool events have
-- no receipt and can never manufacture verified test progress.

CREATE TABLE IF NOT EXISTS execution_check_receipts (
  receipt_id TEXT PRIMARY KEY,
  session_record_id TEXT NOT NULL,
  sequence INTEGER NOT NULL,
  campaign_id TEXT NOT NULL REFERENCES campaigns(id),
  attempt_id TEXT NOT NULL REFERENCES attempts(id),
  work_item_id TEXT NOT NULL,
  generation INTEGER NOT NULL CHECK (generation > 0),
  call_id TEXT NOT NULL,
  suite TEXT NOT NULL,
  command_json TEXT NOT NULL,
  command_sha256 TEXT NOT NULL CHECK (length(command_sha256) = 64),
  candidate_sha256 TEXT NOT NULL CHECK (length(candidate_sha256) = 64),
  exit_code INTEGER,
  expected_exit INTEGER NOT NULL,
  outcome TEXT NOT NULL CHECK (outcome IN ('PASS', 'FAIL')),
  result_sha256 TEXT NOT NULL CHECK (length(result_sha256) = 64),
  created_at TEXT NOT NULL,
  UNIQUE (session_record_id, sequence),
  FOREIGN KEY (session_record_id, sequence)
    REFERENCES provider_tool_events(session_record_id, sequence)
);

CREATE INDEX IF NOT EXISTS execution_check_receipts_check_idx
  ON execution_check_receipts(campaign_id, work_item_id, suite, created_at, sequence);

CREATE TRIGGER IF NOT EXISTS execution_check_receipts_immutable_update
BEFORE UPDATE ON execution_check_receipts
BEGIN SELECT RAISE(ABORT, 'execution check receipt is immutable'); END;

CREATE TRIGGER IF NOT EXISTS execution_check_receipts_immutable_delete
BEFORE DELETE ON execution_check_receipts
BEGIN SELECT RAISE(ABORT, 'execution check receipt is immutable'); END;

-- Progress evidence now records the exact execution attempt and receipt. A
-- non-empty receipt may be consumed once; replaying the same evidence row does
-- not insert a new row and therefore does not conflict with this index.
ALTER TABLE progress_evidence ADD COLUMN attempt_id TEXT NOT NULL DEFAULT '';
ALTER TABLE progress_evidence ADD COLUMN execution_receipt_id TEXT NOT NULL DEFAULT '';

CREATE UNIQUE INDEX IF NOT EXISTS progress_evidence_execution_receipt_idx
  ON progress_evidence(execution_receipt_id)
  WHERE execution_receipt_id <> '';
