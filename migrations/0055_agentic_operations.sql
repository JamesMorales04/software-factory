-- Durable owner-level operation records. They are a projection and scheduling
-- input; Core authority and Campaign lifecycle remain authoritative.
CREATE TABLE IF NOT EXISTS agentic_operations (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL UNIQUE REFERENCES intake_requests(id),
  project_id TEXT NOT NULL,
  director_id TEXT NOT NULL,
  operation_manager_id TEXT NOT NULL,
  priority INTEGER NOT NULL DEFAULT 100 CHECK (priority >= 0 AND priority <= 1000),
  status TEXT NOT NULL,
  worktree_policy TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS agentic_operations_queue_idx
  ON agentic_operations(status, priority, created_at);

-- Historical requests remain visible but are not automatically queued again.
INSERT INTO agentic_operations
  (id,request_id,project_id,director_id,operation_manager_id,priority,status,
   worktree_policy,created_at,updated_at)
SELECT 'operation-' || id, id, project_id, 'director:legacy',
       'operation-manager:' || id, 100, 'OBSERVED', 'PER_ATTEMPT',
       created_at, updated_at
FROM intake_requests;
