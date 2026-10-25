-- Preserve the exact model-output schema identity with each assessment.
ALTER TABLE lead_assessments ADD COLUMN schema_sha256 TEXT NOT NULL DEFAULT '';
