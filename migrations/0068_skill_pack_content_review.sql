-- WP3 review repair: separate source availability from content freshness.
-- Existing rows keep their source URL check date; they have no content review
-- until an explicitly reviewed pack revision is persisted.
ALTER TABLE skill_packs RENAME COLUMN last_verified_at TO source_checked_at;
ALTER TABLE skill_packs ADD COLUMN content_reviewed_at TEXT NOT NULL DEFAULT '';
