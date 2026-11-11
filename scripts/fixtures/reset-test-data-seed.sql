-- Deterministic seed for scripts/reset-test-data.test.sh.
--
-- One row is inserted for each meaningful class: preserved operator
-- configuration and transactional Campaign/model/provider rows that carry
-- immutable-delete triggers. The test applies the real migrations first, so
-- schema drift is exercised rather than mocked.

PRAGMA foreign_keys=OFF;

INSERT INTO projects
  (id,repository,base_commit,created_at,identity,name,source_kind,source_url,
   source_key,source_visibility,default_branch,remote_origin)
VALUES
  ('project-1','file:///tmp/repository','base-commit','2026-01-01T00:00:00Z',
   'local','Project One','local','/tmp/repository','','','main','');

INSERT INTO project_configs
  (project_id,revision,config_hash,repository,base_commit,allowed_paths_json,
   capabilities_json,quality_profile,active,updated_at)
VALUES
  ('project-1',1,'config-hash','/tmp/repository','base-commit','["src"]',
   '[]','standard',1,'2026-01-01T00:00:00Z');

INSERT INTO execution_profiles
  (id,current_revision,content_hash,created_at,updated_at)
VALUES
  ('profile-1',1,'profile-hash','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');

INSERT INTO execution_profile_revisions
  (revision_id,profile_id,revision,content_json,content_hash,actor,created_at,
   schema_version)
VALUES
  ('revision-1','profile-1',1,'{}','profile-hash','owner',
   '2026-01-01T00:00:00Z',1);

INSERT INTO execution_profile_defaults
  (scope,project_id,profile_id,actor,updated_at)
VALUES
  ('project','project-1','profile-1','owner','2026-01-01T00:00:00Z');

INSERT INTO provider_settings
  (provider,kind,model,base_url,auth_mode,credential_ciphertext,
   credential_nonce,credential_fingerprint,enabled,is_default,max_turns,
   max_input_tokens,max_output_tokens,max_cost_micros,
   input_micros_per_million,output_micros_per_million,revision,updated_at)
VALUES
  ('opencode','opencode','test/model','https://provider.invalid','api_key',
   'ciphertext','nonce','fingerprint',1,1,10,1000,1000,100000,1,1,1,
   '2026-01-01T00:00:00Z');

INSERT INTO provider_settings_audit
  (audit_id,provider,revision,action,actor,detail,created_at)
VALUES
  ('provider-audit-1','opencode',1,'configured','owner','fixture',
   '2026-01-01T00:00:00Z');

INSERT INTO github_connections
  (id,login,scopes_json,scopes_reported,credential_ciphertext,
   credential_nonce,credential_fingerprint,enabled,revision,verified_at,
   revoked_at,created_at,updated_at,rate_limited_until)
VALUES
  ('default','owner','["repo"]',1,'ciphertext','nonce','fingerprint',1,1,
   '2026-01-01T00:00:00Z',NULL,'2026-01-01T00:00:00Z',
   '2026-01-01T00:00:00Z',NULL);

INSERT INTO project_github_bindings
  (project_id,owner,repo,repo_full_name,default_branch,bound_at,updated_at,
   repository_github_id)
VALUES
  ('project-1','owner','repo','owner/repo','main','2026-01-01T00:00:00Z',
   '2026-01-01T00:00:00Z',1);

INSERT INTO agentic_owner_charters
  (id,owner,project_id,domains_json,risk_appetite,execution_autonomy,
   repair_autonomy,delivery_autonomy,policy_autonomy,reserved_matters_json,
   expires_at,revoked_at,revocation_reason,created_at,updated_at)
VALUES
  ('charter-1','owner','project-1','["CODE"]','R1',1,0,0,0,'[]',
   '2030-01-01T00:00:00Z',NULL,NULL,'2026-01-01T00:00:00Z',
   '2026-01-01T00:00:00Z');

INSERT INTO actions
  (id,project_id,objective,provider,model,risk,status,objective_sha256,
   created_at)
VALUES
  ('action-1','project-1','Test objective','opencode','test/model','R0',
   'COMPLETED','action-hash','2026-01-01T00:00:00Z');

INSERT INTO campaigns (id,action_id,status,created_at)
VALUES ('campaign-1','action-1','COMPLETED','2026-01-01T00:00:00Z');

INSERT INTO runs (id,campaign_id,status,created_at)
VALUES ('run-1','campaign-1','COMPLETED','2026-01-01T00:00:00Z');

INSERT INTO attempts (id,run_id,status,created_at,sandbox_ready)
VALUES ('attempt-1','run-1','COMPLETED','2026-01-01T00:00:00Z',1);

INSERT INTO candidates
  (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,
   created_at,integration,lineage_json)
VALUES
  ('candidate-1','attempt-1','spec-hash','commit-sha','tree-sha','worker-1',
   '2026-01-01T00:00:00Z',0,'[]');

INSERT INTO quality_receipts
  (receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,
   tree_sha,result,evidence,received_at,findings_json,mechanism,
   model_provider,model_name,model_reservation_id,input_tokens,output_tokens,
   total_tokens,cost_micros,usage_known,model_request_sha256,reviewer_role)
VALUES
  ('receipt-1','candidate-1','verifier','verifier-process','spec-hash',
   'commit-sha','tree-sha','PASS','EXECUTED','2026-01-01T00:00:00Z','[]',
   'DETERMINISTIC','','','',0,0,0,0,0,'','');

INSERT INTO inference_envelopes
  (id,request_id,project_id,currency,state,authorized_micros,
   authorization_revision,created_at_ms,revoked_at_ms)
VALUES
  ('envelope-1','request-1','project-1','USD','ACTIVE',1000000,1,
   1767225600000,NULL);

INSERT INTO inference_reservations
  (id,envelope_id,operation_id,idempotency_key,request_sha256,role,
   authorization_revision,scope_sha256,principal_id,parent_operation_id,
   provider,model,pricing_revision,pricing_snapshot_json,upper_bound_micros,
   settled_micros,state,worker_id,generation,dispatched_at_ms,
   provider_request_id,expires_at_ms,settlement_sha256,created_at_ms,
   updated_at_ms)
VALUES
  ('ledger-reservation-1','envelope-1','operation-1','idempotency-1',
   'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
   'EVALUATION',1,
   'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
   'evaluator',NULL,'opencode','test/model','pricing-1','{}',1000,NULL,
   'AMBIGUOUS',NULL,NULL,NULL,NULL,1767225600000,NULL,1767225600000,
   1767225600000);

INSERT INTO inference_ledger_events
  (id,envelope_id,reservation_id,kind,actor,evidence_ref,payload_sha256,
   created_at_ms)
VALUES
  ('ledger-event-1','envelope-1','ledger-reservation-1','AMBIGUOUS','core',
   'test','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
   1767225600000);

INSERT INTO model_call_reservations
  (reservation_id,role,principal_id,project_id,objective_id,scope_id,
   provider,model,purpose,request_sha256,max_input_tokens,max_output_tokens,
   max_total_tokens,max_cost_micros,input_tokens,output_tokens,total_tokens,
   cost_micros,usage_known,response_text,state,error,expires_at_ms,created_at,
   updated_at,ledger_reservation_id,dispatched_at_ms,provider_request_id)
VALUES
  ('model-call-1','EVALUATION','evaluator','project-1','objective-1',
   'scope-1','opencode','test/model','evaluation',
   'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
   1000,1000,2000,1000,0,0,0,0,0,NULL,'AMBIGUOUS',
   'old unknown reservation',1767225600000,'2026-01-01T00:00:00Z',
   '2026-01-01T00:00:00Z','ledger-reservation-1',1767225600000,NULL);

INSERT INTO events (sequence,subject_id,kind,detail,recorded_at)
VALUES ('1','campaign-1','campaign.created','created','2026-01-01T00:00:00Z');

INSERT INTO agentic_owner_decisions
  (id,project_id,request_id,campaign_id,matter,risk,status,options_json,
   evidence_json,recommendation,deadline,selected_option,rationale,resolved_by,
   created_at,updated_at,resolved_at)
VALUES
  ('decision-1','project-1',NULL,'campaign-1','scope','R0','PENDING','[]','[]',
   'approve','2030-01-01T00:00:00Z',NULL,NULL,NULL,'2026-01-01T00:00:00Z',
   '2026-01-01T00:00:00Z',NULL);

INSERT INTO provider_sessions
  (session_record_id,attempt_id,worker_id,generation,provider,model,
   provider_mode,state,provider_session_id,provider_directory,turn_id,
   input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
   cleanup_receipt,error,created_at,updated_at)
VALUES
  ('session-1','attempt-1','worker-1',1,'opencode','test/model','container',
   'COMPLETED','provider-session-1','/tmp/session',NULL,0,0,0,0,0,NULL,NULL,
   '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');

INSERT INTO provider_turn_reservations
  (reservation_id,session_record_id,attempt_id,generation,provider,model,
   max_input_tokens,max_output_tokens,max_total_tokens,max_cost_micros,
   request_sha256,state,input_tokens,output_tokens,total_tokens,cost_micros,
   usage_known,error,expires_at_ms,created_at,updated_at,
   actual_request_sha256,max_requests,requests_bound,ledger_reservation_id)
VALUES
  ('turn-1','session-1','attempt-1',1,'opencode','test/model',1000,1000,2000,
   1000,
   'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
   'RELEASED',0,0,0,0,0,NULL,1767225600000,'2026-01-01T00:00:00Z',
   '2026-01-01T00:00:00Z','',1,0,'ledger-reservation-1');
