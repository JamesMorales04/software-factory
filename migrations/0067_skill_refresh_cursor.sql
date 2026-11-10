-- WP3: durable rotating cursor for the bounded skill-pack refresh project scan.
-- The scheduler must report skipped projects and resume from the next window
-- instead of repeatedly implying full coverage from the first 64 projects.
CREATE TABLE skill_refresh_state (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  project_offset INTEGER NOT NULL DEFAULT 0 CHECK (project_offset >= 0),
  updated_at TEXT NOT NULL
);
