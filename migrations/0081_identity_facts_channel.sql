-- Authenticated environment identity-facts channel.
--
-- The Core-issued challenge is durable PENDING before the broker is dispatched.
-- Consumption is one transaction: the challenge moves PENDING -> CONSUMED and
-- the immutable receipt is inserted. A replay, concurrent consume or foreign
-- nonce matches no PENDING row, so it changes and deletes nothing.
--
-- These are deliberately separate from environment_build_receipts, which carry
-- READY/SUCCEEDED build semantics and a different unique key.

CREATE TABLE identity_facts_challenges (
    nonce TEXT PRIMARY KEY,
    operation_id TEXT NOT NULL,
    runtime_id TEXT NOT NULL,
    plan_sha256 TEXT NOT NULL,
    pin TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('ENUMERATE', 'QUESTIONS')),
    root TEXT NOT NULL CHECK (root IN ('OPERATION', 'PLAN', 'RUNTIME')),
    questions_json TEXT NOT NULL,
    challenge_json TEXT NOT NULL,
    request_id TEXT,
    plan_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('PENDING', 'CONSUMED', 'EXPIRED', 'REVOKED')),
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    consumed_at TEXT
);

CREATE INDEX identity_facts_challenges_plan
    ON identity_facts_challenges (plan_sha256, created_at);

CREATE TABLE environment_identity_facts_receipts (
    nonce TEXT PRIMARY KEY
        REFERENCES identity_facts_challenges (nonce),
    operation_id TEXT NOT NULL,
    runtime_id TEXT NOT NULL,
    plan_sha256 TEXT NOT NULL,
    pin TEXT NOT NULL,
    observed_root TEXT NOT NULL,
    fact_count INTEGER NOT NULL CHECK (fact_count > 0),
    closure_manifest_sha256 TEXT NOT NULL,
    manifest_json TEXT NOT NULL,
    envelope_json TEXT NOT NULL,
    signature TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE UNIQUE INDEX environment_identity_facts_receipts_operation
    ON environment_identity_facts_receipts (operation_id);

CREATE INDEX environment_identity_facts_receipts_plan
    ON environment_identity_facts_receipts (plan_sha256, created_at);
