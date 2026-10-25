-- Deterministic multi-unit integration attempts and candidate lineage.
ALTER TABLE candidates ADD COLUMN integration INTEGER NOT NULL DEFAULT 0;
ALTER TABLE candidates ADD COLUMN lineage_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE attempt_specs ADD COLUMN kind TEXT NOT NULL DEFAULT 'UNIT';
ALTER TABLE attempt_specs ADD COLUMN integration_candidates_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE attempt_specs ADD COLUMN next_task_id TEXT;
ALTER TABLE attempt_specs ADD COLUMN final_integration INTEGER NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS lead_integrations (
  integration_id TEXT PRIMARY KEY,
  campaign_id TEXT NOT NULL REFERENCES campaigns(id),
  purpose TEXT NOT NULL CHECK (purpose IN ('FANIN','FINAL')),
  next_task_id TEXT NOT NULL DEFAULT '',
  attempt_id TEXT NOT NULL UNIQUE REFERENCES attempts(id),
  status TEXT NOT NULL,
  candidates_json TEXT NOT NULL,
  candidate_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(campaign_id,purpose,next_task_id)
);
CREATE INDEX IF NOT EXISTS lead_integrations_campaign_idx ON lead_integrations(campaign_id);
