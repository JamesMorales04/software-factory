-- Operator-controlled runtime policy that must be visible in Office.
-- Semantic Reviewer model calls are disabled by default and require an
-- explicit operator enable plus a configured default provider.

CREATE TABLE IF NOT EXISTS factory_settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  semantic_review_enabled INTEGER NOT NULL CHECK (semantic_review_enabled IN (0,1)),
  semantic_review_max_output_tokens INTEGER NOT NULL CHECK (semantic_review_max_output_tokens > 0),
  updated_at TEXT NOT NULL
);
INSERT OR IGNORE INTO factory_settings
  (id, semantic_review_enabled, semantic_review_max_output_tokens, updated_at)
VALUES (1, 0, 8192, strftime('%Y-%m-%dT%H:%M:%fZ','now'));
