-- Link a Lead request to the Campaign created by an authorized start.
ALTER TABLE lead_requests ADD COLUMN campaign_id TEXT;
