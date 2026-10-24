-- Stable repository identity derived from the canonical path and root commit set.
ALTER TABLE projects ADD COLUMN identity TEXT NOT NULL DEFAULT '';
