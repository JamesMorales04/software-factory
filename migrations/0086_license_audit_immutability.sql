-- The delegation revocation record is append-only. Deleting it would silently
-- re-enable a revoked Owner delegation because create_license_decision fences
-- new decisions from this table.
CREATE TRIGGER IF NOT EXISTS license_delegation_revocations_immutable_update
BEFORE UPDATE ON license_delegation_revocations
BEGIN
  SELECT RAISE(ABORT, 'licence delegation revocation is immutable');
END;

CREATE TRIGGER IF NOT EXISTS license_delegation_revocations_immutable_delete
BEFORE DELETE ON license_delegation_revocations
BEGIN
  SELECT RAISE(ABORT, 'licence delegation revocation is immutable');
END;
