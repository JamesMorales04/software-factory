-- Environment compiler records (ADR-0041, docs/contracts/environment-plan.md).
--
-- Environment plans are append-only revisions. A frozen EnvironmentSpec is
-- immutable; a later environment change is a new plan/spec row, never an
-- in-place rewrite. Existing requests receive no fake plan: the
-- environment_request_bindings view reports ENVIRONMENT_PLAN_REQUIRED until a
-- real Core-compiled plan and frozen spec exist.

CREATE TABLE environment_plans (
  id TEXT PRIMARY KEY,
  plan_id TEXT NOT NULL,
  project_id TEXT NOT NULL REFERENCES projects(id),
  request_id TEXT REFERENCES intake_requests(id),
  action_id TEXT REFERENCES actions(id),
  campaign_id TEXT REFERENCES campaigns(id),
  revision INTEGER NOT NULL CHECK (revision > 0),
  base_commit TEXT NOT NULL CHECK (length(base_commit) IN (40, 64)),
  risk_class TEXT NOT NULL CHECK (risk_class IN ('R0','R1','R2','R3')),
  conventions_sha256 TEXT NOT NULL CHECK (length(conventions_sha256) = 64),
  plan_sha256 TEXT NOT NULL CHECK (length(plan_sha256) = 64),
  status TEXT NOT NULL CHECK (status IN (
    'DRAFT','CORE_VALIDATED','LEAD_APPROVED','FROZEN','REJECTED','BLOCKED'
  )),
  source TEXT NOT NULL CHECK (source IN ('PROJECT_COMMITTED','GENERATED')),
  builder_image TEXT NOT NULL DEFAULT '',
  builder_image_digest TEXT NOT NULL DEFAULT ''
    CHECK (builder_image_digest = '' OR (
      length(builder_image_digest) = 71
      AND substr(builder_image_digest, 1, 7) = 'sha256:'
    )),
  license_inventory_sha256 TEXT NOT NULL DEFAULT ''
    CHECK (license_inventory_sha256 = '' OR length(license_inventory_sha256) = 64),
  sbom_sha256 TEXT NOT NULL DEFAULT ''
    CHECK (sbom_sha256 = '' OR length(sbom_sha256) = 64),
  lead_review_id TEXT,
  content_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (plan_sha256),
  UNIQUE (request_id, revision)
);
CREATE INDEX environment_plans_request_idx
  ON environment_plans(request_id, revision DESC);
CREATE INDEX environment_plans_action_idx
  ON environment_plans(action_id);
CREATE INDEX environment_plans_campaign_idx
  ON environment_plans(campaign_id);
CREATE INDEX environment_plans_status_idx
  ON environment_plans(status, updated_at);

CREATE TABLE environment_plan_reviews (
  id TEXT PRIMARY KEY,
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  action_id TEXT NOT NULL REFERENCES actions(id),
  plan_id TEXT NOT NULL,
  plan_sha256 TEXT NOT NULL CHECK (length(plan_sha256) = 64),
  plan_revision INTEGER NOT NULL CHECK (plan_revision > 0),
  risk_class TEXT NOT NULL CHECK (risk_class IN ('R0','R1','R2','R3')),
  reviewer_role TEXT NOT NULL CHECK (reviewer_role = 'LEAD'),
  principal_id TEXT NOT NULL CHECK (principal_id LIKE 'lead:%'),
  recorded_by TEXT NOT NULL,
  decision TEXT NOT NULL CHECK (decision IN ('APPROVED','CHANGES_REQUESTED','REJECTED')),
  rationale TEXT NOT NULL,
  constraints_json TEXT NOT NULL DEFAULT '[]',
  evidence_json TEXT NOT NULL DEFAULT '[]',
  model_call_json TEXT NOT NULL,
  decided_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (plan_sha256, principal_id, decision, decided_at)
);
CREATE INDEX environment_plan_reviews_plan_idx
  ON environment_plan_reviews(plan_sha256, decided_at DESC);
CREATE INDEX environment_plan_reviews_request_idx
  ON environment_plan_reviews(request_id, decided_at DESC);

CREATE TABLE environment_specs (
  id TEXT PRIMARY KEY,
  spec_id TEXT NOT NULL,
  spec_sha256 TEXT NOT NULL CHECK (length(spec_sha256) = 64),
  project_id TEXT NOT NULL REFERENCES projects(id),
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  action_id TEXT NOT NULL REFERENCES actions(id),
  campaign_id TEXT REFERENCES campaigns(id),
  base_commit TEXT NOT NULL CHECK (length(base_commit) IN (40, 64)),
  plan_id TEXT NOT NULL,
  plan_sha256 TEXT NOT NULL REFERENCES environment_plans(plan_sha256),
  conventions_sha256 TEXT NOT NULL CHECK (length(conventions_sha256) = 64),
  review_id TEXT NOT NULL REFERENCES environment_plan_reviews(id),
  platform TEXT NOT NULL,
  nix_version TEXT NOT NULL,
  devenv_version TEXT NOT NULL,
  flake_lock_sha256 TEXT NOT NULL DEFAULT ''
    CHECK (flake_lock_sha256 = '' OR length(flake_lock_sha256) = 64),
  devenv_lock_sha256 TEXT NOT NULL DEFAULT ''
    CHECK (devenv_lock_sha256 = '' OR length(devenv_lock_sha256) = 64),
  license_inventory_sha256 TEXT NOT NULL CHECK (length(license_inventory_sha256) = 64),
  sbom_sha256 TEXT NOT NULL CHECK (length(sbom_sha256) = 64),
  builder_image TEXT NOT NULL DEFAULT '',
  builder_image_digest TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL CHECK (status IN ('FROZEN','SUPERSEDED')),
  content_json TEXT NOT NULL,
  frozen_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (spec_sha256),
  UNIQUE (plan_sha256)
);
CREATE INDEX environment_specs_request_idx
  ON environment_specs(request_id, frozen_at DESC);
CREATE INDEX environment_specs_action_idx
  ON environment_specs(action_id);
CREATE INDEX environment_specs_status_idx
  ON environment_specs(status, frozen_at);

-- A spec row is immutable; only a later revision can supersede it.
CREATE TRIGGER environment_specs_immutable_update
BEFORE UPDATE ON environment_specs
WHEN NEW.spec_id != OLD.spec_id
  OR NEW.spec_sha256 != OLD.spec_sha256
  OR NEW.plan_sha256 != OLD.plan_sha256
  OR NEW.content_json != OLD.content_json
  OR NEW.review_id != OLD.review_id
  OR NEW.project_id != OLD.project_id
  OR NEW.request_id != OLD.request_id
  OR NEW.action_id != OLD.action_id
BEGIN
  SELECT RAISE(ABORT, 'EnvironmentSpec is immutable');
END;
CREATE TRIGGER environment_specs_immutable_delete
BEFORE DELETE ON environment_specs
BEGIN
  SELECT RAISE(ABORT, 'EnvironmentSpec is immutable');
END;

CREATE TABLE environment_license_inventories (
  id TEXT PRIMARY KEY,
  plan_sha256 TEXT NOT NULL CHECK (length(plan_sha256) = 64),
  spec_sha256 TEXT NOT NULL DEFAULT ''
    CHECK (spec_sha256 = '' OR length(spec_sha256) = 64),
  inventory_sha256 TEXT NOT NULL CHECK (length(inventory_sha256) = 64),
  sbom_sha256 TEXT NOT NULL CHECK (length(sbom_sha256) = 64),
  coverage TEXT NOT NULL CHECK (coverage IN (
    'SUBSTRATE_AND_DECLARED_PACKAGES'
  )),
  content_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (inventory_sha256)
);
CREATE INDEX environment_license_inventories_plan_idx
  ON environment_license_inventories(plan_sha256);
CREATE TRIGGER environment_license_inventories_immutable_update
BEFORE UPDATE ON environment_license_inventories
BEGIN
  SELECT RAISE(ABORT, 'License inventory is immutable');
END;
CREATE TRIGGER environment_license_inventories_immutable_delete
BEFORE DELETE ON environment_license_inventories
BEGIN
  SELECT RAISE(ABORT, 'License inventory is immutable');
END;

-- Reserved for a future protected Owner-only override action. No HTTP or
-- database write path is enabled in this delivery; unfree packages fail closed.
CREATE TABLE environment_license_overrides (
  id TEXT PRIMARY KEY,
  package_name TEXT NOT NULL,
  version TEXT NOT NULL,
  license TEXT NOT NULL,
  plan_sha256 TEXT NOT NULL CHECK (length(plan_sha256) = 64),
  reason TEXT NOT NULL,
  decision_id TEXT NOT NULL,
  principal_id TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (package_name, version, license, plan_sha256)
);
CREATE INDEX environment_license_overrides_plan_idx
  ON environment_license_overrides(plan_sha256);
CREATE TRIGGER environment_license_overrides_immutable_update
BEFORE UPDATE ON environment_license_overrides
BEGIN
  SELECT RAISE(ABORT, 'License override is immutable');
END;
CREATE TRIGGER environment_license_overrides_immutable_delete
BEFORE DELETE ON environment_license_overrides
BEGIN
  SELECT RAISE(ABORT, 'License override is immutable');
END;

CREATE TABLE environment_build_receipts (
  id TEXT PRIMARY KEY,
  receipt_id TEXT NOT NULL,
  spec_sha256 TEXT NOT NULL CHECK (length(spec_sha256) = 64),
  platform TEXT NOT NULL,
  builder_image TEXT NOT NULL DEFAULT '',
  builder_image_digest TEXT NOT NULL DEFAULT '',
  builder_policy_revision TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN (
    'UNAVAILABLE','READY','BUILDING','SUCCEEDED','FAILED'
  )),
  store_path TEXT NOT NULL DEFAULT '',
  output_sha256 TEXT NOT NULL DEFAULT ''
    CHECK (output_sha256 = '' OR length(output_sha256) = 64),
  log_ref TEXT NOT NULL DEFAULT '',
  attestation_json TEXT NOT NULL DEFAULT '{}',
  blockers_json TEXT NOT NULL DEFAULT '[]',
  duration_ms INTEGER NOT NULL DEFAULT 0 CHECK (duration_ms >= 0),
  content_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (spec_sha256, platform, builder_policy_revision, builder_image_digest)
);
CREATE INDEX environment_build_receipts_spec_idx
  ON environment_build_receipts(spec_sha256, created_at DESC);
CREATE TRIGGER environment_build_receipts_immutable_update
BEFORE UPDATE ON environment_build_receipts
BEGIN
  SELECT RAISE(ABORT, 'Environment build receipt is immutable');
END;
CREATE TRIGGER environment_build_receipts_immutable_delete
BEFORE DELETE ON environment_build_receipts
BEGIN
  SELECT RAISE(ABORT, 'Environment build receipt is immutable');
END;

-- The exact frozen environment a Campaign was authorized with. A Campaign
-- cannot have two different environment bindings.
CREATE TABLE campaign_environments (
  campaign_id TEXT PRIMARY KEY REFERENCES campaigns(id),
  request_id TEXT NOT NULL REFERENCES intake_requests(id),
  action_id TEXT NOT NULL REFERENCES actions(id),
  spec_id TEXT NOT NULL,
  spec_sha256 TEXT NOT NULL CHECK (length(spec_sha256) = 64),
  plan_sha256 TEXT NOT NULL CHECK (length(plan_sha256) = 64),
  builder_image TEXT NOT NULL DEFAULT '',
  builder_image_digest TEXT NOT NULL DEFAULT '',
  bound_at TEXT NOT NULL
);
CREATE INDEX campaign_environments_spec_idx
  ON campaign_environments(spec_sha256);
CREATE TRIGGER campaign_environments_immutable_update
BEFORE UPDATE ON campaign_environments
BEGIN
  SELECT RAISE(ABORT, 'Campaign environment binding is immutable');
END;
CREATE TRIGGER campaign_environments_immutable_delete
BEFORE DELETE ON campaign_environments
BEGIN
  SELECT RAISE(ABORT, 'Campaign environment binding is immutable');
END;

-- Truthful default for old requests: no fabricated plan/spec row. A request
-- without a Core-compiled environment remains visible as ENVIRONMENT_PLAN_REQUIRED.
CREATE VIEW environment_request_bindings AS
SELECT
  r.id AS request_id,
  r.project_id AS project_id,
  r.action_id AS action_id,
  p.plan_id AS plan_id,
  p.plan_sha256 AS plan_sha256,
  p.revision AS plan_revision,
  p.status AS plan_status,
  s.spec_id AS spec_id,
  s.spec_sha256 AS spec_sha256,
  s.status AS spec_status,
  s.builder_image AS builder_image,
  s.builder_image_digest AS builder_image_digest,
  CASE
    WHEN p.plan_id IS NULL THEN 'ENVIRONMENT_PLAN_REQUIRED'
    WHEN p.status = 'BLOCKED' THEN 'ENVIRONMENT_PLAN_INVALID'
    WHEN s.spec_id IS NULL OR s.status != 'FROZEN' THEN 'ENVIRONMENT_PLAN_REQUIRED'
    ELSE NULL
  END AS blocker_code
FROM intake_requests r
LEFT JOIN environment_plans p
  ON p.request_id = r.id
 AND p.revision = (
   SELECT MAX(latest.revision) FROM environment_plans latest
   WHERE latest.request_id = r.id
 )
LEFT JOIN environment_specs s
  ON s.plan_sha256 = p.plan_sha256
 AND s.status = 'FROZEN';
