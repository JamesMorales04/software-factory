-- Read-only knowledge packs and the temporary/promoted skill registry.
-- Context7 output is evidence. Core owns the gap decision, trust gate,
-- promotion and revocation; a skill never grants write or execution tools.
CREATE TABLE IF NOT EXISTS agentic_knowledge_packs (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  project_id TEXT NOT NULL REFERENCES projects(id),
  library TEXT NOT NULL,
  library_id TEXT NOT NULL,
  query TEXT NOT NULL,
  pack_json TEXT NOT NULL,
  pack_sha256 TEXT NOT NULL,
  provider TEXT NOT NULL,
  source_url TEXT NOT NULL,
  fetched_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (request_id, library_id)
);

CREATE INDEX IF NOT EXISTS agentic_knowledge_packs_request_idx
  ON agentic_knowledge_packs(request_id, created_at DESC);

CREATE TABLE IF NOT EXISTS agentic_skills (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id),
  request_id TEXT REFERENCES intake_requests(id),
  name TEXT NOT NULL,
  scope TEXT NOT NULL CHECK (scope IN ('RUN', 'PROJECT', 'GLOBAL')),
  status TEXT NOT NULL CHECK (status IN ('CANDIDATE', 'ACTIVE', 'PROMOTION_READY', 'REVOKED', 'EXPIRED')),
  manifest_json TEXT NOT NULL,
  manifest_sha256 TEXT NOT NULL,
  provenance TEXT NOT NULL,
  license TEXT NOT NULL,
  license_reviewed_at TEXT,
  compatibility_json TEXT NOT NULL,
  success_count INTEGER NOT NULL DEFAULT 0 CHECK (success_count >= 0),
  failure_count INTEGER NOT NULL DEFAULT 0 CHECK (failure_count >= 0),
  last_used_at TEXT,
  promoted_at TEXT,
  last_reviewed_at TEXT,
  next_review_at TEXT,
  revoked_at TEXT,
  owner_approved_by TEXT,
  expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS agentic_skills_project_idx
  ON agentic_skills(project_id, scope, status);

CREATE UNIQUE INDEX IF NOT EXISTS agentic_skills_request_name_idx
  ON agentic_skills(request_id, name) WHERE request_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS agentic_skill_uses (
  id TEXT PRIMARY KEY,
  skill_id TEXT NOT NULL REFERENCES agentic_skills(id),
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  outcome TEXT NOT NULL CHECK (outcome IN ('SUCCESS', 'FAILURE')),
  evidence TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS agentic_skill_uses_skill_idx
  ON agentic_skill_uses(skill_id, created_at DESC);

CREATE TABLE IF NOT EXISTS agentic_skill_adoptions (
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  skill_id TEXT NOT NULL REFERENCES agentic_skills(id),
  role TEXT NOT NULL DEFAULT 'implementer',
  adopted_at TEXT NOT NULL,
  PRIMARY KEY (request_id, skill_id, role)
);
