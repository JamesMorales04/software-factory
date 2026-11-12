-- WP1 hardening: one durable workflow run per request.
CREATE UNIQUE INDEX workflow_runs_request_unique
  ON workflow_runs(request_id);
