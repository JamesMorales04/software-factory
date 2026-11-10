-- Pre-authorization dependency-plan attachment.
--
-- ActionSpecs remain immutable once a Campaign exists. The planning lane may
-- attach the frozen dependency plan after the executable spec is derived from
-- the assessment draft but before the Campaign is created/authorized. This is
-- the only permitted pre-authorization update; the dependency plan, required
-- flag and spec hash are then frozen with the ActionSpec and bound by the
-- Attempt grant and Campaign execution binding.

DROP TRIGGER IF EXISTS action_specs_immutable_update;

CREATE TRIGGER action_specs_immutable_update
BEFORE UPDATE ON action_specs
WHEN EXISTS (SELECT 1 FROM campaigns WHERE action_id = NEW.action_id)
BEGIN
  SELECT RAISE(ABORT, 'ActionSpec is immutable after Campaign start');
END;
