-- Execution Policy V2: immutable policy evidence, versioned authorization rows
-- and deterministic progress supervision.
--
-- Product decision: artificial productivity caps (turns, attempts, sessions,
-- repair cycles, a universal Campaign wall clock, per-role cumulative tokens
-- and per-role monetary ceilings) are not execution authority. Technical
-- operation limits, scope, tools, risk, isolation, leases, revocation,
-- credential lifetime and the independent Quality Gate remain.
--
-- Legacy V1 rows keep every original column, value and constraint byte for
-- byte at the row level; they are historical evidence and are never resumed as
-- V2. A V1 Campaign that needs more work requires a new linked V2 request.
--
-- SQLite rebuilds follow the documented foreign-key-safe procedure. This file
-- runs inside the migration transaction; `PRAGMA defer_foreign_keys=ON` defers
-- integrity checks to commit so rebuilt parent tables can be recreated with the
-- same identity. A temporary assertion table turns a row/count mismatch into an
-- atomic migration failure.

PRAGMA defer_foreign_keys = ON;

-- ---------------------------------------------------------------------------
-- 1. Immutable V2 policy evidence.
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS execution_policies (
  policy_id TEXT PRIMARY KEY,
  schema_version INTEGER NOT NULL CHECK (schema_version = 2),
  authorization_id TEXT NOT NULL,
  monetary_envelope_id TEXT NOT NULL,
  scope_sha256 TEXT NOT NULL CHECK (length(scope_sha256) = 64),
  progress_policy_json TEXT NOT NULL,
  operation_limits_json TEXT NOT NULL,
  policy_json TEXT NOT NULL,
  policy_sha256 TEXT NOT NULL CHECK (length(policy_sha256) = 64),
  created_at TEXT NOT NULL,
  UNIQUE (authorization_id)
);

CREATE INDEX IF NOT EXISTS execution_policies_envelope_idx
  ON execution_policies(monetary_envelope_id);

CREATE TRIGGER IF NOT EXISTS execution_policies_immutable_update
BEFORE UPDATE ON execution_policies
BEGIN SELECT RAISE(ABORT, 'execution policy is immutable'); END;

CREATE TRIGGER IF NOT EXISTS execution_policies_immutable_delete
BEFORE DELETE ON execution_policies
BEGIN SELECT RAISE(ABORT, 'execution policy is immutable'); END;

-- ---------------------------------------------------------------------------
-- 2. Progress supervision: durable snapshot, evidence, checkpoints, decisions.
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS campaign_progress_state (
  campaign_id TEXT PRIMARY KEY REFERENCES campaigns(id),
  policy_sha256 TEXT NOT NULL CHECK (length(policy_sha256) = 64),
  state_json TEXT NOT NULL,
  state_sha256 TEXT NOT NULL CHECK (length(state_sha256) = 64),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS progress_evidence (
  evidence_id TEXT PRIMARY KEY,
  campaign_id TEXT NOT NULL REFERENCES campaigns(id),
  work_item_id TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN (
    'CANDIDATE_CHANGED', 'TESTS_IMPROVED', 'FINDINGS_RESOLVED',
    'WORK_GRAPH_ADVANCED', 'UNIT_ACCEPTED', 'REPAIR_VALIDATED',
    'BLOCKER_IDENTIFIED')),
  artifact_ref TEXT NOT NULL,
  artifact_sha256 TEXT NOT NULL CHECK (length(artifact_sha256) = 64),
  baseline_ref TEXT NOT NULL,
  observed_sequence INTEGER NOT NULL CHECK (observed_sequence > 0),
  evidence_json TEXT NOT NULL,
  evidence_sha256 TEXT NOT NULL CHECK (length(evidence_sha256) = 64),
  created_at TEXT NOT NULL,
  UNIQUE (campaign_id, work_item_id, kind, artifact_ref, artifact_sha256, baseline_ref)
);

CREATE INDEX IF NOT EXISTS progress_evidence_campaign_idx
  ON progress_evidence(campaign_id, work_item_id, observed_sequence);

CREATE TRIGGER IF NOT EXISTS progress_evidence_immutable_update
BEFORE UPDATE ON progress_evidence
BEGIN SELECT RAISE(ABORT, 'progress evidence is append-only'); END;

CREATE TRIGGER IF NOT EXISTS progress_evidence_immutable_delete
BEFORE DELETE ON progress_evidence
BEGIN SELECT RAISE(ABORT, 'progress evidence is append-only'); END;

CREATE TABLE IF NOT EXISTS progress_checkpoints (
  checkpoint_id TEXT PRIMARY KEY,
  campaign_id TEXT NOT NULL REFERENCES campaigns(id),
  work_item_id TEXT NOT NULL,
  operation_id TEXT NOT NULL,
  evidence_ids_json TEXT NOT NULL,
  outcome_sha256 TEXT NOT NULL CHECK (length(outcome_sha256) = 64),
  classification TEXT NOT NULL CHECK (classification IN
    ('VERIFIED', 'PROVISIONAL', 'STAGNANT', 'REPLAYED')),
  decision TEXT NOT NULL CHECK (decision IN
    ('CONTINUE', 'DIAGNOSE', 'REPLAN', 'ESCALATE', 'AWAIT_AUTHORIZATION',
     'AWAIT_RECONCILIATION', 'CANCEL', 'COMPLETE')),
  checkpoint_json TEXT NOT NULL,
  checkpoint_sha256 TEXT NOT NULL CHECK (length(checkpoint_sha256) = 64),
  created_at TEXT NOT NULL,
  UNIQUE (campaign_id, work_item_id, outcome_sha256),
  UNIQUE (campaign_id, operation_id)
);

CREATE INDEX IF NOT EXISTS progress_checkpoints_work_item_idx
  ON progress_checkpoints(campaign_id, work_item_id, created_at, checkpoint_id);

CREATE TRIGGER IF NOT EXISTS progress_checkpoints_immutable_update
BEFORE UPDATE ON progress_checkpoints
BEGIN SELECT RAISE(ABORT, 'progress checkpoint is append-only'); END;

CREATE TRIGGER IF NOT EXISTS progress_checkpoints_immutable_delete
BEFORE DELETE ON progress_checkpoints
BEGIN SELECT RAISE(ABORT, 'progress checkpoint is append-only'); END;

CREATE TABLE IF NOT EXISTS progress_decisions (
  decision_id TEXT PRIMARY KEY,
  campaign_id TEXT NOT NULL REFERENCES campaigns(id),
  work_item_id TEXT NOT NULL,
  checkpoint_id TEXT NOT NULL REFERENCES progress_checkpoints(checkpoint_id),
  decision TEXT NOT NULL CHECK (decision IN
    ('CONTINUE', 'DIAGNOSE', 'REPLAN', 'ESCALATE', 'AWAIT_AUTHORIZATION',
     'AWAIT_RECONCILIATION', 'CANCEL', 'COMPLETE')),
  reason TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS progress_decisions_campaign_idx
  ON progress_decisions(campaign_id, created_at, decision_id);

CREATE TRIGGER IF NOT EXISTS progress_decisions_immutable_update
BEFORE UPDATE ON progress_decisions
BEGIN SELECT RAISE(ABORT, 'progress decision is append-only'); END;

CREATE TRIGGER IF NOT EXISTS progress_decisions_immutable_delete
BEFORE DELETE ON progress_decisions
BEGIN SELECT RAISE(ABORT, 'progress decision is append-only'); END;

-- ---------------------------------------------------------------------------
-- 3. Version discriminators for existing execution evidence.
-- ---------------------------------------------------------------------------

ALTER TABLE standing_delegations
  ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1
  CHECK (schema_version IN (1, 2));
ALTER TABLE standing_delegations ADD COLUMN authority_json TEXT;
ALTER TABLE standing_delegations ADD COLUMN authority_sha256 TEXT;

CREATE TRIGGER IF NOT EXISTS standing_delegations_versioned_authority
BEFORE INSERT ON standing_delegations
WHEN NEW.schema_version NOT IN (1, 2)
  OR (NEW.schema_version = 1
      AND (NEW.authority_json IS NOT NULL OR NEW.authority_sha256 IS NOT NULL))
  OR (NEW.schema_version = 2
      AND (NEW.authority_json IS NULL
           OR NEW.authority_sha256 IS NULL
           OR length(NEW.authority_sha256) <> 64))
BEGIN
  SELECT RAISE(ABORT, 'standing delegation version and authority are inconsistent');
END;

CREATE TRIGGER IF NOT EXISTS standing_delegations_immutable_authority
BEFORE UPDATE ON standing_delegations
WHEN OLD.schema_version <> NEW.schema_version
  OR OLD.authority_json IS NOT NEW.authority_json
  OR OLD.authority_sha256 IS NOT NEW.authority_sha256
BEGIN
  SELECT RAISE(ABORT, 'standing delegation authority is immutable');
END;

ALTER TABLE execution_profile_revisions
  ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1
  CHECK (schema_version IN (1, 2));

CREATE TRIGGER IF NOT EXISTS execution_profile_revisions_versioned_content
BEFORE INSERT ON execution_profile_revisions
WHEN NEW.schema_version NOT IN (1, 2)
BEGIN
  SELECT RAISE(ABORT, 'execution profile revision schema version is unsupported');
END;

ALTER TABLE lead_execution_context
  ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1
  CHECK (schema_version IN (1, 2));

ALTER TABLE campaign_execution_profile_bindings
  ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1
  CHECK (schema_version IN (1, 2));

ALTER TABLE execution_policy_grants
  ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1
  CHECK (schema_version IN (1, 2));

-- ---------------------------------------------------------------------------
-- 4. Rebuild campaign_authorizations.
--
-- V1 rows keep the original NOT NULL budget columns and the original
-- max_attempts BETWEEN 1 AND 5 constraint. V2 rows must carry a policy
-- reference and NULL every obsolete cap column. Zero or huge-number sentinels
-- are forbidden by the versioned CHECK.
-- ---------------------------------------------------------------------------

CREATE TABLE campaign_authorizations_new (
  campaign_id TEXT PRIMARY KEY REFERENCES campaigns(id),
  authorization_id TEXT NOT NULL UNIQUE,
  risk_class TEXT NOT NULL CHECK (risk_class = 'R0'),
  execution_mode TEXT NOT NULL CHECK (execution_mode IN
    ('FIXTURE','TRUSTED_LOCAL_PROVIDER','ISOLATED')),
  actor TEXT NOT NULL,
  rationale TEXT NOT NULL,
  authorized_paths_json TEXT NOT NULL,
  max_attempts INTEGER CHECK (max_attempts BETWEEN 1 AND 5),
  created_at TEXT NOT NULL,
  provider TEXT NOT NULL DEFAULT '',
  model TEXT NOT NULL DEFAULT '',
  provider_mode TEXT NOT NULL DEFAULT '',
  delegation_id TEXT NOT NULL DEFAULT '',
  max_sessions INTEGER CHECK (max_sessions >= 0),
  max_turns INTEGER CHECK (max_turns >= 0),
  max_input_tokens INTEGER CHECK (max_input_tokens >= 0),
  max_output_tokens INTEGER CHECK (max_output_tokens >= 0),
  max_cost_micros INTEGER CHECK (max_cost_micros >= 0),
  deadline_secs INTEGER CHECK (deadline_secs >= 0),
  allowed_tools_json TEXT NOT NULL DEFAULT '[]',
  schema_version INTEGER NOT NULL DEFAULT 1,
  policy_id TEXT REFERENCES execution_policies(policy_id),
  CHECK (
    (schema_version = 1
      AND max_attempts IS NOT NULL
      AND max_sessions IS NOT NULL
      AND max_turns IS NOT NULL
      AND max_input_tokens IS NOT NULL
      AND max_output_tokens IS NOT NULL
      AND max_cost_micros IS NOT NULL
      AND deadline_secs IS NOT NULL
      AND policy_id IS NULL)
    OR
    (schema_version = 2
      AND policy_id IS NOT NULL
      AND max_attempts IS NULL
      AND max_sessions IS NULL
      AND max_turns IS NULL
      AND max_input_tokens IS NULL
      AND max_output_tokens IS NULL
      AND max_cost_micros IS NULL
      AND deadline_secs IS NULL)
  )
);

DROP TABLE IF EXISTS _campaign_authorization_assert;
CREATE TEMP TABLE _campaign_authorization_assert (
  exact_rows_preserved INTEGER NOT NULL CHECK (exact_rows_preserved = 1),
  foreign_keys_ok INTEGER NOT NULL CHECK (foreign_keys_ok = 1)
);

INSERT INTO campaign_authorizations_new
  (campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
   authorized_paths_json,max_attempts,created_at,provider,model,provider_mode,
   delegation_id,max_sessions,max_turns,max_input_tokens,max_output_tokens,
   max_cost_micros,deadline_secs,allowed_tools_json,schema_version,policy_id)
SELECT campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
       authorized_paths_json,max_attempts,created_at,provider,model,provider_mode,
       delegation_id,max_sessions,max_turns,max_input_tokens,max_output_tokens,
       max_cost_micros,deadline_secs,allowed_tools_json,1,NULL
FROM campaign_authorizations;

INSERT INTO _campaign_authorization_assert (exact_rows_preserved, foreign_keys_ok)
SELECT
  CASE WHEN NOT EXISTS (
    SELECT campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
           authorized_paths_json,max_attempts,created_at,provider,model,provider_mode,
           delegation_id,max_sessions,max_turns,max_input_tokens,max_output_tokens,
           max_cost_micros,deadline_secs,allowed_tools_json,1,NULL
    FROM campaign_authorizations
    EXCEPT
    SELECT campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
           authorized_paths_json,max_attempts,created_at,provider,model,provider_mode,
           delegation_id,max_sessions,max_turns,max_input_tokens,max_output_tokens,
           max_cost_micros,deadline_secs,allowed_tools_json,schema_version,policy_id
    FROM campaign_authorizations_new
  ) AND NOT EXISTS (
    SELECT campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
           authorized_paths_json,max_attempts,created_at,provider,model,provider_mode,
           delegation_id,max_sessions,max_turns,max_input_tokens,max_output_tokens,
           max_cost_micros,deadline_secs,allowed_tools_json,schema_version,policy_id
    FROM campaign_authorizations_new
    EXCEPT
    SELECT campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
           authorized_paths_json,max_attempts,created_at,provider,model,provider_mode,
           delegation_id,max_sessions,max_turns,max_input_tokens,max_output_tokens,
           max_cost_micros,deadline_secs,allowed_tools_json,1,NULL
    FROM campaign_authorizations
  ) THEN 1 ELSE 0 END,
  CASE WHEN (SELECT COUNT(*) FROM pragma_foreign_key_check) = 0 THEN 1 ELSE 0 END;

DROP TABLE campaign_authorizations;

ALTER TABLE campaign_authorizations_new RENAME TO campaign_authorizations;

DROP TABLE _campaign_authorization_assert;

CREATE TRIGGER campaign_authorizations_immutable_update
BEFORE UPDATE ON campaign_authorizations
BEGIN SELECT RAISE(ABORT, 'campaign authorization is immutable'); END;

CREATE TRIGGER campaign_authorizations_immutable_delete
BEFORE DELETE ON campaign_authorizations
BEGIN SELECT RAISE(ABORT, 'campaign authorization is immutable'); END;

-- A V2 authorization may only reference the V2 policy created for the same
-- authorization identity. This is the join that prevents swapping policies.
CREATE TRIGGER campaign_authorizations_v2_policy_identity
BEFORE INSERT ON campaign_authorizations
WHEN NEW.schema_version = 2
BEGIN
  SELECT RAISE(ABORT, 'V2 authorization policy identity mismatch')
  WHERE NOT EXISTS (
    SELECT 1 FROM execution_policies p
    WHERE p.policy_id = NEW.policy_id
      AND p.authorization_id = NEW.authorization_id
      AND p.schema_version = 2
  );
END;

-- A legacy authorization remains historical evidence and cannot acquire a V2
-- policy or a new budget after the fact.
CREATE TRIGGER campaign_authorizations_v1_stays_legacy
BEFORE UPDATE ON campaign_authorizations
WHEN OLD.schema_version = 1 AND NEW.schema_version = 2
BEGIN SELECT RAISE(ABORT, 'legacy authorization cannot become V2'); END;
