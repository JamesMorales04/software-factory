-- Constraints added after 0084 was already applied. Keeping 0084 byte-identical
-- to the applied c75f checkpoint avoids a SQLx migration checksum mismatch on
-- retained databases; this migration adds the new grant identity and audit
-- immutability rules.
--
-- If a retained database already contains duplicate
-- (project_id, delegation_id, lead_request_id, content_sha256) decisions, the
-- unique index creation fails explicitly rather than silently deleting a grant.
CREATE UNIQUE INDEX IF NOT EXISTS license_decisions_grant_identity_idx
  ON license_decisions(project_id, delegation_id, lead_request_id, content_sha256);

DROP TRIGGER IF EXISTS license_decisions_immutable_content;
CREATE TRIGGER license_decisions_immutable_content
BEFORE UPDATE ON license_decisions
WHEN OLD.id != NEW.id
  OR OLD.delegation_id != NEW.delegation_id
  OR OLD.project_id != NEW.project_id
  OR OLD.lead_request_id != NEW.lead_request_id
  OR OLD.lead_reservation_id != NEW.lead_reservation_id
  OR OLD.lead_actor_sha256 != NEW.lead_actor_sha256
  OR OLD.lead_request_sha256 != NEW.lead_request_sha256
  OR OLD.lead_output_sha256 != NEW.lead_output_sha256
  OR OLD.lead_principal != NEW.lead_principal
  OR OLD.delegation_matter != NEW.delegation_matter
  OR OLD.delegation_option != NEW.delegation_option
  OR OLD.delegation_option_sha256 != NEW.delegation_option_sha256
  OR OLD.delegation_revision != NEW.delegation_revision
  OR OLD.delegation_pin != NEW.delegation_pin
  OR OLD.delegation_use_context != NEW.delegation_use_context
  OR OLD.delegation_criterion != NEW.delegation_criterion
  OR OLD.delegation_deadline != NEW.delegation_deadline
  OR OLD.identity_json != NEW.identity_json
  OR OLD.license_expression != NEW.license_expression
  OR OLD.selected_branch != NEW.selected_branch
  OR OLD.software_obligations_json != NEW.software_obligations_json
  OR OLD.documentation_json != NEW.documentation_json
  OR OLD.documentation_obligations_json != NEW.documentation_obligations_json
  OR OLD.software_verdict != NEW.software_verdict
  OR OLD.evidence_sha256 != NEW.evidence_sha256
  OR OLD.identity_facts_receipt_digest != NEW.identity_facts_receipt_digest
  OR OLD.content_sha256 != NEW.content_sha256
  OR OLD.provenance_json != NEW.provenance_json
  OR OLD.use_context != NEW.use_context
  OR OLD.commercial_use != NEW.commercial_use
  OR OLD.payment_required != NEW.payment_required
  OR OLD.catalog_sha256 != NEW.catalog_sha256
  OR OLD.profile_sha256 != NEW.profile_sha256
  OR OLD.policy_sha256 != NEW.policy_sha256
  OR OLD.stage_json != NEW.stage_json
  OR OLD.decision != NEW.decision
  OR OLD.revision != NEW.revision
  OR OLD.idempotency_key != NEW.idempotency_key
  OR OLD.created_at != NEW.created_at
  OR (OLD.revoked_at IS NOT NULL AND OLD.revoked_at IS NOT NEW.revoked_at)
  OR (OLD.revoked_at IS NOT NULL AND OLD.revocation_reason IS NOT NEW.revocation_reason)
  OR (OLD.revoked_at IS NOT NULL AND OLD.revoked_by IS NOT NEW.revoked_by)
BEGIN
  SELECT RAISE(ABORT, 'licence decision content is immutable');
END;

CREATE TRIGGER IF NOT EXISTS license_decision_revocations_immutable_update
BEFORE UPDATE ON license_decision_revocations
BEGIN
  SELECT RAISE(ABORT, 'licence decision revocation is immutable');
END;

CREATE TRIGGER IF NOT EXISTS license_decision_revocations_immutable_delete
BEFORE DELETE ON license_decision_revocations
BEGIN
  SELECT RAISE(ABORT, 'licence decision revocation is immutable');
END;
