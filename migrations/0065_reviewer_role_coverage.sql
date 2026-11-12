-- WP9: reviewer independence is identity/process based. A reviewer receipt
-- records the role its authenticated identity was assigned so the Gate can
-- require one PASS per required role. Empty means a legacy receipt with no
-- role coverage claim.
ALTER TABLE quality_receipts ADD COLUMN reviewer_role TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS quality_receipt_candidate_reviewer_role_idx
  ON quality_receipts(candidate_id, reviewer_role);

-- Preserve the required role set next to the reviewer count so a Campaign
-- keeps its frozen reviewer plan even if the participant plan is projected.
ALTER TABLE campaign_review_requirements
  ADD COLUMN required_roles_json TEXT NOT NULL DEFAULT '[]';
