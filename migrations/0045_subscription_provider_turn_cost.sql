-- Subscription provider turns are not priced against a monetary envelope. The
-- turn still needs token bounds, but its cost bound is legitimately zero.
CREATE TABLE provider_turn_reservations_subscription (
  reservation_id TEXT PRIMARY KEY,
  session_record_id TEXT NOT NULL REFERENCES provider_sessions(session_record_id),
  attempt_id TEXT NOT NULL REFERENCES attempts(id),
  generation INTEGER NOT NULL CHECK (generation > 0),
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  max_input_tokens INTEGER NOT NULL CHECK (max_input_tokens > 0),
  max_output_tokens INTEGER NOT NULL CHECK (max_output_tokens > 0),
  max_total_tokens INTEGER NOT NULL CHECK (max_total_tokens > 0),
  max_cost_micros INTEGER NOT NULL CHECK (max_cost_micros >= 0),
  request_sha256 TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('RESERVED','SETTLED','RELEASED')),
  input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
  output_tokens INTEGER NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
  total_tokens INTEGER NOT NULL DEFAULT 0 CHECK (total_tokens >= 0),
  cost_micros INTEGER NOT NULL DEFAULT 0 CHECK (cost_micros >= 0),
  usage_known INTEGER NOT NULL DEFAULT 0 CHECK (usage_known IN (0,1)),
  error TEXT,
  expires_at_ms INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  actual_request_sha256 TEXT NOT NULL DEFAULT '',
  max_requests INTEGER NOT NULL DEFAULT 1,
  requests_bound INTEGER NOT NULL DEFAULT 0,
  ledger_reservation_id TEXT REFERENCES inference_reservations(id) ON DELETE RESTRICT
);

INSERT INTO provider_turn_reservations_subscription
  (reservation_id,session_record_id,attempt_id,generation,provider,model,
   max_input_tokens,max_output_tokens,max_total_tokens,max_cost_micros,request_sha256,
   state,input_tokens,output_tokens,total_tokens,cost_micros,usage_known,error,
   expires_at_ms,created_at,updated_at,actual_request_sha256,max_requests,requests_bound,
   ledger_reservation_id)
SELECT reservation_id,session_record_id,attempt_id,generation,provider,model,
       max_input_tokens,max_output_tokens,max_total_tokens,max_cost_micros,request_sha256,
       state,input_tokens,output_tokens,total_tokens,cost_micros,usage_known,error,
       expires_at_ms,created_at,updated_at,actual_request_sha256,max_requests,requests_bound,
       ledger_reservation_id
FROM provider_turn_reservations;

DROP TABLE provider_turn_reservations;
ALTER TABLE provider_turn_reservations_subscription RENAME TO provider_turn_reservations;

CREATE UNIQUE INDEX IF NOT EXISTS provider_turn_one_reserved_idx
  ON provider_turn_reservations(session_record_id) WHERE state='RESERVED';
CREATE INDEX IF NOT EXISTS provider_turn_campaign_idx
  ON provider_turn_reservations(attempt_id,state);
CREATE UNIQUE INDEX IF NOT EXISTS provider_turn_ledger_reservation_idx
  ON provider_turn_reservations(ledger_reservation_id)
  WHERE ledger_reservation_id IS NOT NULL;
