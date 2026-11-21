use super::*;
use crate::planning::environment::{
    EnvironmentBuildReceipt, EnvironmentBuilderStatus, EnvironmentPlan, EnvironmentSpec,
    SandboxAttestation,
};
use crate::planning::identity_facts::{
    IdentityFact, IdentityFactsChallenge, IdentityFactsEnvelope, IdentityFactsManifest,
    IdentityFactsMode, IdentityFactsReceipt, IdentityFactsRootBinding, OperationRoot,
    closure_manifest_sha256, sign, verify,
};
use crate::planning::lead_classification::sha256_hex;

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
const SOURCE_FLAT: &str = "d6d0c7192b02b0519c902a93719053e865ade5a784a3b327d93d888457b23c4b";
const SOURCE_NAR_NIX32: &str = "1hbr1vva53ma8gbx529ybgx73k00c0zan9siiigahjamf460yjvl";
const SOURCE_SIZE: u64 = 70136;
const SOURCE_SIGNATURE: &str = "cache.nixos.org-1:xYwC8Vo2buDIPQNzCyHhC8Kz7Y6riU596Zp3tnRTIicdLtih5xQzAPlip25FtXIsShiTfkl4eABEg4TX0c+ZDA==";
const SOURCE_CA: &str = "fixed:sha256:0jrwn9bq921xv4kv78w4lzjssrg8af8734raj2f53c025ccwgl6n";
const PLAN_ROOT: &str = "/nix/store/cccccccccccccccccccccccccccccccc-plan-root";

fn source_proof() -> SourceEvidenceProof {
    SourceEvidenceProof {
        source_store_path: SOURCE_PATH.into(),
        source_drv_path: SOURCE_DRV.into(),
        flat_sha256: SOURCE_FLAT.into(),
        nar_sri: SOURCE_NAR.into(),
        nar_size: SOURCE_SIZE,
        signature: SOURCE_SIGNATURE.into(),
        output_drv_path: OUTPUT_DRV.into(),
        output_store_path: OUTPUT_PATH.into(),
        output_drv_input_drvs: vec![
            "/nix/store/05q48dcd4lgk4vh7wyk330gr2fr082i2-bootstrap-tools.drv".into(),
            "/nix/store/37wqz292qyx3z03p4s50lrs32vcgq9f4-bash-5.2p37.drv".into(),
            SOURCE_DRV.into(),
            "/nix/store/vxv9n01jpwvh83byh2q72mcf4vqlm3j5-lzip-1.25.drv".into(),
            "/nix/store/zd1gad3l5xmc1i9hivj0l468srnsmvb2-bootstrap-stage4-stdenv-linux.drv".into(),
        ],
        output_drv_aterm_sha256: "294060511213ba226bb3687e28ad65650d8058e4b60fab50157ba19d2ad46d8c"
            .into(),
        source_drv_aterm_sha256: "5c1177a7e9a55b0e5fc1860375b211e13560ae88776e3796fee8ec105d9cf52e"
            .into(),
        nar_nix32: Some(SOURCE_NAR_NIX32.into()),
        fingerprint: None,
        ca: Some(SOURCE_CA.into()),
    }
}

fn source_evidence() -> VerifiedSourceEvidence {
    VerifiedSourceEvidence::verify(&source_proof()).expect("the pinned cache signature verifies")
}

const REQUEST_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROJECT_CONFIG: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const RESPONSE: &str = r#"{"decision":"ACCEPT"}"#;

fn challenge(nar_hash: &str) -> IdentityFactsChallenge {
    IdentityFactsChallenge {
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
                nar_hash: nar_hash.into(),
                outputs: vec![OUTPUT_PATH.into()],
            },
            crate::planning::identity_facts::IdentityQuestion {
                store_path: SOURCE_PATH.into(),
                deriver: Some(SOURCE_DRV.into()),
                nar_hash: SOURCE_NAR.into(),
                outputs: vec![SOURCE_PATH.into()],
            },
        ],
        stage: crate::planning::identity_facts::IdentityFactsStage::Spec,
        resolution_key: String::new(),
    }
}

fn facts(nar_hash: &str) -> Vec<IdentityFact> {
    vec![
        IdentityFact {
            store_path: OUTPUT_PATH.into(),
            deriver: Some(OUTPUT_DRV.into()),
            nar_hash: nar_hash.into(),
            outputs: vec![OUTPUT_PATH.into()],
        },
        IdentityFact {
            store_path: SOURCE_PATH.into(),
            deriver: Some(SOURCE_DRV.into()),
            nar_hash: SOURCE_NAR.into(),
            outputs: vec![SOURCE_PATH.into()],
        },
    ]
}

fn receipt(nar_hash: &str) -> IdentityFactsReceipt {
    let challenge = challenge(nar_hash);
    let facts = facts(nar_hash);
    let envelope = IdentityFactsEnvelope {
        schema_version: 1,
        context: IDENTITY_FACTS_CONTEXT.into(),
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

fn verified(nar_hash: &str) -> VerifiedIdentityFacts {
    let receipt = receipt(nar_hash);
    verify(&receipt, BEARER, &challenge(nar_hash)).expect("the fixture receipt verifies")
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
        // COPYING: the GPL-2.0 licence text observed by the builder.
        software_texts: vec![],
        software_text_sha256: "022e9c2e9cc1aac3622313df6f971fbb39c1d1b0cb45324c16b8a1fe231c5ea6"
            .into(),
        selected_branch: "GPL-2.0-or-later".into(),
        documentation: vec![
            DocumentationReview {
                path: "share/info/ed.info".into(),
                license: "GFDL-1.3-or-later".into(),
                output_sha256: "3618c355d1fd6b5ae66048499735d14a0dd0c438b29e5c2e41040f76fd03d19b"
                    .into(),
                license_text_path: "doc/fdl.texi".into(),
                license_text_sha256:
                    "a7e43ea88ee8fc4722d38482fd1a14c12bfbe2f0512d61980c810627b19cdce5".into(),
                obligations: vec!["notice".into(), "source".into()],
            },
            DocumentationReview {
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
        request_sha256: REQUEST_SHA.to_owned(),
        response_sha256: sha256_hex(RESPONSE.as_bytes()),
    }
}

fn reservation() -> crate::db::LicenseEvidenceReservation {
    crate::db::LicenseEvidenceReservation {
        reservation_id: "reservation-1".into(),
        role: "LEAD".into(),
        project_id: "project-1".into(),
        objective_id: "lead-1".into(),
        scope_id: "scope-1".into(),
        provider: "provider".into(),
        model: "model".into(),
        purpose: "license_evidence_review".into(),
        request_sha256: REQUEST_SHA.to_owned(),
        state: "SETTLED".into(),
        response_text: RESPONSE.into(),
        principal_id: "operator".into(),
        expires_at_ms: chrono::Utc::now().timestamp_millis() + 60_000,
        lead_state: "RUNNING".into(),
        withdrawn: false,
        updated_at: chrono::Utc::now().to_rfc3339(),
    }
}

fn delegation() -> crate::db::AgenticOwnerDecisionRecord {
    crate::db::AgenticOwnerDecisionRecord {
        id: "delegation-1".into(),
        project_id: "project-1".into(),
        request_id: None,
        campaign_id: None,
        matter: "LICENSE_IDENTITY_EVIDENCE_DELEGATION".into(),
        risk: crate::lead::RiskClass::R0,
        status: "APPROVED".into(),
        options: vec!["APPROVE_DELEGATION".into(), "DENY".into()],
        evidence: serde_json::json!({
            "domain": "licenses-identity-evidence",
            "pin": PIN,
            "useContext": "commercial-development",
            "economicCriterion": "no-mandatory-fee-commercial-use",
            "revision": 7,
        }),
        recommendation: "delegate the exact decision".into(),
        deadline: (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
        selected_option: Some("APPROVE_DELEGATION".into()),
        rationale: Some("owner".into()),
        resolved_by: Some("operator".into()),
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
        resolved_at: Some(chrono::Utc::now().to_rfc3339()),
    }
}

fn binding(facts: &BoundIdentityFacts) -> PolicyBinding {
    PolicyBinding::test_binding(
        PLAN,
        1,
        None,
        INVENTORY,
        EVIDENCE_BUNDLE,
        CandidateBinding::base(BASE).expect("base candidate"),
        &identity_facts_receipt_digest(facts.verified()),
    )
}

struct Fixture {
    facts: BoundIdentityFacts,
    binding: PolicyBinding,
    reservation: crate::db::LicenseEvidenceReservation,
    delegation: crate::db::AgenticOwnerDecisionRecord,
    source: VerifiedSourceEvidence,
}

impl Fixture {
    fn new() -> Self {
        let facts = BoundIdentityFacts::spec_cache_observation(verified(OUTPUT_NAR));
        let binding = binding(&facts);
        Self {
            facts,
            binding,
            reservation: reservation(),
            delegation: delegation(),
            source: source_evidence(),
        }
    }

    fn context(&self) -> LicenseDecisionContext<'_> {
        LicenseDecisionContext {
            project_id: "project-1",
            lead_request_id: "lead-1",
            principal: "operator",
            request_sha256: REQUEST_SHA,
            project_config_hash: PROJECT_CONFIG,
            base_commit: BASE,
            reservation: &self.reservation,
            delegation: &self.delegation,
            delegation_revoked: false,
            identity_facts: &self.facts,
            source_evidence: &self.source,
            binding: &self.binding,
        }
    }

    fn verified_with(
        &self,
        review: &LicenseDecisionReview,
    ) -> Result<VerifiedLicenseDecision, LicenseDecisionRefusal> {
        verify_license_evidence_review(review, &self.context())
    }
}

#[test]
fn exact_ed_evidence_yields_exact_software_and_documentation_decisions() {
    let fixture = Fixture::new();
    let decision = fixture.verified_with(&review()).expect("exact evidence");
    assert_eq!(decision.license_expression(), "GPL-2.0-or-later");
    assert_eq!(decision.selected_branch(), "GPL-2.0-or-later");
    assert_eq!(decision.software_obligations(), ["notice", "source"]);
    assert_eq!(decision.documentation_obligations(), ["notice", "source"]);
    assert_eq!(decision.documentation().len(), 2);
    assert_eq!(decision.documentation()[0].path, "share/info/ed.info");
    assert_eq!(decision.documentation()[0].verdict, "UNKNOWN_CLASSIFIED");
    assert_eq!(decision.documentation()[1].verdict, "ALLOWED");
    assert_eq!(decision.software_verdict(), "ALLOWED");
    assert_eq!(decision.revision(), 7);
    assert_eq!(decision.content_sha256().len(), 64);
    assert_eq!(decision.identity_facts_receipt_digest().len(), 64);
}

#[test]
fn a_reviewed_expression_that_is_not_the_identity_fact_is_refused() {
    // D2: `ed` is GPL-2.0-or-later, so a model review claiming MIT is refused
    // even when the rest of the evidence is exact.
    let fixture = Fixture::new();
    let mut mit = review();
    mit.license_expression = "MIT".into();
    mit.selected_branch = "MIT".into();
    mit.software_obligations = vec!["notice".into()];
    mit.software_text_sha256 =
        "d61b72d6cf49f497102555b1d6eee1c636baff95f6f8eb49d443edbb01b617b4".into();
    assert_eq!(
        fixture.verified_with(&mit),
        Err(LicenseDecisionRefusal::LicenseExpressionMismatch)
    );
}

#[test]
fn a_selected_branch_outside_the_expression_ast_is_refused() {
    // D3: a foreign branch cannot borrow another licence's obligations.
    let fixture = Fixture::new();
    let mut foreign = review();
    foreign.selected_branch = "AGPL-3.0-only".into();
    foreign.software_obligations = vec!["notice".into(), "source".into(), "network".into()];
    assert_eq!(
        fixture.verified_with(&foreign),
        Err(LicenseDecisionRefusal::BranchNotInExpression)
    );
    assert_eq!(
        selectable_branches("MIT OR Apache-2.0").unwrap(),
        ["MIT", "Apache-2.0"]
    );
    assert_eq!(
        selectable_branches("GPL-2.0-only WITH Classpath-exception-2.0").unwrap(),
        ["GPL-2.0-only WITH Classpath-exception-2.0"]
    );
    assert!(selectable_branches("MIT AND").is_err());
    assert_eq!(
        selectable_branches("GPL-2.0-or-later AND MIT").unwrap(),
        ["GPL-2.0-or-later AND MIT"]
    );
    assert_eq!(
        selectable_branches("(MIT OR Apache-2.0) AND GPL-2.0-only").unwrap(),
        ["MIT AND GPL-2.0-only", "Apache-2.0 AND GPL-2.0-only"]
    );
    assert_eq!(
        exact_software_obligations("GPL-2.0-or-later AND MIT", "MIT"),
        Err(LicenseDecisionRefusal::BranchNotInExpression)
    );
    assert_eq!(
        branch_obligations("GPL-2.0-or-later AND MIT").unwrap(),
        ["notice", "source"]
    );
}

#[test]
fn documentation_is_never_ignored_and_obligations_are_exact() {
    // D4: GFDL is classified but UNKNOWN; its obligations are exact and a
    // missing/blanket documentation record is refused.
    let fixture = Fixture::new();
    let mut missing = review();
    missing.documentation.clear();
    assert_eq!(
        fixture.verified_with(&missing),
        Err(LicenseDecisionRefusal::DocumentationMismatch)
    );
    let mut blanket = review();
    blanket.documentation[0].obligations = vec!["notice".into()];
    assert_eq!(
        fixture.verified_with(&blanket),
        Err(LicenseDecisionRefusal::ObligationsMismatch)
    );
    let mut blank = review();
    blank.documentation_obligations = vec![];
    assert_eq!(
        fixture.verified_with(&blank),
        Err(LicenseDecisionRefusal::ObligationsMismatch)
    );
    let mut no_text = review();
    no_text.documentation[0].license_text_sha256 = "f".repeat(64);
    assert_eq!(
        fixture.verified_with(&no_text),
        Err(LicenseDecisionRefusal::DocumentationTextMissing)
    );
    let mut wrong_output = review();
    wrong_output.documentation[0].output_sha256 = "f".repeat(64);
    assert_eq!(
        fixture.verified_with(&wrong_output),
        Err(LicenseDecisionRefusal::DocumentationMismatch)
    );
    let mut wrong_text_path = review();
    wrong_text_path.documentation[1].license_text_path = "main.c".into();
    assert_eq!(
        fixture.verified_with(&wrong_text_path),
        Err(LicenseDecisionRefusal::DocumentationTextMissing)
    );
    let mut extra = review();
    extra.documentation.push(DocumentationReview {
        path: "share/extra".into(),
        license: "GFDL-1.3-or-later".into(),
        output_sha256: "f".repeat(64),
        license_text_path: "doc/fdl.texi".into(),
        license_text_sha256: "a7e43ea88ee8fc4722d38482fd1a14c12bfbe2f0512d61980c810627b19cdce5"
            .into(),
        obligations: vec!["notice".into(), "source".into()],
    });
    assert_eq!(
        fixture.verified_with(&extra),
        Err(LicenseDecisionRefusal::DocumentationMismatch)
    );
}

#[test]
fn a_copyleft_prior_deny_needs_evidence_but_source_available_nc_and_paid_stay_denied() {
    // D5: no boolean bypass. Copyleft has a real path; BUSL/NC/paid do not.
    assert!(
        reconcile_prior_deny(
            "AGPL-3.0-only",
            "AGPL-3.0-only",
            "AGPL-3.0-only",
            true,
            true,
            true,
            false
        )
        .is_ok()
    );
    assert_eq!(
        reconcile_prior_deny("BUSL-1.1", "BUSL-1.1", "BUSL-1.1", true, true, true, false),
        Err(LicenseDecisionRefusal::SoftwareDenied)
    );
    assert_eq!(
        reconcile_prior_deny(
            "CC-BY-NC-4.0",
            "CC-BY-NC-4.0",
            "CC-BY-NC-4.0",
            true,
            true,
            true,
            false
        ),
        Err(LicenseDecisionRefusal::SoftwareUnknown)
    );
    assert_eq!(
        reconcile_prior_deny(
            "AGPL-3.0-only",
            "AGPL-3.0-only",
            "AGPL-3.0-only",
            true,
            true,
            true,
            true
        ),
        Err(LicenseDecisionRefusal::PaymentRequired)
    );
    assert_eq!(
        reconcile_prior_deny(
            "AGPL-3.0-only",
            "AGPL-3.0-only",
            "AGPL-3.0-only",
            false,
            true,
            true,
            false
        ),
        Err(LicenseDecisionRefusal::SoftwareDenied)
    );
    assert!(
        reconcile_prior_deny(
            "(AGPL-3.0-only AND MIT)",
            "AGPL-3.0-only AND MIT",
            "(AGPL-3.0-only AND MIT)",
            true,
            true,
            true,
            false
        )
        .is_ok()
    );
}

#[test]
fn an_and_expression_keeps_every_conjunct_and_rejects_a_single_atom() {
    let fixture = Fixture::new();
    let mut asset = source_asset().clone();
    let mut identity = asset["identities"][0].clone();
    identity["name"] = serde_json::json!("ed-and");
    identity["version"] = serde_json::json!("1.21.1-and");
    identity["license"] = serde_json::json!("GPL-2.0-or-later AND MIT");
    identity
        .as_object_mut()
        .expect("identity object")
        .remove("documentationLicense");
    let license_evidence = serde_json::json!([
        {
            "path": "COPYING",
            "purpose": "license-text",
            "role": "license-text",
            "license": "GPL-2.0-only",
            "sha256": "022e9c2e9cc1aac3622313df6f971fbb39c1d1b0cb45324c16b8a1fe231c5ea6"
        },
        {
            "path": "ed.h",
            "purpose": "header",
            "role": "licensing",
            "license": "GPL-2.0-or-later",
            "sha256": "24f5c4349b47f7e8e8b35dd766d6f0309fca2fbfebb9ccd780788f8d37d51e58"
        },
        {
            "path": "MIT",
            "purpose": "license-text",
            "role": "license-text",
            "license": "MIT",
            "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        },
        {
            "path": "MIT.header",
            "purpose": "header",
            "role": "licensing",
            "license": "MIT",
            "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        }
    ]);
    identity["licenseEvidence"] = license_evidence.clone();
    identity["outputFiles"] = serde_json::json!([]);
    asset["evidence"]["licenseEvidenceCanonicalSha256"] =
        serde_json::json!(canonical_json_sha256(&license_evidence));
    asset["evidence"]["outputFilesCanonicalSha256"] =
        serde_json::json!(canonical_json_sha256(&serde_json::json!([])));
    asset["identities"]
        .as_array_mut()
        .expect("identities array")
        .push(identity);

    let mut and_review = review();
    and_review.name = "ed-and".into();
    and_review.version = "1.21.1-and".into();
    and_review.license_expression = "GPL-2.0-or-later AND MIT".into();
    and_review.selected_branch = "GPL-2.0-or-later AND MIT".into();
    and_review.software_obligations = vec!["notice".into(), "source".into()];
    and_review.software_texts = vec![
        SoftwareTextEvidence {
            path: "COPYING".into(),
            license: "GPL-2.0-only".into(),
            sha256: "022e9c2e9cc1aac3622313df6f971fbb39c1d1b0cb45324c16b8a1fe231c5ea6".into(),
        },
        SoftwareTextEvidence {
            path: "MIT".into(),
            license: "MIT".into(),
            sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        },
    ];
    and_review.documentation.clear();
    and_review.documentation_obligations.clear();
    let context = fixture.context();
    assert!(
        verify_against_source_asset(&and_review, &context, &asset).is_ok(),
        "the full AND selection keeps all conjuncts and obligations"
    );

    let mut atom_only = and_review.clone();
    atom_only.selected_branch = "MIT".into();
    atom_only.software_obligations = vec!["notice".into()];
    assert_eq!(
        verify_against_source_asset(&atom_only, &context, &asset),
        Err(LicenseDecisionRefusal::BranchNotInExpression)
    );

    let mut missing_mit_text = and_review.clone();
    missing_mit_text
        .software_texts
        .retain(|text| text.path == "COPYING");
    assert_eq!(
        verify_against_source_asset(&missing_mit_text, &context, &asset),
        Err(LicenseDecisionRefusal::LicenseTextDigest)
    );

    let mut missing_gpl_obligations = and_review.clone();
    missing_gpl_obligations.software_obligations = vec!["notice".into()];
    assert_eq!(
        verify_against_source_asset(&missing_gpl_obligations, &context, &asset),
        Err(LicenseDecisionRefusal::ObligationsMismatch)
    );

    let mut missing_mit_licensing = asset.clone();
    missing_mit_licensing["identities"]
        .as_array_mut()
        .expect("identities")
        .last_mut()
        .expect("synthetic identity")["licenseEvidence"] = serde_json::json!([
        {
            "path": "COPYING",
            "purpose": "license-text",
            "role": "license-text",
            "license": "GPL-2.0-only",
            "sha256": "022e9c2e9cc1aac3622313df6f971fbb39c1d1b0cb45324c16b8a1fe231c5ea6"
        },
        {
            "path": "ed.h",
            "purpose": "header",
            "role": "licensing",
            "license": "GPL-2.0-or-later",
            "sha256": "24f5c4349b47f7e8e8b35dd766d6f0309fca2fbfebb9ccd780788f8d37d51e58"
        },
        {
            "path": "MIT",
            "purpose": "license-text",
            "role": "license-text",
            "license": "MIT",
            "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        }
    ]);
    let missing_licensing_evidence = missing_mit_licensing["identities"]
        .as_array()
        .expect("identities")
        .last()
        .expect("identity")["licenseEvidence"]
        .clone();
    missing_mit_licensing["evidence"]["licenseEvidenceCanonicalSha256"] =
        serde_json::json!(canonical_json_sha256(&missing_licensing_evidence));
    assert_eq!(
        verify_against_source_asset(&and_review, &context, &missing_mit_licensing),
        Err(LicenseDecisionRefusal::MissingLicensingFact)
    );
}

#[test]
fn the_binding_is_a_tagged_real_stage_and_config_hashes_cannot_stand_in() {
    // D1: a project-config hash is not a spec/inventory/plan.
    let fixture = Fixture::new();
    let forged = PolicyBinding::test_binding(
        &"e".repeat(64),
        1,
        None,
        INVENTORY,
        EVIDENCE_BUNDLE,
        CandidateBinding::base(BASE).unwrap(),
        &identity_facts_receipt_digest(fixture.facts.verified()),
    );
    let context = LicenseDecisionContext {
        project_id: "project-1",
        lead_request_id: "lead-1",
        principal: "operator",
        request_sha256: REQUEST_SHA,
        project_config_hash: PROJECT_CONFIG,
        base_commit: BASE,
        reservation: &fixture.reservation,
        delegation: &fixture.delegation,
        delegation_revoked: false,
        identity_facts: &fixture.facts,
        source_evidence: &fixture.source,
        binding: &forged,
    };
    assert_eq!(
        verify_license_evidence_review(&review(), &context),
        Err(LicenseDecisionRefusal::PolicyBindingForged)
    );

    // A hashed base commit is never a candidate.
    let hashed_base = sha256_hex(BASE.as_bytes());
    let binding = PolicyBinding::test_binding(
        PLAN,
        1,
        None,
        INVENTORY,
        EVIDENCE_BUNDLE,
        CandidateBinding::candidate(hashed_base).unwrap(),
        &identity_facts_receipt_digest(fixture.facts.verified()),
    );
    let context = LicenseDecisionContext {
        binding: &binding,
        ..context
    };
    assert_eq!(
        verify_license_evidence_review(&review(), &context),
        Err(LicenseDecisionRefusal::CandidateUnverified)
    );

    // Renaming the stage to a runtime spec with a foreign plan is refused.
    let fixture = Fixture::new();
    let binding = PolicyBinding::test_binding(
        PLAN,
        1,
        Some(&"9".repeat(64)),
        INVENTORY,
        EVIDENCE_BUNDLE,
        CandidateBinding::base(BASE).unwrap(),
        &identity_facts_receipt_digest(fixture.facts.verified()),
    );
    let context = LicenseDecisionContext {
        project_id: "project-1",
        lead_request_id: "lead-1",
        principal: "operator",
        request_sha256: REQUEST_SHA,
        project_config_hash: PROJECT_CONFIG,
        base_commit: BASE,
        reservation: &fixture.reservation,
        delegation: &fixture.delegation,
        delegation_revoked: false,
        identity_facts: &fixture.facts,
        source_evidence: &fixture.source,
        binding: &binding,
    };
    assert_eq!(
        verify_license_evidence_review(&review(), &context),
        Err(LicenseDecisionRefusal::RootBindingMismatch)
    );
}

#[test]
fn text_digests_request_response_and_receipt_are_tamper_evident() {
    let fixture = Fixture::new();
    let mut text = review();
    text.software_text_sha256 = "f".repeat(64);
    assert_eq!(
        fixture.verified_with(&text),
        Err(LicenseDecisionRefusal::LicenseTextDigest)
    );
    let mut foreign_text = review();
    // The GFDL documentation text is real and trusted, but it is not the
    // software licence text for GPL-2.0-or-later.
    foreign_text.software_text_sha256 =
        "a7e43ea88ee8fc4722d38482fd1a14c12bfbe2f0512d61980c810627b19cdce5".into();
    assert_eq!(
        fixture.verified_with(&foreign_text),
        Err(LicenseDecisionRefusal::LicenseTextDigest)
    );
    let mut request = review();
    request.request_sha256 = "b".repeat(64);
    assert_eq!(
        fixture.verified_with(&request),
        Err(LicenseDecisionRefusal::RequestBinding)
    );
    let mut response = review();
    response.response_sha256 = "b".repeat(64);
    assert_eq!(
        fixture.verified_with(&response),
        Err(LicenseDecisionRefusal::ResponseBinding)
    );
    let foreign = verified("sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
    let mut tampered = Fixture::new();
    tampered.facts = BoundIdentityFacts::spec_cache_observation(foreign);
    // Rebuild the binding for the foreign receipt: the digest now matches, so
    // the refusal must come from exact output/source fact membership.
    tampered.binding = binding(&tampered.facts);
    assert_eq!(
        tampered.verified_with(&review()),
        Err(LicenseDecisionRefusal::ReceiptMismatch)
    );

    let mut bad_signature = source_proof();
    bad_signature.signature = "cache.nixos.org-1:AAAA".into();
    assert!(VerifiedSourceEvidence::verify(&bad_signature).is_err());
    let mut bad_size = source_proof();
    bad_size.nar_size += 1;
    assert!(VerifiedSourceEvidence::verify(&bad_size).is_err());
    let mut bad_nix32 = source_proof();
    bad_nix32.nar_nix32 = Some("f".repeat(52));
    assert!(VerifiedSourceEvidence::verify(&bad_nix32).is_err());
    let mut missing_input = source_proof();
    missing_input.output_drv_input_drvs.clear();
    assert!(VerifiedSourceEvidence::verify(&missing_input).is_err());
    let mut unrelated_input = source_proof();
    unrelated_input.output_drv_input_drvs =
        vec!["/nix/store/05q48dcd4lgk4vh7wyk330gr2fr082i2-bootstrap-tools.drv".into()];
    assert!(
        VerifiedSourceEvidence::verify(&unrelated_input).is_err(),
        "the real output drv input list must contain the exact source drv"
    );

    // A flat hash is not covered by the cache fingerprint signature; it must
    // still match the shared asset before it can be used. An asserted CA that
    // disagrees with the flat hash is refused outright.
    let mut bad_ca = source_proof();
    bad_ca.ca = Some("fixed:sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into());
    assert!(VerifiedSourceEvidence::verify(&bad_ca).is_err());
    let mut bad_flat_proof = source_proof();
    bad_flat_proof.ca = None;
    bad_flat_proof.flat_sha256 = "f".repeat(64);
    let bad_flat = VerifiedSourceEvidence::verify(&bad_flat_proof).expect("signature covers NAR");
    let mut bad_flat_fixture = Fixture::new();
    bad_flat_fixture.source = bad_flat;
    assert_eq!(
        bad_flat_fixture.verified_with(&review()),
        Err(LicenseDecisionRefusal::SourceEvidenceMismatch)
    );

    let plan_root = BoundIdentityFacts::spec_cache_observation(verified_with_root(PLAN_ROOT));
    let mut wide = Fixture::new();
    wide.facts = plan_root;
    wide.binding = binding(&wide.facts);
    assert!(
        wide.verified_with(&review()).is_ok(),
        "a plan-wide receipt with a legitimate plan observed root must pass exact membership"
    );
}

fn verified_with_root(root: &str) -> VerifiedIdentityFacts {
    let challenge = challenge(OUTPUT_NAR);
    let facts = facts(OUTPUT_NAR);
    let envelope = IdentityFactsEnvelope {
        schema_version: 1,
        context: IDENTITY_FACTS_CONTEXT.into(),
        challenge: challenge.clone(),
        manifest: IdentityFactsManifest {
            observed_root: root.into(),
            fact_count: facts.len(),
            closure_manifest_sha256: closure_manifest_sha256(&facts),
            facts,
            truncated: false,
        },
    };
    let signature = sign(&envelope, BEARER).expect("the fixture receipt signs");
    let receipt = IdentityFactsReceipt {
        envelope,
        signature,
    };
    verify(&receipt, BEARER, &challenge).expect("the fixture receipt verifies")
}

#[test]
fn canonical_json_is_sorted_compact_and_lf_terminated() {
    let left: serde_json::Value =
        serde_json::from_str(r#"{"b":1,"a":{"d":2,"c":3},"z":[true,null]}"#).unwrap();
    let right: serde_json::Value =
        serde_json::from_str(r#"{"z":[true,null],"a":{"c":3,"d":2},"b":1}"#).unwrap();
    assert_eq!(canonical_json_sha256(&left), canonical_json_sha256(&right));
    let mut canonical = String::new();
    write_canonical(&left, &mut canonical);
    canonical.push('\n');
    assert_eq!(
        canonical,
        "{\"a\":{\"c\":3,\"d\":2},\"b\":1,\"z\":[true,null]}\n"
    );
    assert_eq!(
        canonical_json_sha256(&left),
        sha256_hex(canonical.as_bytes())
    );
}

#[test]
fn actor_delegation_project_and_terminal_states_fail_closed() {
    let fixture = Fixture::new();
    // Delegation revocation is durable state, not a request boolean.
    let revoked = Fixture::new();
    let context = LicenseDecisionContext {
        project_id: "project-1",
        lead_request_id: "lead-1",
        principal: "operator",
        request_sha256: REQUEST_SHA,
        project_config_hash: PROJECT_CONFIG,
        base_commit: BASE,
        reservation: &revoked.reservation,
        delegation: &revoked.delegation,
        delegation_revoked: true,
        identity_facts: &revoked.facts,
        source_evidence: &revoked.source,
        binding: &revoked.binding,
    };
    assert!(verify_license_evidence_review(&review(), &context).is_err());

    let context = LicenseDecisionContext {
        project_id: "project-2",
        lead_request_id: "lead-1",
        principal: "operator",
        request_sha256: REQUEST_SHA,
        project_config_hash: PROJECT_CONFIG,
        base_commit: BASE,
        reservation: &fixture.reservation,
        delegation: &fixture.delegation,
        delegation_revoked: false,
        identity_facts: &fixture.facts,
        source_evidence: &fixture.source,
        binding: &fixture.binding,
    };
    assert_eq!(
        verify_license_evidence_review(&review(), &context),
        Err(LicenseDecisionRefusal::ProjectMismatch)
    );

    let mut terminal = Fixture::new();
    terminal.reservation.lead_state = "FAILED".into();
    assert_eq!(
        terminal.verified_with(&review()),
        Err(LicenseDecisionRefusal::LeadTerminal)
    );

    let mut wrong_principal = Fixture::new();
    wrong_principal.reservation.principal_id = "someone-else".into();
    assert_eq!(
        wrong_principal.verified_with(&review()),
        Err(LicenseDecisionRefusal::PrincipalMismatch)
    );

    let mut expired = Fixture::new();
    expired.reservation.expires_at_ms = chrono::Utc::now().timestamp_millis() - 1;
    assert_eq!(
        expired.verified_with(&review()),
        Err(LicenseDecisionRefusal::Actor(
            "the licence evidence reservation has expired".into()
        ))
    );

    let mut expired_delegation = Fixture::new();
    expired_delegation.delegation.deadline =
        (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
    assert!(expired_delegation.verified_with(&review()).is_err());
}

#[test]
fn needs_evidence_or_deny_cannot_be_an_accidental_acceptance() {
    let fixture = Fixture::new();
    let mut needs = review();
    needs.decision = "NEEDS_EVIDENCE".into();
    assert_eq!(
        fixture.verified_with(&needs),
        Err(LicenseDecisionRefusal::DecisionMismatch)
    );
    let mut deny = review();
    deny.decision = "DENY".into();
    assert!(fixture.verified_with(&deny).is_ok());
    let mut not_reviewed = review();
    not_reviewed.obligations_reviewed = false;
    assert_eq!(
        fixture.verified_with(&not_reviewed),
        Err(LicenseDecisionRefusal::ObligationsMismatch)
    );
}

#[test]
fn a_stored_receipt_is_reverified_with_the_broker_bearer() {
    let receipt = receipt(OUTPUT_NAR);
    let stored = crate::db::StoredIdentityFactsReceipt {
        nonce: receipt.envelope.challenge.nonce.clone(),
        operation_id: receipt.envelope.challenge.operation_id.clone(),
        runtime_id: receipt.envelope.challenge.runtime_id.clone(),
        plan_sha256: PLAN.into(),
        spec_sha256: OBSERVED_SPEC.into(),
        pin: PIN.into(),
        observed_root: OUTPUT_PATH.into(),
        root_binding: IdentityFactsRootBinding::SpecCacheObservation,
        stage: crate::planning::identity_facts::IdentityFactsStage::Spec,
        resolution_key: String::new(),
        fact_count: facts(OUTPUT_NAR).len() as u32,
        closure_manifest_sha256: receipt.envelope.manifest.closure_manifest_sha256.clone(),
        manifest_json: serde_json::to_string(&receipt.envelope.manifest).unwrap(),
        envelope_json: serde_json::to_string(&receipt.envelope).unwrap(),
        signature: receipt.signature.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let parsed = IdentityFactsReceipt {
        envelope: serde_json::from_str(&stored.envelope_json).unwrap(),
        signature: stored.signature.clone(),
    };
    assert_eq!(parsed.envelope.context, IDENTITY_FACTS_CONTEXT);
    assert_eq!(parsed.envelope.challenge.plan_sha256, stored.plan_sha256);
    assert_eq!(parsed.envelope.challenge.spec_sha256, stored.spec_sha256);
    assert_eq!(parsed.envelope.challenge.pin, stored.pin);
    assert_eq!(parsed.envelope.manifest.observed_root, stored.observed_root);
    let mut stale_stage = stored.clone();
    stale_stage.stage = crate::planning::identity_facts::IdentityFactsStage::Resolution;
    assert_eq!(
        verify_stored_identity_facts(&stale_stage, BEARER),
        Err(LicenseDecisionRefusal::ReceiptMismatch)
    );
    let mut stale_key = stored.clone();
    stale_key.resolution_key = "f".repeat(64);
    assert_eq!(
        verify_stored_identity_facts(&stale_key, BEARER),
        Err(LicenseDecisionRefusal::ReceiptMismatch)
    );
    assert_eq!(
        parsed.envelope.manifest.fact_count,
        stored.fact_count as usize
    );
    assert_eq!(
        parsed.envelope.manifest.closure_manifest_sha256,
        stored.closure_manifest_sha256
    );
    let verified = verify_stored_identity_facts(&stored, BEARER);
    assert!(verified.is_ok(), "{:?}", verified.err());
    assert_eq!(
        verify_stored_identity_facts(&stored, b"another-bearer").unwrap_err(),
        LicenseDecisionRefusal::ReceiptMismatch
    );
    let mut tampered = stored;
    tampered.plan_sha256 = "f".repeat(64);
    assert_eq!(
        verify_stored_identity_facts(&tampered, BEARER).unwrap_err(),
        LicenseDecisionRefusal::ReceiptMismatch
    );
}

fn stored_plan(plan: &EnvironmentPlan) -> StoredEnvironmentPlan {
    StoredEnvironmentPlan {
        row_id: "plan-row".into(),
        revision: plan.revision,
        status: "CORE_VALIDATED".into(),
        plan_sha256: plan.plan_sha256.clone(),
        content_json: serde_json::to_string(plan).expect("plan serializes"),
        created_at: "2026-11-20T19:24:23Z".into(),
        updated_at: "2026-11-20T19:24:23Z".into(),
        plan: plan.clone(),
    }
}

fn challenge_for(plan_sha: &str, spec_sha: &str, nar_hash: &str) -> IdentityFactsChallenge {
    IdentityFactsChallenge {
        operation_id: "operation-1".into(),
        runtime_id: "runtime-1".into(),
        nonce: "identity-facts:test".into(),
        pin: PIN.into(),
        plan_sha256: plan_sha.into(),
        spec_sha256: spec_sha.into(),
        mode: IdentityFactsMode::Questions,
        root: OperationRoot::Operation,
        questions: vec![
            crate::planning::identity_facts::IdentityQuestion {
                store_path: OUTPUT_PATH.into(),
                deriver: Some(OUTPUT_DRV.into()),
                nar_hash: nar_hash.into(),
                outputs: vec![OUTPUT_PATH.into()],
            },
            crate::planning::identity_facts::IdentityQuestion {
                store_path: SOURCE_PATH.into(),
                deriver: Some(SOURCE_DRV.into()),
                nar_hash: SOURCE_NAR.into(),
                outputs: vec![SOURCE_PATH.into()],
            },
        ],
        stage: crate::planning::identity_facts::IdentityFactsStage::Spec,
        resolution_key: String::new(),
    }
}

fn verified_resolution(plan_sha: &str, resolution_key: &str) -> VerifiedIdentityFacts {
    let mut challenge = challenge_for(plan_sha, "", OUTPUT_NAR);
    challenge.stage = crate::planning::identity_facts::IdentityFactsStage::Resolution;
    challenge.spec_sha256 = String::new();
    challenge.resolution_key = resolution_key.into();
    let facts = facts(OUTPUT_NAR);
    let envelope = IdentityFactsEnvelope {
        schema_version: 1,
        context: IDENTITY_FACTS_CONTEXT.into(),
        challenge,
        manifest: IdentityFactsManifest {
            observed_root: OUTPUT_PATH.into(),
            fact_count: facts.len(),
            closure_manifest_sha256: closure_manifest_sha256(&facts),
            facts,
            truncated: false,
        },
    };
    let envelope_signature = sign(&envelope, BEARER).expect("the fixture receipt signs");
    let receipt = IdentityFactsReceipt {
        envelope,
        signature: envelope_signature,
    };
    verify(&receipt, BEARER, &receipt.envelope.challenge).expect("the resolution receipt verifies")
}

fn verified_for(
    plan_sha: &str,
    spec_sha: &str,
    nar_hash: &str,
    observed_root: &str,
) -> VerifiedIdentityFacts {
    let challenge = challenge_for(plan_sha, spec_sha, nar_hash);
    let facts = facts(nar_hash);
    let envelope = IdentityFactsEnvelope {
        schema_version: 1,
        context: IDENTITY_FACTS_CONTEXT.into(),
        challenge: challenge.clone(),
        manifest: IdentityFactsManifest {
            observed_root: observed_root.into(),
            fact_count: facts.len(),
            closure_manifest_sha256: closure_manifest_sha256(&facts),
            facts,
            truncated: false,
        },
    };
    let signature = sign(&envelope, BEARER).expect("the fixture receipt signs");
    let receipt = IdentityFactsReceipt {
        envelope,
        signature,
    };
    verify(&receipt, BEARER, &challenge).expect("the fixture receipt verifies")
}

#[test]
fn real_plan_and_frozen_spec_build_the_typed_binding() {
    let retained: EnvironmentPlan =
        serde_json::from_str(include_str!("fixtures/retained-plan.json")).expect("real plan");
    let retained_evidence = InventoryEvidenceBundle::from_plan(&retained);
    let retained_facts = BoundIdentityFacts::spec_cache_observation(verified_for(
        &retained.plan_sha256,
        OBSERVED_SPEC,
        OUTPUT_NAR,
        OUTPUT_PATH,
    ));
    let retained_binding = PolicyBinding::from_plan_inventory_evidence(
        default_catalog_sha256(),
        default_profile_sha256(),
        default_binding_sha256(),
        &stored_plan(&retained),
        &retained_evidence,
        CandidateBinding::base(retained.base_commit.clone()).expect("base"),
        &retained_facts,
    );
    // The retained plan was compiled under an older catalogue: it is STALE under
    // the current policy and must not be silently accepted.
    assert_eq!(
        retained_binding,
        Err(LicenseDecisionRefusal::PolicyMismatch)
    );

    let mut plan = retained;
    plan.license_inventory.catalog_sha256 = default_catalog_sha256().into();
    plan.license_inventory.profile_sha256 = default_profile_sha256().into();
    plan.license_inventory.inventory_sha256 = plan.license_inventory.compute_sha256(&plan.plan_id);
    plan.license_inventory_sha256 = plan.license_inventory.inventory_sha256.clone();
    plan.plan_sha256 = plan.compute_sha256();
    let evidence = InventoryEvidenceBundle::from_plan(&plan);
    let facts = BoundIdentityFacts::spec_cache_observation(verified_for(
        &plan.plan_sha256,
        OBSERVED_SPEC,
        OUTPUT_NAR,
        OUTPUT_PATH,
    ));
    let binding = PolicyBinding::from_plan_inventory_evidence(
        default_catalog_sha256(),
        default_profile_sha256(),
        default_binding_sha256(),
        &stored_plan(&plan),
        &evidence,
        CandidateBinding::base(plan.base_commit.clone()).expect("base"),
        &facts,
    )
    .expect("current real plan binds");
    assert_eq!(binding.stage().plan_sha256(), plan.plan_sha256);
    assert_eq!(
        binding.stage().inventory_sha256(),
        plan.license_inventory_sha256
    );
    assert_eq!(
        binding.identity_facts_receipt_digest(),
        identity_facts_receipt_digest(facts.verified())
    );
    let resolution_key = "d".repeat(64);
    let resolution_facts = BoundIdentityFacts::spec_cache_observation(verified_resolution(
        &plan.plan_sha256,
        &resolution_key,
    ));
    let resolution_binding = PolicyBinding::from_plan_inventory_evidence(
        default_catalog_sha256(),
        default_profile_sha256(),
        default_binding_sha256(),
        &stored_plan(&plan),
        &evidence,
        CandidateBinding::base(plan.base_commit.clone()).expect("base"),
        &resolution_facts,
    )
    .expect("a failed-realisation observation binds the plan stage");
    assert_eq!(
        resolution_binding.stage().resolution_key(),
        Some(resolution_key.as_str())
    );
    assert!(
        resolution_binding
            .stage_json()
            .contains("\"resolutionKey\"")
    );

    let wrong_evidence = InventoryEvidenceBundle { entries: vec![] };
    assert_eq!(
        PolicyBinding::from_plan_inventory_evidence(
            default_catalog_sha256(),
            default_profile_sha256(),
            default_binding_sha256(),
            &stored_plan(&plan),
            &wrong_evidence,
            CandidateBinding::base(plan.base_commit.clone()).expect("base"),
            &facts,
        ),
        Err(LicenseDecisionRefusal::EvidenceBundleMismatch)
    );
    assert_eq!(
        PolicyBinding::from_plan_inventory_evidence(
            default_catalog_sha256(),
            default_profile_sha256(),
            default_binding_sha256(),
            &stored_plan(&plan),
            &evidence,
            CandidateBinding::base("a".repeat(40)).expect("base"),
            &facts,
        ),
        Err(LicenseDecisionRefusal::PolicyBindingIncomplete)
    );

    let spec_sha256 = "d".repeat(64);
    let spec = EnvironmentSpec {
        schema_version: 1,
        spec_id: "spec-1".into(),
        spec_sha256: spec_sha256.clone(),
        project_id: plan.project_id.clone(),
        request_id: plan.request_id.clone(),
        action_id: plan.action_id.clone(),
        campaign_id: None,
        base_commit: plan.base_commit.clone(),
        plan_id: plan.plan_id.clone(),
        plan_sha256: plan.plan_sha256.clone(),
        conventions_sha256: plan.conventions_sha256.clone(),
        review_id: "review-1".into(),
        platform: "x86_64-linux".into(),
        nix_version: "2.24.0".into(),
        devenv_version: "1.0.0".into(),
        flake_lock_sha256: String::new(),
        devenv_lock_sha256: String::new(),
        license_inventory_sha256: plan.license_inventory_sha256.clone(),
        sbom_sha256: plan.license_inventory.sbom_sha256.clone(),
        builder_image: plan.builder.image.clone(),
        builder_image_digest: plan.builder.digest.clone(),
        resource_policy: plan.resources.clone(),
        network_policy: plan.network.clone(),
        status: crate::planning::environment::EnvironmentSpecStatus::Frozen,
        frozen_at: "2026-11-20T20:00:00Z".into(),
    };
    let stored_spec = StoredEnvironmentSpec {
        row_id: "spec-row".into(),
        spec_sha256: spec.spec_sha256.clone(),
        content_json: serde_json::to_string(&spec).expect("spec serializes"),
        frozen_at: spec.frozen_at.clone(),
        created_at: "2026-11-20T20:00:00Z".into(),
        spec,
    };
    let spec_verified = verified_for(&plan.plan_sha256, &spec_sha256, OUTPUT_NAR, OUTPUT_PATH);
    let build_receipt = EnvironmentBuildReceipt {
        schema_version: 1,
        receipt_id: "receipt-1".into(),
        spec_sha256: spec_sha256.clone(),
        platform: stored_spec.spec.platform.clone(),
        builder_image: stored_spec.spec.builder_image.clone(),
        builder_image_digest: stored_spec.spec.builder_image_digest.clone(),
        builder_policy_revision: "nix-devenv-environment-v1".into(),
        status: EnvironmentBuilderStatus::Succeeded,
        store_path: OUTPUT_PATH.into(),
        output_sha256: "e".repeat(64),
        sbom_sha256: stored_spec.spec.sbom_sha256.clone(),
        log_ref: "log".into(),
        sandbox_attestation: SandboxAttestation {
            non_root: true,
            no_secrets: true,
            no_host_network: true,
            no_host_socket: true,
            no_host_mount: true,
            sandbox: true,
            impure: false,
            bounded_resources: true,
        },
        blockers: vec![],
        duration_ms: 1,
        created_at: "2026-11-20T20:00:01Z".into(),
    };
    let spec_facts =
        BoundIdentityFacts::bind_build_receipt(spec_verified, &build_receipt, &stored_spec.spec)
            .expect("complete build receipt binds");
    let spec_binding = PolicyBinding::from_spec_inventory_evidence(
        default_catalog_sha256(),
        default_profile_sha256(),
        default_binding_sha256(),
        &stored_plan(&plan),
        &stored_spec,
        &evidence,
        CandidateBinding::base(plan.base_commit.clone()).expect("base"),
        &spec_facts,
    )
    .expect("frozen spec binds");
    assert_eq!(
        spec_binding.stage().spec_sha256(),
        Some(spec_sha256.as_str())
    );
}

#[test]
fn an_old_documentation_row_without_output_association_is_not_defaulted() {
    let old = serde_json::json!([{
        "path": "share/doc/old",
        "license": "MIT",
        "licenseTextSha256": "a".repeat(64),
        "obligations": ["notice"],
        "verdict": "ALLOWED"
    }]);
    assert!(
        serde_json::from_value::<Vec<super::DocumentationDecision>>(old).is_err(),
        "missing outputSha256/licenseTextPath must be read as incomplete, never defaulted to an authorising empty value"
    );
}

fn verified_output_only() -> VerifiedIdentityFacts {
    let challenge = IdentityFactsChallenge {
        operation_id: "operation-1".into(),
        runtime_id: "runtime-1".into(),
        nonce: "identity-facts:test".into(),
        pin: PIN.into(),
        plan_sha256: PLAN.into(),
        spec_sha256: OBSERVED_SPEC.into(),
        mode: IdentityFactsMode::Questions,
        root: OperationRoot::Operation,
        questions: vec![crate::planning::identity_facts::IdentityQuestion {
            store_path: OUTPUT_PATH.into(),
            deriver: Some(OUTPUT_DRV.into()),
            nar_hash: OUTPUT_NAR.into(),
            outputs: vec![OUTPUT_PATH.into()],
        }],
        stage: crate::planning::identity_facts::IdentityFactsStage::Spec,
        resolution_key: String::new(),
    };
    let facts = vec![IdentityFact {
        store_path: OUTPUT_PATH.into(),
        deriver: Some(OUTPUT_DRV.into()),
        nar_hash: OUTPUT_NAR.into(),
        outputs: vec![OUTPUT_PATH.into()],
    }];
    let envelope = IdentityFactsEnvelope {
        schema_version: 1,
        context: IDENTITY_FACTS_CONTEXT.into(),
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
    let receipt = IdentityFactsReceipt {
        envelope,
        signature,
    };
    verify(&receipt, BEARER, &challenge).expect("the fixture receipt verifies")
}

#[test]
fn a_plan_wide_receipt_without_a_source_fact_uses_the_separate_source_proof() {
    let mut fixture = Fixture::new();
    fixture.facts = BoundIdentityFacts::spec_cache_observation(verified_output_only());
    fixture.binding = binding(&fixture.facts);
    assert!(
        fixture.verified_with(&review()).is_ok(),
        "the source is proven by the separate fixed-output proof, not by inventing a manifest fact"
    );
}

#[test]
fn the_shared_source_proof_vector_is_the_single_canonical_representation() {
    let vector: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../deploy/environment-builder/environment_builder/source_evidence_proof_vector.json"
    ))
    .expect("the shared source-proof vector is valid JSON");
    assert_eq!(vector["schemaVersion"], 1);
    let proof = vector["sourceProof"].clone();
    let verified = VerifiedSourceEvidence::from_verified_json(&proof)
        .expect("the shared proof verifies and round-trips exactly");

    // One canonical representation, byte-identical to the builder's vector.
    assert_eq!(verified.to_json(), proof);
    let digest = vector["canonicalSha256"]
        .as_str()
        .expect("the vector carries its digest");
    assert_eq!(verified.canonical_sha256(), digest);
    assert_eq!(canonical_json_sha256(&verified.to_json()), digest);
    assert_eq!(
        crate::planning::lead_classification::sha256_hex(
            vector["canonicalJson"]
                .as_str()
                .expect("the vector carries canonical bytes")
                .as_bytes()
        ),
        digest
    );
    assert_eq!(
        verified.output_drv_input_drvs().len(),
        5,
        "the real output drv inputs are retained, not synthesised"
    );
    assert!(
        verified
            .output_drv_input_drvs()
            .contains(&verified.source_drv_path().to_owned())
    );
    assert!(
        !verified.to_json().to_string().contains("signatureValid"),
        "a derived proof carries no caller validity boolean"
    );
    assert!(
        !verified
            .to_json()
            .to_string()
            .contains("sourceInOutputInputDrvs"),
        "the input-membership fact is verified, not restated as a boolean"
    );
}
