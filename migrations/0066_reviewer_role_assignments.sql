-- WP9 P1: the frozen Campaign reviewer requirement must bind every required
-- role to an exact authenticated reviewer principal. An empty assignment set
-- keeps legacy campaigns readable but makes a multi-role Gate fail closed.
ALTER TABLE campaign_review_requirements
  ADD COLUMN required_assignments_json TEXT NOT NULL DEFAULT '[]';
