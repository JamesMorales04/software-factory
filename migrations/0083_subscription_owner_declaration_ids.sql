-- Persist the exact Owner Extra Usage declarations that backed an admission.
ALTER TABLE subscription_scope_admissions
  ADD COLUMN owner_extra_usage_declarations_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE subscription_scope_admissions
  ADD COLUMN request_fingerprint TEXT NOT NULL DEFAULT '';
