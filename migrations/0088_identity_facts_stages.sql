-- Explicit identity-facts stages. `SPEC` binds the frozen environment spec
-- (the existing behaviour). `RESOLUTION` binds an exact failed realisation
-- before any spec exists: the plan plus the canonical realisation key. The
-- default keeps every existing row and pending challenge byte-identical.
ALTER TABLE identity_facts_challenges
    ADD COLUMN stage TEXT NOT NULL DEFAULT 'SPEC'
    CHECK (stage IN ('SPEC', 'RESOLUTION'));

ALTER TABLE identity_facts_challenges
    ADD COLUMN resolution_key TEXT NOT NULL DEFAULT '';

ALTER TABLE environment_identity_facts_receipts
    ADD COLUMN stage TEXT NOT NULL DEFAULT 'SPEC'
    CHECK (stage IN ('SPEC', 'RESOLUTION'));

ALTER TABLE environment_identity_facts_receipts
    ADD COLUMN resolution_key TEXT NOT NULL DEFAULT '';
