-- WP0 hardening: exact plan binding, query indexes and workflow identity.
-- SQLite cannot add a foreign key to an existing table without rebuilding it;
-- finding_dispositions.candidate_id remains application-validated because a
-- finding can be recorded before a replacement candidate exists.

ALTER TABLE plan_reviews ADD COLUMN plan_sha256 TEXT NOT NULL DEFAULT '';

CREATE INDEX planning_artifacts_request_idx
  ON planning_artifacts(request_id, kind, revision DESC);
CREATE INDEX stack_decisions_request_idx ON stack_decisions(request_id, revision DESC);
CREATE INDEX architecture_plans_request_idx ON architecture_plans(request_id, revision DESC);
CREATE INDEX documentation_plans_request_idx ON documentation_plans(request_id, revision DESC);
CREATE INDEX skill_plans_request_idx ON skill_plans(request_id, revision DESC);
CREATE INDEX definition_of_done_request_idx ON definition_of_done(request_id, revision DESC);
CREATE INDEX reviewer_plans_request_idx ON reviewer_plans(request_id, revision DESC);
CREATE INDEX dependency_plans_request_idx ON dependency_plans(request_id, revision DESC);
CREATE INDEX openspec_projections_request_idx ON openspec_projections(request_id, revision DESC);
CREATE INDEX finding_dispositions_candidate_idx
  ON finding_dispositions(candidate_id, status, updated_at);
CREATE INDEX workflow_steps_status_idx ON workflow_steps(run_id, status, updated_at);
CREATE UNIQUE INDEX workflow_steps_identity_idx
  ON workflow_steps(run_id, kind, attempt);
CREATE INDEX idempotency_keys_expiry_idx ON idempotency_keys(expires_at);
CREATE INDEX external_effects_state_idx ON external_effects(kind, state, updated_at);
