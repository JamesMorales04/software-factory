-- Owner-granted autonomous R0 intake. The grant remains a non-model authority:
-- the owner explicitly enables automatic R0 execution and confirms the
-- subscription billing assumption once; the Lead model may then assess and the
-- deterministic Core gate may start within the frozen policy.
ALTER TABLE execution_policy_grants
  ADD COLUMN autonomous INTEGER NOT NULL DEFAULT 0 CHECK (autonomous IN (0, 1));
ALTER TABLE execution_policy_grants
  ADD COLUMN subscription_confirmed INTEGER NOT NULL DEFAULT 0
  CHECK (subscription_confirmed IN (0, 1));
