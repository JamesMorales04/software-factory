-- WP8 hardening: persist the authenticated owner principal on a refutation so
-- the decision attribution remains auditable after the request completes.
ALTER TABLE finding_dispositions
  ADD COLUMN owner_principal TEXT;
