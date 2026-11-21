use super::*;
use crate::planning::identity_facts::{
    IdentityFact, IdentityFactsChallenge, IdentityFactsEnvelope, IdentityFactsManifest,
    IdentityFactsMode, IdentityFactsReceipt, OperationRoot, closure_manifest_sha256, sign, verify,
};
use crate::planning::license_decision::{
    CandidateBinding, LicenseDecisionContext, LicenseDecisionReview, PolicyBinding,
    identity_facts_receipt_digest, verify_license_evidence_review,
};

const BEARER: &[u8] = b"scoped-environment-bearer-0123456789abcdef";
const PIN: &str = "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d";
const PLAN: &str = "c6cb4499d86380bd53dfe1c89a33882213b3e36af7e58250dee9f5d12acc377a";
const INVENTORY: &str = "16b2df71a8a7996ee5b433655b0c6c1773d23113a350e4b5b8e6074375df2773";
const EVIDENCE_BUNDLE: &str = "57bfc9052d4417c2a5ce92617936e5550fc461beba3e7e741a212aef5f0fae2e";
const BASE: &str = "4fdf1b15f862b2564642443a71ef3e25a2785013";
const OBSERVED_SPEC: &str = "067f8c8948e570e3ac2c8b8c8a9c47a0120759916b19e3c420308ff509d1b901";
const OUTPUT_PATH: &str = "/nix/store/36qzlljldgb41snr4l38pfpcy19w7s2j-ed-1.21.1";
const OUTPUT_DRV: &str = "/nix/store/frkdwgpbgh17hz57giw5rm3bs8bsflfb-ed-1.21.1.drv";
const OUTPUT_NAR: &str = "sha256-humgomxjPuMnaa5VWRl8ondk5lDth9N3px977TJOpG4=";
const SOURCE_DRV: &str = "/nix/store/j151nb5vh176kdrrh1rkilxkdkmvm47w-ed-1.21.1.tar.lz.drv";
const SOURCE_PATH: &str = "/nix/store/wa88qds9615j2r5imfryzbyg86c97iga-ed-1.21.1.tar.lz";
const SOURCE_HASH: &str = "sha256-1tDHGSsCsFGckCqTcZBT6GWt5aeEo7Mn2T2IhFeyPEs=";
const SOURCE_NAR: &str = "sha256-dEsPDHFVSahejFEnqz5gAMxx+ls+idLXQ6qOovYOecE=";
const REQUEST_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const RESPONSE: &str = r#"{"decision":"ACCEPT"}"#;
const PROJECT_CONFIG: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

struct Fixture {
    path: std::path::PathBuf,
    db: Database,
    decision: VerifiedLicenseDecision,
    owner_id: String,
}

fn receipt() -> IdentityFactsReceipt {
    let challenge = IdentityFactsChallenge {
        operation_id: "operation-1".into(),
        runtime_id: "runtime-1".into(),
        nonce: "identity-facts:test".into(),
        pin: PIN.into(),
        plan_sha256: PLAN.into(),
        spec_sha256: OBSERVED_SPEC.into(),
        mode: IdentityFactsMode::Questions,
        root: OperationRoot::Operation,
        questions: vec![
            crate::planning::identity_facts::IdentityQuestion {
                store_path: OUTPUT_PATH.into(),
                deriver: Some(OUTPUT_DRV.into()),
                nar_hash: OUTPUT_NAR.into(),
                outputs: vec![OUTPUT_PATH.into()],
            },
            crate::planning::identity_facts::IdentityQuestion {
                store_path: SOURCE_PATH.into(),
                deriver: Some(SOURCE_DRV.into()),
                nar_hash: SOURCE_NAR.into(),
                outputs: vec![SOURCE_PATH.into()],
            },
        ],
    };
    let facts = vec![
        IdentityFact {
            store_path: OUTPUT_PATH.into(),
            deriver: Some(OUTPUT_DRV.into()),
            nar_hash: OUTPUT_NAR.into(),
            outputs: vec![OUTPUT_PATH.into()],
        },
        IdentityFact {
            store_path: SOURCE_PATH.into(),
            deriver: Some(SOURCE_DRV.into()),
            nar_hash: SOURCE_NAR.into(),
            outputs: vec![SOURCE_PATH.into()],
        },
    ];
    let envelope = IdentityFactsEnvelope {
        schema_version: 1,
        context: crate::planning::identity_facts::IDENTITY_FACTS_CONTEXT.into(),
        challenge: challenge.clone(),
        manifest: IdentityFactsManifest {
            observed_root: OUTPUT_PATH.into(),
            fact_count: facts.len(),
            closure_manifest_sha256: closure_manifest_sha256(&facts),
            facts,
            truncated: false,
        },
    };
    let signature = sign(&envelope, BEARER).expect("the fixture receipt signs");
    IdentityFactsReceipt {
        envelope,
        signature,
    }
}

fn review() -> LicenseDecisionReview {
    LicenseDecisionReview {
        decision: "ACCEPT".into(),
        pin: PIN.into(),
        name: "ed".into(),
        version: "1.21.1".into(),
        output_drv_path: OUTPUT_DRV.into(),
        output_store_path: OUTPUT_PATH.into(),
        output_nar_hash: OUTPUT_NAR.into(),
        source_drv_path: SOURCE_DRV.into(),
        source_store_path: SOURCE_PATH.into(),
        source_output_hash: SOURCE_HASH.into(),
        license_expression: "GPL-2.0-or-later".into(),
        software_text_sha256: "022e9c2e9cc1aac3622313df6f971fbb39c1d1b0cb45324c16b8a1fe231c5ea6"
            .into(),
        selected_branch: "GPL-2.0-or-later".into(),
        documentation: vec![
            crate::planning::license_decision::DocumentationReview {
                path: "share/info/ed.info".into(),
                license: "GFDL-1.3-or-later".into(),
                output_sha256: "3618c355d1fd6b5ae66048499735d14a0dd0c438b29e5c2e41040f76fd03d19b"
                    .into(),
                license_text_path: "doc/fdl.texi".into(),
                license_text_sha256:
                    "a7e43ea88ee8fc4722d38482fd1a14c12bfbe2f0512d61980c810627b19cdce5".into(),
                obligations: vec!["notice".into(), "source".into()],
            },
            crate::planning::license_decision::DocumentationReview {
                path: "share/man/man1/ed.1.gz".into(),
                license: "GPL-2.0-or-later".into(),
                output_sha256: "891a6ac40cfdd4e3b58ef59444cd7544d2304feb986752a033270f69ec0a11c2"
                    .into(),
                license_text_path: "ed.h".into(),
                license_text_sha256:
                    "24f5c4349b47f7e8e8b35dd766d6f0309fca2fbfebb9ccd780788f8d37d51e58".into(),
                obligations: vec!["notice".into(), "source".into()],
            },
        ],
        use_context: "commercial-development".into(),
        commercial_use: true,
        payment_required: false,
        obligations_reviewed: true,
        software_obligations: vec!["notice".into(), "source".into()],
        documentation_obligations: vec!["notice".into(), "source".into()],
        request_sha256: REQUEST_SHA.into(),
        response_sha256: super::license_decisions_sha256(RESPONSE.as_bytes()),
    }
}

async fn fixture() -> Fixture {
    let path = std::env::temp_dir().join(format!(
        "factory-license-decisions-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    let db = Database::open(&path).await.unwrap();
    let now = Utc::now().to_rfc3339();
    sqlx::query::query(
        "INSERT INTO projects(id,repository,base_commit,created_at,identity)
         VALUES ('project-1','/fixture',?1,?2,'')",
    )
    .bind(BASE)
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO standing_delegations
         (id,principal_id,audit_id,project_id,revision,content_hash,content_json,revoked_at,created_at)
         VALUES ('standing-1','operator','audit-1','project-1',1,?1,'{}',NULL,?2)",
    )
    .bind("b".repeat(64))
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO actions
         (id,project_id,objective,provider,model,risk,status,objective_sha256,created_at)
         VALUES ('action-1','project-1','objective','provider','model','R0','PLANNED',?1,?2)",
    )
    .bind("c".repeat(64))
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO lead_requests
         (id,project_id,action_id,delegation_id,objective,request_hash,project_config_hash,state,failure_reason,created_at,updated_at)
         VALUES ('lead-1','project-1','action-1','standing-1','objective',?1,?2,'RUNNING',NULL,?3,?3)",
    )
    .bind("d".repeat(64))
    .bind("e".repeat(64))
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO environment_plans
         (id,plan_id,project_id,request_id,action_id,campaign_id,revision,base_commit,risk_class,
          conventions_sha256,plan_sha256,status,source,builder_image,builder_image_digest,
          license_inventory_sha256,sbom_sha256,lead_review_id,content_json,created_at,updated_at)
         VALUES ('plan-row','plan-1','project-1',NULL,'action-1',NULL,1,?1,'R0',?2,?3,
                 'CORE_VALIDATED','GENERATED','','',?4,?5,NULL,'{}',?6,?6)",
    )
    .bind(BASE)
    .bind("c".repeat(64))
    .bind(PLAN)
    .bind(INVENTORY)
    .bind(EVIDENCE_BUNDLE)
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query::query(
        "INSERT INTO model_call_reservations
         (reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,model,
          purpose,request_sha256,max_input_tokens,max_output_tokens,max_total_tokens,
          max_cost_micros,response_text,state,expires_at_ms,created_at,updated_at)
         VALUES ('reservation-1','LEAD','operator','project-1','lead-1','scope-1','provider','model',
                 'license_evidence_review',?1,1000,1000,2000,1000,?2,'SETTLED',?3,?4,?4)",
    )
    .bind(REQUEST_SHA)
    .bind(RESPONSE)
    .bind(Utc::now().timestamp_millis() + 60_000)
    .bind(&now)
    .execute(&db.pool)
    .await
    .unwrap();
    let owner = db
        .create_agentic_owner_decision(&NewAgenticOwnerDecision {
            project_id: "project-1",
            request_id: None,
            campaign_id: None,
            matter: LICENSE_DELEGATION_MATTER,
            risk: crate::lead::RiskClass::R0,
            options: &[LICENSE_DELEGATION_OPTION.into(), "DENY_DELEGATION".into()],
            evidence: &serde_json::json!({
                "domain": "licenses-identity-evidence",
                "pin": PIN,
                "useContext": "commercial-development",
                "economicCriterion": "no-mandatory-fee-commercial-use",
                "revision": 7,
            }),
            recommendation: "Delegate the exact licence decision.",
            deadline: &(Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
        })
        .await
        .unwrap();
    db.resolve_agentic_owner_decision(
        &owner.id,
        LICENSE_DELEGATION_OPTION,
        "Owner delegated the exact decision.",
        "operator",
    )
    .await
    .unwrap();

    let receipt = receipt();
    let facts = verify(&receipt, BEARER, &receipt.envelope.challenge.clone()).unwrap();
    let binding = PolicyBinding::test_binding(
        PLAN,
        1,
        None,
        INVENTORY,
        EVIDENCE_BUNDLE,
        CandidateBinding::base(BASE).unwrap(),
        &identity_facts_receipt_digest(&facts),
    );
    let reservation = db
        .lead_license_evidence_reservation("lead-1")
        .await
        .unwrap();
    let delegation = db.agentic_owner_decision(&owner.id).await.unwrap();
    let context = LicenseDecisionContext {
        project_id: "project-1",
        lead_request_id: "lead-1",
        principal: "operator",
        request_sha256: REQUEST_SHA,
        project_config_hash: PROJECT_CONFIG,
        base_commit: BASE,
        reservation: &reservation,
        delegation: &delegation,
        delegation_revoked: false,
        identity_facts: &facts,
        binding: &binding,
    };
    let decision = verify_license_evidence_review(&review(), &context).unwrap();
    Fixture {
        path,
        db,
        decision,
        owner_id: owner.id,
    }
}

#[tokio::test]
async fn a_verified_decision_is_durable_idempotent_and_revocable() {
    let fixture = fixture().await;
    let db = &fixture.db;
    let first = db
        .create_license_decision(&fixture.decision, "decision-1")
        .await
        .unwrap();
    assert_eq!(first.status, "ACTIVE");
    assert_eq!(first.lead_request_sha256, REQUEST_SHA);
    assert_eq!(first.lead_principal, "operator");
    assert_eq!(first.delegation_option_sha256.len(), 64);
    assert_eq!(first.software_obligations, ["notice", "source"]);
    assert_eq!(first.documentation.len(), 2);
    assert_eq!(first.plan_sha256, PLAN);
    assert_eq!(first.plan_revision, 1);
    assert_eq!(first.inventory_sha256, INVENTORY);
    assert_eq!(first.spec_sha256, None);

    let replay = db
        .create_license_decision(&fixture.decision, "decision-1")
        .await
        .unwrap();
    assert_eq!(first, replay);

    // A tampered option binding is refused by the transaction-bound check.
    let mut tampered = fixture.decision.clone();
    tampered.delegation_option_sha256 = "f".repeat(64);
    assert!(matches!(
        db.create_license_decision(&tampered, "tampered-option")
            .await,
        Err(Error::Conflict(_))
    ));

    // The same key with different content is a conflict, never a silent update.
    let mut changed_review = review();
    changed_review.decision = "DENY".into();
    let changed = {
        let reservation = db
            .lead_license_evidence_reservation("lead-1")
            .await
            .unwrap();
        let delegation = db.agentic_owner_decision(&fixture.owner_id).await.unwrap();
        let facts = verify(&receipt(), BEARER, &receipt().envelope.challenge.clone()).unwrap();
        let binding = PolicyBinding::test_binding(
            PLAN,
            1,
            None,
            INVENTORY,
            EVIDENCE_BUNDLE,
            CandidateBinding::base(BASE).unwrap(),
            &identity_facts_receipt_digest(&facts),
        );
        let context = LicenseDecisionContext {
            project_id: "project-1",
            lead_request_id: "lead-1",
            principal: "operator",
            request_sha256: REQUEST_SHA,
            project_config_hash: PROJECT_CONFIG,
            base_commit: BASE,
            reservation: &reservation,
            delegation: &delegation,
            delegation_revoked: false,
            identity_facts: &facts,
            binding: &binding,
        };
        verify_license_evidence_review(&changed_review, &context).unwrap()
    };
    assert!(matches!(
        db.create_license_decision(&changed, "decision-1").await,
        Err(Error::Conflict(_))
    ));

    // Revocation is actor and project bound.
    assert!(matches!(
        db.revoke_license_decision(&first.id, "project-2", "operator", "superseded")
            .await,
        Err(Error::NotFound)
    ));
    let revoked = db
        .revoke_license_decision(&first.id, "project-1", "operator", "superseded")
        .await
        .unwrap();
    assert_eq!(revoked.status, "REVOKED");
    assert_eq!(revoked.revoked_by.as_deref(), Some("operator"));
    assert_eq!(revoked.revocation_reason.as_deref(), Some("superseded"));
    assert!(matches!(
        db.revoke_license_decision(&first.id, "project-1", "operator", "different")
            .await,
        Err(Error::Conflict(_))
    ));

    // A revoked delegation fences new decisions.
    assert!(matches!(
        db.revoke_license_delegation(&fixture.owner_id, "project-2", "operator", "owner revoke")
            .await,
        Err(Error::NotFound)
    ));
    db.revoke_license_delegation(&fixture.owner_id, "project-1", "operator", "owner revoke")
        .await
        .unwrap();
    assert!(
        db.license_delegation_revoked(&fixture.owner_id)
            .await
            .unwrap()
    );
    assert!(matches!(
        db.create_license_decision(&fixture.decision, "decision-2")
            .await,
        Err(Error::Conflict(_))
    ));

    db.close().await;
    let reopened = Database::open(&fixture.path).await.unwrap();
    let durable = reopened.license_decision(&first.id).await.unwrap();
    assert_eq!(durable.revoked_at, revoked.revoked_at);
    assert_eq!(durable.revoked_by.as_deref(), Some("operator"));
    reopened.close().await;
    let _ = std::fs::remove_file(&fixture.path);
}

#[tokio::test]
async fn revocation_between_verification_and_persistence_is_refused() {
    let fixture = fixture().await;
    let db = &fixture.db;
    db.revoke_license_delegation(&fixture.owner_id, "project-1", "operator", "owner revoke")
        .await
        .unwrap();
    assert!(matches!(
        db.create_license_decision(&fixture.decision, "decision-race")
            .await,
        Err(Error::Conflict(_))
    ));
    db.close().await;
    let _ = std::fs::remove_file(&fixture.path);
}

#[tokio::test]
async fn delegation_evidence_lead_state_and_grant_identity_are_revalidated() {
    let grant_fixture = fixture().await;
    let first = grant_fixture
        .db
        .create_license_decision(&grant_fixture.decision, "grant-key-1")
        .await
        .unwrap();
    // A retry with a different key spelling must not duplicate the grant.
    let same = grant_fixture
        .db
        .create_license_decision(&grant_fixture.decision, "grant-key-2")
        .await
        .unwrap();
    assert_eq!(same.id, first.id);
    assert_eq!(
        grant_fixture
            .db
            .license_decisions("project-1")
            .await
            .unwrap()
            .len(),
        1
    );
    grant_fixture.db.close().await;
    let _ = std::fs::remove_file(&grant_fixture.path);

    // A stage plan that is not the exact persisted Lead request plan is refused.
    let fake_stage_fixture = fixture().await;
    let mut fake_stage = fake_stage_fixture.decision.clone();
    let digest = fake_stage.identity_facts_receipt_digest().to_owned();
    fake_stage.binding = PolicyBinding::test_binding(
        &"f".repeat(64),
        1,
        None,
        INVENTORY,
        EVIDENCE_BUNDLE,
        CandidateBinding::base(BASE).unwrap(),
        &digest,
    );
    assert!(matches!(
        fake_stage_fixture
            .db
            .create_license_decision(&fake_stage, "fake-stage")
            .await,
        Err(Error::Conflict(_))
    ));
    fake_stage_fixture.db.close().await;
    let _ = std::fs::remove_file(&fake_stage_fixture.path);

    // Delegation evidence changes after verification are refused transactionally.
    let evidence_fixture = fixture().await;
    sqlx::query::query("UPDATE agentic_owner_decisions SET evidence_json=?1 WHERE id=?2")
        .bind(
            serde_json::json!({
                "domain": "licenses-identity-evidence",
                "pin": PIN,
                "useContext": "commercial-development",
                "economicCriterion": "no-mandatory-fee-commercial-use",
                "revision": 8,
            })
            .to_string(),
        )
        .bind(&evidence_fixture.owner_id)
        .execute(&evidence_fixture.db.pool)
        .await
        .unwrap();
    assert!(matches!(
        evidence_fixture
            .db
            .create_license_decision(&evidence_fixture.decision, "evidence-change")
            .await,
        Err(Error::Conflict(_))
    ));
    evidence_fixture.db.close().await;
    let _ = std::fs::remove_file(&evidence_fixture.path);

    // A terminal lead cannot receive a new decision.
    let terminal_fixture = fixture().await;
    sqlx::query::query("UPDATE lead_requests SET state='FAILED' WHERE id='lead-1'")
        .execute(&terminal_fixture.db.pool)
        .await
        .unwrap();
    assert!(matches!(
        terminal_fixture
            .db
            .create_license_decision(&terminal_fixture.decision, "lead-failed")
            .await,
        Err(Error::Conflict(_))
    ));
    terminal_fixture.db.close().await;
    let _ = std::fs::remove_file(&terminal_fixture.path);

    // A withdrawal is also terminal even if the state still says RUNNING.
    let withdrawn_fixture = fixture().await;
    sqlx::query::query(
        "INSERT INTO lead_withdrawals(request_id,principal_id,principal_label,reason,created_at)
         VALUES ('lead-1','operator','operator','withdrawn',?1)",
    )
    .bind(Utc::now().to_rfc3339())
    .execute(&withdrawn_fixture.db.pool)
    .await
    .unwrap();
    assert!(matches!(
        withdrawn_fixture
            .db
            .create_license_decision(&withdrawn_fixture.decision, "lead-withdrawn")
            .await,
        Err(Error::Conflict(_))
    ));
    withdrawn_fixture.db.close().await;
    let _ = std::fs::remove_file(&withdrawn_fixture.path);
}

#[tokio::test]
async fn revocation_audit_rows_are_appended_once_and_never_removed() {
    let fixture = fixture().await;
    let db = &fixture.db;
    let decision = db
        .create_license_decision(&fixture.decision, "audit-key")
        .await
        .unwrap();
    db.revoke_license_decision(&decision.id, "project-1", "operator", "superseded")
        .await
        .unwrap();
    assert!(
        sqlx::query::query("DELETE FROM license_decision_revocations WHERE decision_id=?1")
            .bind(&decision.id)
            .execute(&db.pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query::query(
            "UPDATE license_decisions SET revocation_reason='rewritten' WHERE id=?1"
        )
        .bind(&decision.id)
        .execute(&db.pool)
        .await
        .is_err()
    );

    db.revoke_license_delegation(&fixture.owner_id, "project-1", "operator", "owner revoke")
        .await
        .unwrap();
    assert!(
        sqlx::query::query("DELETE FROM license_delegation_revocations WHERE delegation_id=?1")
            .bind(&fixture.owner_id)
            .execute(&db.pool)
            .await
            .is_err()
    );
    db.close().await;
    let _ = std::fs::remove_file(&fixture.path);
}
