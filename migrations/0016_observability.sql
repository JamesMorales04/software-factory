-- Durable OTLP export cursor. The ledger remains authoritative; this row only
-- records the last event sequence successfully exported to the collector.
CREATE TABLE IF NOT EXISTS observability_cursor (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  last_sequence INTEGER NOT NULL DEFAULT 0 CHECK (last_sequence >= 0),
  updated_at TEXT NOT NULL
);
INSERT OR IGNORE INTO observability_cursor(id,last_sequence,updated_at)
  VALUES (1,0,'1970-01-01T00:00:00Z');
