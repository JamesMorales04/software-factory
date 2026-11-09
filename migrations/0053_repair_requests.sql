-- A repair is a new request from the exact rejected candidate, not a new
-- request from the Project base commit. The link is evidence for the UI and
-- prevents an operator from losing the candidate lineage.
ALTER TABLE autonomy_repairs ADD COLUMN repair_request_id TEXT;
