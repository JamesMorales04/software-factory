-- WP8 hardening: bind a finding disposition to the reviewer that raised it.
-- The reviewer principal is recorded when the receipt is ingested and cannot be
-- replaced by a later caller.
ALTER TABLE finding_dispositions
  ADD COLUMN reviewer_principal TEXT NOT NULL DEFAULT '';

CREATE INDEX finding_dispositions_request_idx
  ON finding_dispositions(request_id, status, updated_at);
