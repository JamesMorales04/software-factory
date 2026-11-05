-- Recreate the admission identity guard so the execution profile bound by
-- migration 0046 cannot be changed after admission creation.
DROP TRIGGER IF EXISTS subscription_scope_admission_identity_immutable;

CREATE TRIGGER subscription_scope_admission_identity_immutable
BEFORE UPDATE ON subscription_scope_admissions
WHEN OLD.admission_id <> NEW.admission_id
  OR OLD.intake_request_id <> NEW.intake_request_id
  OR OLD.lead_request_id <> NEW.lead_request_id
  OR OLD.test_profile_id <> NEW.test_profile_id
  OR OLD.execution_profile_id <> NEW.execution_profile_id
  OR OLD.provider <> NEW.provider
  OR OLD.model <> NEW.model
  OR OLD.settings_revision <> NEW.settings_revision
  OR OLD.profile_revision_id <> NEW.profile_revision_id
  OR OLD.profile_hash <> NEW.profile_hash
  OR OLD.assumption_json <> NEW.assumption_json
  OR OLD.assumption_sha256 <> NEW.assumption_sha256
  OR OLD.routes_json <> NEW.routes_json
  OR OLD.created_at_ms <> NEW.created_at_ms
  OR OLD.expires_at_ms <> NEW.expires_at_ms
  OR OLD.idempotency_key <> NEW.idempotency_key
BEGIN SELECT RAISE(ABORT, 'subscription scope admission identity is immutable'); END;
