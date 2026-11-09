-- Repair requests are a separate, idempotent link from the immutable
-- autonomy repair decision to the new candidate-based intake request.
CREATE TABLE IF NOT EXISTS repair_requests (
  candidate_id TEXT PRIMARY KEY REFERENCES candidates(id),
  request_id TEXT NOT NULL UNIQUE REFERENCES intake_requests(id),
  created_at TEXT NOT NULL
);
