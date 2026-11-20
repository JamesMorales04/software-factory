//! Core-verified exact licence decision consumer.
//!
//! A model may propose a decision, but this module validates the exact identity,
//! source evidence, identity-facts receipt, Lead actor, Owner delegation, stage
//! binding, selected branch and software/documentation obligations against the
//! shared evidence and the current policy. The resulting
//! [`VerifiedLicenseDecision`] is the only value the durable decision writer
//! accepts.
//!
//! The module is deliberately fail-closed:
//!
//! * the licence expression must equal the identity licensing fact;
//! * the selected branch must be an atom of the expression AST (including a
//!   `WITH` exception);
//! * software and documentation obligations are exact, per context;
//! * a denied copyleft licence can be reconciled by real evidence (trusted text
//!   digest, exact licensing fact, commercial/no-payment predicate and exact
//!   obligations), never by a caller-supplied boolean;
//! * the binding is a tagged real artefact (plan draft or runtime spec), never a
//!   project-config hash disguised as a spec;
//! * the identity-facts receipt is HMAC-verified and bound to the stage.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use super::identity_facts::{
    self, IDENTITY_FACTS_CONTEXT, IdentityFactsReceipt, VerifiedIdentityFacts,
};
use super::lead_classification::{
    ClassificationRefusal, VerifiedLicenseMandate, is_sha256, sha256_hex,
};
use super::license_policy::{
    CatalogEntry, LicenseContext, LicenseDecision, default_binding_sha256, default_catalog,
    default_catalog_sha256, default_profile, default_profile_sha256, evaluate,
};
use serde::{Deserialize, Serialize};

pub const VERSIONED_SOURCE_EVIDENCE_JSON: &str = include_str!(
    "../../../deploy/environment-builder/environment_builder/versioned_source_evidence.json"
);

/// The candidate Core executed, or the raw existing base commit.
///
/// A base commit is never hashed and called a candidate; a candidate digest is
/// a real frozen artefact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CandidateBinding {
    Base { base_commit: String },
    Candidate { candidate_sha256: String },
}

impl CandidateBinding {
    pub fn base(base_commit: impl Into<String>) -> Result<Self, LicenseDecisionRefusal> {
        let base_commit = base_commit.into();
        if !is_commit(&base_commit) {
            return Err(LicenseDecisionRefusal::PolicyBindingIncomplete);
        }
        Ok(Self::Base { base_commit })
    }

    pub fn candidate(candidate_sha256: impl Into<String>) -> Result<Self, LicenseDecisionRefusal> {
        let candidate_sha256 = candidate_sha256.into();
        if !is_sha256(&candidate_sha256) {
            return Err(LicenseDecisionRefusal::PolicyBindingIncomplete);
        }
        Ok(Self::Candidate { candidate_sha256 })
    }

    pub fn base_commit(&self) -> Option<&str> {
        match self {
            Self::Base { base_commit } => Some(base_commit),
            Self::Candidate { .. } => None,
        }
    }

    pub fn candidate_sha256(&self) -> Option<&str> {
        match self {
            Self::Base { .. } => None,
            Self::Candidate { candidate_sha256 } => Some(candidate_sha256),
        }
    }

    fn validate(&self) -> Result<(), LicenseDecisionRefusal> {
        match self {
            Self::Base { base_commit } if is_commit(base_commit) => Ok(()),
            Self::Candidate { candidate_sha256 } if is_sha256(candidate_sha256) => Ok(()),
            _ => Err(LicenseDecisionRefusal::PolicyBindingIncomplete),
        }
    }
}

/// The exact real stage of the environment at decision time.
///
/// Before the environment spec exists, the only real artefacts are the retained
/// plan, its real licence inventory and the evidence snapshot. A plan hash is
/// never presented as a spec hash, and a spec-required consumer stays BLOCKED
/// until a real spec row exists. When the spec materialises the decision may be
/// REBOUND to the same verdict only if plan/inventory/evidence/policy/receipt
/// are unchanged; any change is STALE.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "stage",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum StageBinding {
    PlanInventoryEvidence {
        plan_sha256: String,
        plan_revision: u32,
        inventory_sha256: String,
        evidence_bundle_sha256: String,
        identity_facts_receipt_digest: String,
        candidate: CandidateBinding,
    },
    SpecInventoryEvidence {
        plan_sha256: String,
        plan_revision: u32,
        spec_sha256: String,
        inventory_sha256: String,
        evidence_bundle_sha256: String,
        identity_facts_receipt_digest: String,
        candidate: CandidateBinding,
    },
}

impl StageBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn plan_inventory_evidence(
        plan_sha256: impl Into<String>,
        plan_revision: u32,
        inventory_sha256: impl Into<String>,
        evidence_bundle_sha256: impl Into<String>,
        identity_facts_receipt_digest: impl Into<String>,
        candidate: CandidateBinding,
    ) -> Result<Self, LicenseDecisionRefusal> {
        let stage = Self::PlanInventoryEvidence {
            plan_sha256: plan_sha256.into(),
            plan_revision,
            inventory_sha256: inventory_sha256.into(),
            evidence_bundle_sha256: evidence_bundle_sha256.into(),
            identity_facts_receipt_digest: identity_facts_receipt_digest.into(),
            candidate,
        };
        stage.validate()?;
        Ok(stage)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn spec_inventory_evidence(
        plan_sha256: impl Into<String>,
        plan_revision: u32,
        spec_sha256: impl Into<String>,
        inventory_sha256: impl Into<String>,
        evidence_bundle_sha256: impl Into<String>,
        identity_facts_receipt_digest: impl Into<String>,
        candidate: CandidateBinding,
    ) -> Result<Self, LicenseDecisionRefusal> {
        let stage = Self::SpecInventoryEvidence {
            plan_sha256: plan_sha256.into(),
            plan_revision,
            spec_sha256: spec_sha256.into(),
            inventory_sha256: inventory_sha256.into(),
            evidence_bundle_sha256: evidence_bundle_sha256.into(),
            identity_facts_receipt_digest: identity_facts_receipt_digest.into(),
            candidate,
        };
        stage.validate()?;
        Ok(stage)
    }

    pub fn plan_sha256(&self) -> &str {
        match self {
            Self::PlanInventoryEvidence { plan_sha256, .. }
            | Self::SpecInventoryEvidence { plan_sha256, .. } => plan_sha256,
        }
    }

    pub fn plan_revision(&self) -> u32 {
        match self {
            Self::PlanInventoryEvidence { plan_revision, .. }
            | Self::SpecInventoryEvidence { plan_revision, .. } => *plan_revision,
        }
    }

    pub fn inventory_sha256(&self) -> &str {
        match self {
            Self::PlanInventoryEvidence {
                inventory_sha256, ..
            }
            | Self::SpecInventoryEvidence {
                inventory_sha256, ..
            } => inventory_sha256,
        }
    }

    pub fn spec_sha256(&self) -> Option<&str> {
        match self {
            Self::SpecInventoryEvidence { spec_sha256, .. } => Some(spec_sha256),
            Self::PlanInventoryEvidence { .. } => None,
        }
    }

    pub fn evidence_bundle_sha256(&self) -> &str {
        match self {
            Self::PlanInventoryEvidence {
                evidence_bundle_sha256,
                ..
            }
            | Self::SpecInventoryEvidence {
                evidence_bundle_sha256,
                ..
            } => evidence_bundle_sha256,
        }
    }

    pub fn identity_facts_receipt_digest(&self) -> &str {
        match self {
            Self::PlanInventoryEvidence {
                identity_facts_receipt_digest,
                ..
            }
            | Self::SpecInventoryEvidence {
                identity_facts_receipt_digest,
                ..
            } => identity_facts_receipt_digest,
        }
    }

    pub fn candidate(&self) -> &CandidateBinding {
        match self {
            Self::PlanInventoryEvidence { candidate, .. }
            | Self::SpecInventoryEvidence { candidate, .. } => candidate,
        }
    }

    fn validate(&self) -> Result<(), LicenseDecisionRefusal> {
        if !is_sha256(self.plan_sha256())
            || !is_sha256(self.inventory_sha256())
            || !is_sha256(self.evidence_bundle_sha256())
            || !is_sha256(self.identity_facts_receipt_digest())
            || self.plan_revision() == 0
            || self.plan_sha256() == self.inventory_sha256()
            || self.plan_sha256() == self.evidence_bundle_sha256()
            || self.inventory_sha256() == self.evidence_bundle_sha256()
        {
            return Err(LicenseDecisionRefusal::PolicyBindingIncomplete);
        }
        self.candidate().validate()?;
        if let Some(spec_sha256) = self.spec_sha256()
            && (!is_sha256(spec_sha256) || spec_sha256 == self.inventory_sha256())
        {
            return Err(LicenseDecisionRefusal::PolicyBindingIncomplete);
        }
        Ok(())
    }
}

/// Exact policy + stage binding persisted with the decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyBinding {
    catalog_sha256: String,
    profile_sha256: String,
    policy_sha256: String,
    stage: StageBinding,
}

impl PolicyBinding {
    /// Construct a binding from Core-owned real artefacts.
    ///
    /// Catalogue/profile must be the current shared policy; the policy digest is
    /// the builder-policy digest Core loaded; the stage is a real plan or spec
    /// inventory stage with its signed identity-facts receipt digest.
    pub fn new(
        catalog_sha256: impl Into<String>,
        profile_sha256: impl Into<String>,
        policy_sha256: impl Into<String>,
        stage: StageBinding,
    ) -> Result<Self, LicenseDecisionRefusal> {
        let catalog_sha256 = catalog_sha256.into();
        let profile_sha256 = profile_sha256.into();
        let policy_sha256 = policy_sha256.into();
        if catalog_sha256 != default_catalog_sha256() || profile_sha256 != default_profile_sha256()
        {
            return Err(LicenseDecisionRefusal::PolicyMismatch);
        }
        if !is_sha256(&policy_sha256) {
            return Err(LicenseDecisionRefusal::PolicyBindingIncomplete);
        }
        stage.validate()?;
        Ok(Self {
            catalog_sha256,
            profile_sha256,
            policy_sha256,
            stage,
        })
    }

    pub fn catalog_sha256(&self) -> &str {
        &self.catalog_sha256
    }

    pub fn profile_sha256(&self) -> &str {
        &self.profile_sha256
    }

    pub fn policy_sha256(&self) -> &str {
        &self.policy_sha256
    }

    pub fn stage(&self) -> &StageBinding {
        &self.stage
    }

    pub fn candidate(&self) -> &CandidateBinding {
        self.stage.candidate()
    }

    pub fn identity_facts_receipt_digest(&self) -> &str {
        self.stage.identity_facts_receipt_digest()
    }

    /// Serialized stage for durable storage (`stage_json`).
    pub fn stage_json(&self) -> String {
        serde_json::to_string(&self.stage).expect("binding stage is serializable")
    }
}

/// One documentation output and its exact licence decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DocumentationReview {
    pub path: String,
    pub license: String,
    pub license_text_sha256: String,
    pub obligations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LicenseDecisionReview {
    pub decision: String,
    pub pin: String,
    pub name: String,
    pub version: String,
    pub output_drv_path: String,
    pub output_store_path: String,
    pub output_nar_hash: String,
    pub source_drv_path: String,
    pub source_store_path: String,
    pub source_output_hash: String,
    pub license_expression: String,
    pub software_text_sha256: String,
    pub selected_branch: String,
    pub documentation: Vec<DocumentationReview>,
    pub use_context: String,
    pub commercial_use: bool,
    pub payment_required: bool,
    pub obligations_reviewed: bool,
    pub software_obligations: Vec<String>,
    pub documentation_obligations: Vec<String>,
    /// Filled by Core after parsing: hash of the exact canonical Core input.
    #[serde(default)]
    pub request_sha256: String,
    /// Filled by Core after parsing: `sha256(response_text_bytes)`.
    #[serde(default)]
    pub response_sha256: String,
}

/// Documentation result persisted with the decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentationDecision {
    pub path: String,
    pub license: String,
    pub license_text_sha256: String,
    pub obligations: Vec<String>,
    pub verdict: String,
}

/// Exact Lead decision payload Core may persist. Fields are crate-visible for
/// the database writer; the type is not `Deserialize` and has no public
/// constructor, so a model/HTTP payload cannot mint it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedLicenseDecision {
    pub(crate) identity: serde_json::Value,
    pub(crate) license_expression: String,
    pub(crate) selected_branch: String,
    pub(crate) documentation: Vec<DocumentationDecision>,
    pub(crate) software_obligations: Vec<String>,
    pub(crate) documentation_obligations: Vec<String>,
    pub(crate) software_verdict: String,
    pub(crate) evidence_sha256: String,
    pub(crate) identity_sha256: String,
    pub(crate) identity_facts_receipt_digest: String,
    pub(crate) content_sha256: String,
    pub(crate) provenance: serde_json::Value,
    pub(crate) use_context: String,
    pub(crate) commercial_use: bool,
    pub(crate) payment_required: bool,
    pub(crate) decision: String,
    pub(crate) revision: u64,
    pub(crate) binding: PolicyBinding,
    pub(crate) project_id: String,
    pub(crate) lead_request_id: String,
    pub(crate) lead_reservation_id: String,
    pub(crate) lead_actor_sha256: String,
    pub(crate) lead_request_sha256: String,
    pub(crate) lead_output_sha256: String,
    pub(crate) lead_principal: String,
    pub(crate) delegation_id: String,
    pub(crate) delegation_matter: String,
    pub(crate) delegation_option: String,
    pub(crate) delegation_option_sha256: String,
    pub(crate) delegation_revision: u64,
    pub(crate) delegation_pin: String,
    pub(crate) delegation_use_context: String,
    pub(crate) delegation_criterion: String,
    pub(crate) delegation_deadline: String,
}

impl VerifiedLicenseDecision {
    pub fn decision(&self) -> &str {
        &self.decision
    }

    pub fn license_expression(&self) -> &str {
        &self.license_expression
    }

    pub fn selected_branch(&self) -> &str {
        &self.selected_branch
    }

    pub fn documentation(&self) -> &[DocumentationDecision] {
        &self.documentation
    }

    pub fn software_obligations(&self) -> &[String] {
        &self.software_obligations
    }

    pub fn documentation_obligations(&self) -> &[String] {
        &self.documentation_obligations
    }

    pub fn software_verdict(&self) -> &str {
        &self.software_verdict
    }

    pub fn evidence_sha256(&self) -> &str {
        &self.evidence_sha256
    }

    pub fn identity_sha256(&self) -> &str {
        &self.identity_sha256
    }

    pub fn identity_facts_receipt_digest(&self) -> &str {
        &self.identity_facts_receipt_digest
    }

    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }

    pub fn provenance(&self) -> &serde_json::Value {
        &self.provenance
    }

    pub fn use_context(&self) -> &str {
        &self.use_context
    }

    pub fn commercial_use(&self) -> bool {
        self.commercial_use
    }

    pub fn payment_required(&self) -> bool {
        self.payment_required
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn verdict(&self) -> &str {
        &self.software_verdict
    }

    pub fn identity(&self) -> &serde_json::Value {
        &self.identity
    }

    pub fn binding(&self) -> &PolicyBinding {
        &self.binding
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    pub fn lead_request_id(&self) -> &str {
        &self.lead_request_id
    }

    pub fn lead_reservation_id(&self) -> &str {
        &self.lead_reservation_id
    }

    pub fn lead_actor_sha256(&self) -> &str {
        &self.lead_actor_sha256
    }

    pub fn lead_request_sha256(&self) -> &str {
        &self.lead_request_sha256
    }

    pub fn lead_output_sha256(&self) -> &str {
        &self.lead_output_sha256
    }

    pub fn lead_principal(&self) -> &str {
        &self.lead_principal
    }

    pub fn delegation_id(&self) -> &str {
        &self.delegation_id
    }

    pub fn delegation_matter(&self) -> &str {
        &self.delegation_matter
    }

    pub fn delegation_option(&self) -> &str {
        &self.delegation_option
    }

    pub fn delegation_option_sha256(&self) -> &str {
        &self.delegation_option_sha256
    }

    pub fn delegation_revision(&self) -> u64 {
        self.delegation_revision
    }

    pub fn delegation_pin(&self) -> &str {
        &self.delegation_pin
    }

    pub fn delegation_use_context(&self) -> &str {
        &self.delegation_use_context
    }

    pub fn delegation_criterion(&self) -> &str {
        &self.delegation_criterion
    }

    pub fn delegation_deadline(&self) -> &str {
        &self.delegation_deadline
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LicenseDecisionRefusal {
    InvalidInput(String),
    InvalidExpression,
    UnknownIdentity,
    IdentityMismatch,
    LicenseExpressionMismatch,
    AssetCanonicalMismatch,
    AssetCanonicalHashMissing,
    LicenseTextDigest,
    MissingLicensingFact,
    BranchNotInExpression,
    ObligationsMismatch,
    DocumentationMismatch,
    DocumentationTextMissing,
    DocumentationDenied,
    DocumentationUnknown,
    SoftwareDenied,
    SoftwareUnknown,
    PolicyNotEvaluated,
    CommercialCriterion,
    PaymentRequired,
    DecisionMismatch,
    PolicyBindingIncomplete,
    PolicyBindingForged,
    PolicyMismatch,
    PlanBindingMismatch,
    SpecBindingMismatch,
    ReceiptMismatch,
    ReceiptDigestMismatch,
    Actor(String),
    RequestBinding,
    ResponseBinding,
    ProjectMismatch,
    PrincipalMismatch,
    LeadTerminal,
    DelegationMismatch,
    DelegationExpired,
    DelegationRevoked,
}

impl std::fmt::Display for LicenseDecisionRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(message) => write!(formatter, "invalid licence decision: {message}"),
            Self::InvalidExpression => formatter.write_str("the licence expression is invalid"),
            Self::UnknownIdentity => {
                formatter.write_str("the review identity is not in the exact shared evidence")
            }
            Self::IdentityMismatch => {
                formatter.write_str("the review identity fields do not match the shared evidence")
            }
            Self::LicenseExpressionMismatch => formatter
                .write_str("the reviewed licence expression is not the identity licensing fact"),
            Self::AssetCanonicalMismatch => {
                formatter.write_str("the shared licence asset is not canonical and hash-bound")
            }
            Self::AssetCanonicalHashMissing => {
                formatter.write_str("the shared asset has no canonical hash")
            }
            Self::LicenseTextDigest => {
                formatter.write_str("the software licence text digest is not a trusted text")
            }
            Self::MissingLicensingFact => {
                formatter.write_str("no licensing fact establishes the selected expression")
            }
            Self::BranchNotInExpression => {
                formatter.write_str("the selected branch is not an atom of the licence AST")
            }
            Self::ObligationsMismatch => {
                formatter.write_str("the obligations are not exact for the selected context")
            }
            Self::DocumentationMismatch => formatter
                .write_str("the documentation outputs or licences differ from the exact evidence"),
            Self::DocumentationTextMissing => {
                formatter.write_str("a documentation licence has no trusted text digest")
            }
            Self::DocumentationDenied => {
                formatter.write_str("a documentation licence is denied by policy")
            }
            Self::DocumentationUnknown => {
                formatter.write_str("a documentation licence is unknown and cannot be classified")
            }
            Self::SoftwareDenied => {
                formatter.write_str("the selected software licence is denied by policy")
            }
            Self::SoftwareUnknown => formatter
                .write_str("the selected software licence is unknown and cannot be classified"),
            Self::PolicyNotEvaluated => {
                formatter.write_str("the current licence profile does not evaluate the closure")
            }
            Self::CommercialCriterion => formatter.write_str(
                "commercial use without a mandatory fee is not established by the licence",
            ),
            Self::PaymentRequired => {
                formatter.write_str("a payment obligation cannot be accepted under the delegation")
            }
            Self::DecisionMismatch => {
                formatter.write_str("the proposed decision does not match the verified verdict")
            }
            Self::PolicyBindingIncomplete => {
                formatter.write_str("the policy/spec/inventory/candidate binding is incomplete")
            }
            Self::PolicyBindingForged => formatter.write_str(
                "a project-config hash or hashed base commit cannot stand in for a real artefact",
            ),
            Self::PolicyMismatch => formatter
                .write_str("the catalogue/profile/policy binding is not the current policy"),
            Self::PlanBindingMismatch => {
                formatter.write_str("the identity-facts receipt does not bind the plan draft")
            }
            Self::SpecBindingMismatch => {
                formatter.write_str("the identity-facts receipt does not bind the runtime spec")
            }
            Self::ReceiptMismatch => formatter
                .write_str("the identity-facts receipt does not bind the exact reviewed identity"),
            Self::ReceiptDigestMismatch => {
                formatter.write_str("the identity-facts receipt digest differs from the binding")
            }
            Self::Actor(message) => write!(formatter, "the Lead actor is invalid: {message}"),
            Self::RequestBinding => formatter
                .write_str("the review and reservation do not bind the exact Core request digest"),
            Self::ResponseBinding => formatter
                .write_str("the review output digest is not the exact reservation response text"),
            Self::ProjectMismatch => {
                formatter.write_str("the actor, delegation and decision projects differ")
            }
            Self::PrincipalMismatch => {
                formatter.write_str("the actor principal differs from the decision principal")
            }
            Self::LeadTerminal => {
                formatter.write_str("the Lead request is terminal and cannot authorize a decision")
            }
            Self::DelegationMismatch => {
                formatter.write_str("the Owner delegation does not bind the exact request")
            }
            Self::DelegationExpired => {
                formatter.write_str("the Owner licence delegation has expired")
            }
            Self::DelegationRevoked => {
                formatter.write_str("the Owner licence delegation has been revoked")
            }
        }
    }
}

impl std::error::Error for LicenseDecisionRefusal {}

impl From<ClassificationRefusal> for LicenseDecisionRefusal {
    fn from(refusal: ClassificationRefusal) -> Self {
        Self::Actor(refusal.to_string())
    }
}

fn is_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn exact<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(serde_json::Value::as_str)
}

fn source_asset() -> &'static serde_json::Value {
    static ASSET: OnceLock<serde_json::Value> = OnceLock::new();
    ASSET.get_or_init(|| {
        serde_json::from_str(VERSIONED_SOURCE_EVIDENCE_JSON)
            .expect("the shared versioned source evidence is valid JSON")
    })
}

fn write_canonical(value: &serde_json::Value, output: &mut String) {
    match value {
        serde_json::Value::Null => output.push_str("null"),
        serde_json::Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        serde_json::Value::Number(value) => output.push_str(&value.to_string()),
        serde_json::Value::String(value) => {
            output.push_str(&serde_json::to_string(value).expect("a string serializes"));
        }
        serde_json::Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_canonical(value, output);
            }
            output.push(']');
        }
        serde_json::Value::Object(values) => {
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort();
            output.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push_str(&serde_json::to_string(key).expect("a key serializes"));
                output.push(':');
                write_canonical(&values[key], output);
            }
            output.push('}');
        }
    }
}

/// Canonical sorted, compact JSON with a trailing LF, as used by the builder.
pub fn canonical_json_sha256(value: &serde_json::Value) -> String {
    let mut canonical = String::new();
    write_canonical(value, &mut canonical);
    canonical.push('\n');
    sha256_hex(canonical.as_bytes())
}

/// Digest of the complete verified identity-facts receipt.
pub fn identity_facts_receipt_digest(facts: &VerifiedIdentityFacts) -> String {
    let value =
        serde_json::to_value(facts.receipt()).expect("the identity-facts receipt serializes");
    canonical_json_sha256(&value)
}

/// Re-verify a stored identity-facts receipt with the broker bearer.
///
/// The persisted envelope's own signed challenge is used as the expected
/// challenge, then every persisted column is cross-checked against it. The MAC
/// covers the complete envelope, so a receipt for a foreign plan/spec/pin is
/// refused by the stage binding below even though the broker signature is valid.
pub fn verify_stored_identity_facts(
    stored: &crate::db::StoredIdentityFactsReceipt,
    bearer: &[u8],
) -> Result<VerifiedIdentityFacts, LicenseDecisionRefusal> {
    // The durable column stores the signed envelope; the tag is stored
    // separately so a receipt can never sign its own signature.
    let envelope: super::identity_facts::IdentityFactsEnvelope =
        serde_json::from_str(&stored.envelope_json)
            .map_err(|_| LicenseDecisionRefusal::ReceiptMismatch)?;
    let receipt = IdentityFactsReceipt {
        envelope,
        signature: stored.signature.clone(),
    };
    if receipt.envelope.context != IDENTITY_FACTS_CONTEXT
        || receipt.envelope.challenge.plan_sha256 != stored.plan_sha256
        || receipt.envelope.challenge.spec_sha256 != stored.spec_sha256
        || receipt.envelope.challenge.pin != stored.pin
        || receipt.envelope.manifest.observed_root != stored.observed_root
        || receipt.envelope.manifest.fact_count != stored.fact_count as usize
        || receipt.envelope.manifest.closure_manifest_sha256 != stored.closure_manifest_sha256
    {
        return Err(LicenseDecisionRefusal::ReceiptMismatch);
    }
    identity_facts::verify(&receipt, bearer, &receipt.envelope.challenge)
        .map_err(|_| LicenseDecisionRefusal::ReceiptMismatch)
}

fn tokenize_expression(expression: &str) -> Result<Vec<String>, LicenseDecisionRefusal> {
    if expression.trim().is_empty() {
        return Err(LicenseDecisionRefusal::InvalidExpression);
    }
    let mut tokens = Vec::new();
    let mut depth = 0i32;
    for raw in expression.split_whitespace() {
        let mut token = raw;
        while let Some(rest) = token.strip_prefix('(') {
            depth += 1;
            token = rest;
        }
        if token.is_empty() {
            continue;
        }
        let mut trailing = 0;
        while let Some(rest) = token.strip_suffix(')') {
            trailing += 1;
            token = rest;
        }
        if !token.is_empty() {
            tokens.push(token.to_string());
        }
        depth -= trailing;
        if depth < 0 || trailing > 0 && token.is_empty() {
            return Err(LicenseDecisionRefusal::InvalidExpression);
        }
    }
    if depth != 0 {
        return Err(LicenseDecisionRefusal::InvalidExpression);
    }
    Ok(tokens)
}

fn is_operator(token: &str) -> bool {
    matches!(token.to_ascii_uppercase().as_str(), "AND" | "OR" | "WITH")
}

/// Collect the selectable atom branches of an SPDX `AND`/`OR`/`WITH` expression.
pub fn expression_branches(expression: &str) -> Result<Vec<String>, LicenseDecisionRefusal> {
    let tokens = tokenize_expression(expression)?;
    let mut branches = Vec::new();
    let mut index = 0usize;
    while index < tokens.len() {
        let atom = &tokens[index];
        if is_operator(atom) {
            return Err(LicenseDecisionRefusal::InvalidExpression);
        }
        index += 1;
        let mut branch = atom.clone();
        if tokens
            .get(index)
            .is_some_and(|value| value.eq_ignore_ascii_case("WITH"))
        {
            let exception = tokens
                .get(index + 1)
                .filter(|value| !is_operator(value))
                .ok_or(LicenseDecisionRefusal::InvalidExpression)?;
            branch = format!("{} WITH {}", atom, exception);
            index += 2;
        }
        branches.push(branch);
        if index == tokens.len() {
            break;
        }
        let operator = &tokens[index];
        if !operator.eq_ignore_ascii_case("AND") && !operator.eq_ignore_ascii_case("OR") {
            return Err(LicenseDecisionRefusal::InvalidExpression);
        }
        index += 1;
        if index == tokens.len() {
            return Err(LicenseDecisionRefusal::InvalidExpression);
        }
    }
    if branches.is_empty() {
        return Err(LicenseDecisionRefusal::InvalidExpression);
    }
    Ok(branches)
}

fn base_atom(branch: &str) -> &str {
    branch
        .split_once(" WITH ")
        .map(|(base, _)| base)
        .unwrap_or(branch)
}

fn catalogue_entry(identifier: &str) -> Option<&'static CatalogEntry> {
    let catalog = default_catalog();
    let resolved = catalog
        .alias_index
        .get(identifier)
        .map(String::as_str)
        .unwrap_or(identifier);
    catalog.entries.get(resolved)
}

fn catalogue_obligations(identifier: &str) -> Option<Vec<String>> {
    catalogue_entry(identifier).map(|entry| entry.obligations.clone())
}

fn family_permits_commercial_no_fee(family: &str) -> bool {
    matches!(
        family,
        "permissive" | "weak-copyleft" | "strong-copyleft" | "network-copyleft" | "public-domain"
    )
}

fn family_is_copyleft(family: &str) -> bool {
    matches!(
        family,
        "weak-copyleft" | "strong-copyleft" | "network-copyleft"
    )
}

/// A licence text belongs to the reviewed expression when it is the exact
/// identifier, the same family with an explicit `-only`/`-or-later` suffix, or
/// the selected `WITH` base atom. It never lets an unrelated text stand in.
fn text_license_matches(text_license: &str, expression: &str) -> bool {
    text_license == expression
        || text_license == expression.replace("-or-later", "-only")
        || text_license == base_atom(expression)
        || text_license == base_atom(expression).replace("-or-later", "-only")
}

/// Bool-free reconciliation of a generic prior DENY.
///
/// Only an exact single-atom expression that is the identity licensing fact, a
/// trusted licence-text digest, exact obligations, an active catalogue entry and
/// the commercial/no-fee predicate can reconcile it. Source-available
/// (BUSL/Commons-Clause), NC and paid/unknown licences stay denied.
pub(crate) fn reconcile_prior_deny(
    expression: &str,
    selected_branch: &str,
    identity_license: &str,
    trusted_text: bool,
    obligations_exact: bool,
    commercial_use: bool,
    payment_required: bool,
) -> Result<(), LicenseDecisionRefusal> {
    if expression != selected_branch || expression != identity_license {
        return Err(LicenseDecisionRefusal::SoftwareDenied);
    }
    if payment_required {
        return Err(LicenseDecisionRefusal::PaymentRequired);
    }
    if !commercial_use {
        return Err(LicenseDecisionRefusal::CommercialCriterion);
    }
    if !trusted_text || !obligations_exact {
        return Err(LicenseDecisionRefusal::SoftwareDenied);
    }
    let entry = catalogue_entry(base_atom(selected_branch))
        .ok_or(LicenseDecisionRefusal::SoftwareUnknown)?;
    if entry.family == "source-available" || !family_is_copyleft(&entry.family) {
        return Err(LicenseDecisionRefusal::SoftwareDenied);
    }
    if entry.state != "active" || entry.obligations.is_empty() {
        return Err(LicenseDecisionRefusal::SoftwareDenied);
    }
    Ok(())
}

/// Inputs Core must provide from its own state. Every field is a database or
/// Core-owned value, never a caller payload attribute.
pub struct LicenseDecisionContext<'a> {
    pub project_id: &'a str,
    pub lead_request_id: &'a str,
    pub principal: &'a str,
    pub request_sha256: &'a str,
    pub project_config_hash: &'a str,
    pub base_commit: &'a str,
    pub reservation: &'a crate::db::LicenseEvidenceReservation,
    pub delegation: &'a crate::db::AgenticOwnerDecisionRecord,
    pub delegation_revoked: bool,
    pub identity_facts: &'a VerifiedIdentityFacts,
    pub binding: &'a PolicyBinding,
}

/// Validate one model review against the exact shared evidence, the identity
/// receipt, the Lead actor, the Owner delegation and the current policy binding.
pub fn verify_license_evidence_review(
    review: &LicenseDecisionReview,
    context: &LicenseDecisionContext<'_>,
) -> Result<VerifiedLicenseDecision, LicenseDecisionRefusal> {
    if review.pin.trim().is_empty()
        || review.name.trim().is_empty()
        || review.version.trim().is_empty()
        || review.use_context.trim().is_empty()
    {
        return Err(LicenseDecisionRefusal::InvalidInput(
            "identity, version and use context are required".into(),
        ));
    }
    if !matches!(
        review.decision.as_str(),
        "ACCEPT" | "NEEDS_EVIDENCE" | "DENY"
    ) {
        return Err(LicenseDecisionRefusal::InvalidInput(
            "decision must be ACCEPT, NEEDS_EVIDENCE or DENY".into(),
        ));
    }
    if !is_commit(context.base_commit) {
        return Err(LicenseDecisionRefusal::PolicyBindingIncomplete);
    }
    if context.project_id != context.reservation.project_id {
        return Err(LicenseDecisionRefusal::ProjectMismatch);
    }
    if context.lead_request_id != context.reservation.objective_id {
        return Err(LicenseDecisionRefusal::ProjectMismatch);
    }
    if context.principal != context.reservation.principal_id {
        return Err(LicenseDecisionRefusal::PrincipalMismatch);
    }
    if matches!(
        context.reservation.lead_state.as_str(),
        "BLOCKED" | "FAILED"
    ) || matches!(
        context.reservation.lead_state.as_str(),
        "WITHDRAWN" | "CANCELLED"
    ) {
        return Err(LicenseDecisionRefusal::LeadTerminal);
    }
    if context.reservation.expires_at_ms <= chrono::Utc::now().timestamp_millis() {
        return Err(LicenseDecisionRefusal::Actor(
            "the licence evidence reservation has expired".into(),
        ));
    }

    let mandate = VerifiedLicenseMandate::from_reservation(
        context.reservation,
        context.lead_request_id,
        context.delegation,
        context.delegation_revoked,
        &review.pin,
    )?;
    if mandate.project_id() != context.project_id {
        return Err(LicenseDecisionRefusal::ProjectMismatch);
    }
    if mandate.use_context() != review.use_context {
        return Err(LicenseDecisionRefusal::DelegationMismatch);
    }
    let actor = mandate.actor();
    if actor.request_sha256() != context.request_sha256
        || review.request_sha256 != context.request_sha256
    {
        return Err(LicenseDecisionRefusal::RequestBinding);
    }
    if review.response_sha256 != actor.output_sha256() {
        return Err(LicenseDecisionRefusal::ResponseBinding);
    }
    let lead_actor_sha256 = mandate.actor_binding_sha256();
    let delegation_option_sha256 = canonical_json_sha256(&serde_json::json!({
        "matter": context.delegation.matter,
        "options": context.delegation.options,
        "selectedOption": context.delegation.selected_option,
    }));

    // Binding: current policy, tagged real artefact, no disguised config/base.
    if context.binding.catalog_sha256() != default_catalog_sha256()
        || context.binding.profile_sha256() != default_profile_sha256()
    {
        return Err(LicenseDecisionRefusal::PolicyMismatch);
    }
    if context.binding.policy_sha256() != default_binding_sha256() {
        return Err(LicenseDecisionRefusal::PolicyMismatch);
    }
    if context.binding.identity_facts_receipt_digest()
        != identity_facts_receipt_digest(context.identity_facts)
    {
        return Err(LicenseDecisionRefusal::ReceiptDigestMismatch);
    }
    let receipt = context.identity_facts.receipt();
    if receipt.envelope.context != IDENTITY_FACTS_CONTEXT
        || receipt.envelope.challenge.pin != review.pin
    {
        return Err(LicenseDecisionRefusal::ReceiptMismatch);
    }
    if context.binding.stage().plan_sha256() == context.project_config_hash
        || context.binding.stage().inventory_sha256() == context.project_config_hash
        || context.binding.stage().evidence_bundle_sha256() == context.project_config_hash
    {
        return Err(LicenseDecisionRefusal::PolicyBindingForged);
    }
    if receipt.envelope.challenge.plan_sha256 != context.binding.stage().plan_sha256() {
        return Err(LicenseDecisionRefusal::PlanBindingMismatch);
    }
    if let Some(spec_sha256) = context.binding.stage().spec_sha256() {
        if receipt.envelope.challenge.spec_sha256 != spec_sha256 {
            return Err(LicenseDecisionRefusal::SpecBindingMismatch);
        }
        if spec_sha256 == context.project_config_hash {
            return Err(LicenseDecisionRefusal::PolicyBindingForged);
        }
    }
    match context.binding.candidate() {
        CandidateBinding::Candidate { candidate_sha256 } => {
            if candidate_sha256 == context.project_config_hash
                || candidate_sha256 == &sha256_hex(context.base_commit.as_bytes())
            {
                return Err(LicenseDecisionRefusal::PolicyBindingForged);
            }
        }
        CandidateBinding::Base { base_commit } => {
            if base_commit != context.base_commit {
                return Err(LicenseDecisionRefusal::PolicyBindingIncomplete);
            }
        }
    }

    // Exact identity and canonical shared asset.
    let asset = source_asset();
    let identities = asset
        .get("identities")
        .and_then(serde_json::Value::as_array)
        .ok_or(LicenseDecisionRefusal::UnknownIdentity)?;
    let identity = identities
        .iter()
        .find(|identity| {
            exact(identity, "pin") == Some(review.pin.as_str())
                && exact(identity, "name") == Some(review.name.as_str())
                && exact(identity, "version") == Some(review.version.as_str())
        })
        .ok_or(LicenseDecisionRefusal::UnknownIdentity)?;
    for (_label, actual, expected) in [
        (
            "output drv",
            review.output_drv_path.as_str(),
            exact(
                identity.get("output").unwrap_or(&serde_json::Value::Null),
                "drvPath",
            ),
        ),
        (
            "output path",
            review.output_store_path.as_str(),
            exact(
                identity.get("output").unwrap_or(&serde_json::Value::Null),
                "storePath",
            ),
        ),
        (
            "output nar",
            review.output_nar_hash.as_str(),
            exact(
                identity.get("output").unwrap_or(&serde_json::Value::Null),
                "narHash",
            ),
        ),
        (
            "source drv",
            review.source_drv_path.as_str(),
            exact(
                identity.get("source").unwrap_or(&serde_json::Value::Null),
                "drvPath",
            ),
        ),
        (
            "source path",
            review.source_store_path.as_str(),
            exact(
                identity.get("source").unwrap_or(&serde_json::Value::Null),
                "storePath",
            ),
        ),
        (
            "source hash",
            review.source_output_hash.as_str(),
            exact(
                identity.get("source").unwrap_or(&serde_json::Value::Null),
                "outputHash",
            ),
        ),
    ] {
        if expected != Some(actual) {
            return Err(LicenseDecisionRefusal::IdentityMismatch);
        }
    }
    let evidence = asset
        .get("evidence")
        .ok_or(LicenseDecisionRefusal::AssetCanonicalHashMissing)?;
    let license_evidence = identity
        .get("licenseEvidence")
        .cloned()
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    let output_files = identity
        .get("outputFiles")
        .cloned()
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    if canonical_json_sha256(&license_evidence)
        != exact(evidence, "licenseEvidenceCanonicalSha256")
            .ok_or(LicenseDecisionRefusal::AssetCanonicalHashMissing)?
    {
        return Err(LicenseDecisionRefusal::AssetCanonicalMismatch);
    }
    if canonical_json_sha256(&output_files)
        != exact(evidence, "outputFilesCanonicalSha256")
            .ok_or(LicenseDecisionRefusal::AssetCanonicalHashMissing)?
    {
        return Err(LicenseDecisionRefusal::AssetCanonicalMismatch);
    }
    let evidence_sha256 = exact(evidence, "licenseEvidenceCanonicalSha256")
        .ok_or(LicenseDecisionRefusal::AssetCanonicalHashMissing)?
        .to_owned();
    let identity_sha256 = canonical_json_sha256(identity);
    if exact(identity, "license") != Some(review.license_expression.as_str()) {
        return Err(LicenseDecisionRefusal::LicenseExpressionMismatch);
    }

    let license_entries = license_evidence
        .as_array()
        .ok_or(LicenseDecisionRefusal::AssetCanonicalMismatch)?;
    let mut text_by_license: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut trusted_text = false;
    for entry in license_entries {
        let sha = exact(entry, "sha256").ok_or(LicenseDecisionRefusal::AssetCanonicalMismatch)?;
        if !is_sha256(sha) {
            return Err(LicenseDecisionRefusal::AssetCanonicalMismatch);
        }
        if let Some(license) = exact(entry, "license") {
            text_by_license.entry(license).or_default().insert(sha);
        }
        if exact(entry, "purpose") == Some("license-text")
            && sha == review.software_text_sha256
            && exact(entry, "license")
                .is_some_and(|license| text_license_matches(license, &review.license_expression))
        {
            trusted_text = true;
        }
    }
    if !trusted_text {
        return Err(LicenseDecisionRefusal::LicenseTextDigest);
    }
    let licensing_fact = license_entries.iter().any(|entry| {
        exact(entry, "role") == Some("licensing")
            && exact(entry, "license") == Some(review.license_expression.as_str())
    });
    if !licensing_fact {
        return Err(LicenseDecisionRefusal::MissingLicensingFact);
    }

    // Selected branch must be an atom of the expression AST.
    let branches = expression_branches(&review.license_expression)?;
    if !branches
        .iter()
        .any(|branch| branch == &review.selected_branch)
    {
        return Err(LicenseDecisionRefusal::BranchNotInExpression);
    }
    let software_obligations = catalogue_obligations(base_atom(&review.selected_branch))
        .ok_or(LicenseDecisionRefusal::SoftwareUnknown)?;
    if review.software_obligations != software_obligations {
        return Err(LicenseDecisionRefusal::ObligationsMismatch);
    }
    let selected_entry = catalogue_entry(base_atom(&review.selected_branch))
        .ok_or(LicenseDecisionRefusal::SoftwareUnknown)?;
    let commercial_ok = review.commercial_use
        && !review.payment_required
        && family_permits_commercial_no_fee(&selected_entry.family);
    let software_verdict = match evaluate(
        &review.selected_branch,
        LicenseContext::Closure,
        default_catalog(),
        default_profile(),
    ) {
        LicenseDecision::Allowed => "ALLOWED",
        LicenseDecision::Unknown => "UNKNOWN_CLASSIFIED",
        LicenseDecision::Denied => {
            reconcile_prior_deny(
                &review.license_expression,
                &review.selected_branch,
                &review.license_expression,
                trusted_text,
                review.software_obligations == software_obligations,
                review.commercial_use,
                review.payment_required,
            )?;
            "RECONCILED_COPYLEFT"
        }
        LicenseDecision::NotEvaluated => return Err(LicenseDecisionRefusal::PolicyNotEvaluated),
    };
    let expression_decision = evaluate(
        &review.license_expression,
        LicenseContext::Closure,
        default_catalog(),
        default_profile(),
    );
    if expression_decision == LicenseDecision::Denied
        && review.license_expression != review.selected_branch
    {
        return Err(LicenseDecisionRefusal::SoftwareDenied);
    }
    if expression_decision == LicenseDecision::NotEvaluated {
        return Err(LicenseDecisionRefusal::PolicyNotEvaluated);
    }

    // Documentation: exact outputs, exact text and per-context verdicts.
    let expected_docs = output_files
        .as_array()
        .ok_or(LicenseDecisionRefusal::DocumentationMismatch)?
        .iter()
        .filter(|entry| exact(entry, "purpose") == Some("documentation"))
        .map(|entry| {
            (
                exact(entry, "path").unwrap_or_default().to_owned(),
                exact(entry, "license").unwrap_or_default().to_owned(),
            )
        })
        .collect::<BTreeSet<_>>();
    let mut actual_docs = BTreeSet::new();
    let mut documentation = Vec::new();
    let mut documentation_obligation_set = BTreeSet::new();
    for document in &review.documentation {
        if !actual_docs.insert((document.path.clone(), document.license.clone())) {
            return Err(LicenseDecisionRefusal::DocumentationMismatch);
        }
        let obligations = catalogue_obligations(&document.license)
            .ok_or(LicenseDecisionRefusal::DocumentationUnknown)?;
        if document.obligations != obligations {
            return Err(LicenseDecisionRefusal::ObligationsMismatch);
        }
        let entry = catalogue_entry(&document.license)
            .ok_or(LicenseDecisionRefusal::DocumentationUnknown)?;
        if !family_permits_commercial_no_fee(&entry.family) {
            return Err(LicenseDecisionRefusal::DocumentationDenied);
        }
        let trusted = text_by_license
            .get(document.license.as_str())
            .is_some_and(|digests| digests.contains(document.license_text_sha256.as_str()));
        if !trusted {
            return Err(LicenseDecisionRefusal::DocumentationTextMissing);
        }
        let verdict = match evaluate(
            &document.license,
            LicenseContext::Closure,
            default_catalog(),
            default_profile(),
        ) {
            LicenseDecision::Allowed => "ALLOWED",
            LicenseDecision::Unknown => "UNKNOWN_CLASSIFIED",
            LicenseDecision::Denied => return Err(LicenseDecisionRefusal::DocumentationDenied),
            LicenseDecision::NotEvaluated => {
                return Err(LicenseDecisionRefusal::PolicyNotEvaluated);
            }
        };
        documentation_obligation_set.extend(obligations);
        documentation.push(DocumentationDecision {
            path: document.path.clone(),
            license: document.license.clone(),
            license_text_sha256: document.license_text_sha256.clone(),
            obligations: document.obligations.clone(),
            verdict: verdict.to_owned(),
        });
    }
    if actual_docs != expected_docs {
        return Err(LicenseDecisionRefusal::DocumentationMismatch);
    }
    let documentation_obligations = documentation_obligation_set.into_iter().collect::<Vec<_>>();
    if review.documentation_obligations != documentation_obligations {
        return Err(LicenseDecisionRefusal::ObligationsMismatch);
    }
    documentation.sort_by(|left, right| left.path.cmp(&right.path));

    if review.decision == "ACCEPT" {
        if review.use_context != "commercial-development" {
            return Err(LicenseDecisionRefusal::CommercialCriterion);
        }
        if !review.obligations_reviewed {
            return Err(LicenseDecisionRefusal::ObligationsMismatch);
        }
        if !commercial_ok {
            return Err(LicenseDecisionRefusal::CommercialCriterion);
        }
        if matches!(software_verdict, "DENIED") {
            return Err(LicenseDecisionRefusal::SoftwareDenied);
        }
    }
    if review.decision == "NEEDS_EVIDENCE" && software_verdict == "ALLOWED" {
        return Err(LicenseDecisionRefusal::DecisionMismatch);
    }
    if review.decision == "DENY" && software_verdict == "ALLOWED" && commercial_ok {
        // A Lead may reject a permissive licence, but the record must still be
        // an explicit, evidence-bound denial rather than an accidental one.
    }

    let receipt_digest = identity_facts_receipt_digest(context.identity_facts);
    let provenance = serde_json::json!({
        "pin": review.pin,
        "identitySha256": identity_sha256,
        "outputDrvPath": review.output_drv_path,
        "outputStorePath": review.output_store_path,
        "outputNarHash": review.output_nar_hash,
        "sourceDrvPath": review.source_drv_path,
        "sourceStorePath": review.source_store_path,
        "sourceOutputHash": review.source_output_hash,
        "licenseEvidenceSha256": evidence_sha256,
        "identityFactsReceiptDigest": receipt_digest,
        "stage": context.binding.stage_json(),
        "candidate": context.binding.candidate(),
    });
    let payload = serde_json::json!({
        "projectId": context.project_id,
        "leadRequestId": context.lead_request_id,
        "leadReservationId": actor.reservation_id(),
        "leadActorSha256": lead_actor_sha256,
        "leadRequestSha256": context.request_sha256,
        "leadOutputSha256": actor.output_sha256(),
        "leadPrincipal": context.principal,
        "delegation": {
            "id": context.delegation.id,
            "matter": context.delegation.matter,
            "option": "APPROVE_DELEGATION",
            "optionSha256": delegation_option_sha256,
            "revision": mandate.delegation_revision(),
            "pin": mandate.delegation_pin(),
            "useContext": mandate.use_context(),
            "criterion": "no-mandatory-fee-commercial-use",
            "deadline": context.delegation.deadline,
        },
        "identity": identity,
        "identitySha256": identity_sha256,
        "licenseExpression": review.license_expression,
        "selectedBranch": review.selected_branch,
        "softwareObligations": software_obligations,
        "documentation": documentation,
        "documentationObligations": documentation_obligations,
        "softwareVerdict": software_verdict,
        "evidenceSha256": evidence_sha256,
        "identityFactsReceiptDigest": receipt_digest,
        "catalogSha256": context.binding.catalog_sha256(),
        "profileSha256": context.binding.profile_sha256(),
        "policySha256": context.binding.policy_sha256(),
        "stage": context.binding.stage(),
        "candidate": context.binding.candidate(),
        "useContext": review.use_context,
        "commercialUse": review.commercial_use,
        "paymentRequired": review.payment_required,
        "decision": review.decision,
        "revision": mandate.delegation_revision(),
    });
    let content_sha256 = canonical_json_sha256(&payload);

    Ok(VerifiedLicenseDecision {
        identity: identity.clone(),
        license_expression: review.license_expression.clone(),
        selected_branch: review.selected_branch.clone(),
        documentation,
        software_obligations,
        documentation_obligations,
        software_verdict: software_verdict.to_owned(),
        evidence_sha256,
        identity_sha256,
        identity_facts_receipt_digest: receipt_digest,
        content_sha256,
        provenance,
        use_context: review.use_context.clone(),
        commercial_use: review.commercial_use,
        payment_required: review.payment_required,
        decision: review.decision.clone(),
        revision: mandate.delegation_revision(),
        binding: context.binding.clone(),
        project_id: context.project_id.to_owned(),
        lead_request_id: context.lead_request_id.to_owned(),
        lead_reservation_id: actor.reservation_id().to_owned(),
        lead_actor_sha256,
        lead_request_sha256: context.request_sha256.to_owned(),
        lead_output_sha256: actor.output_sha256().to_owned(),
        lead_principal: context.principal.to_owned(),
        delegation_id: context.delegation.id.clone(),
        delegation_matter: context.delegation.matter.clone(),
        delegation_option: "APPROVE_DELEGATION".into(),
        delegation_option_sha256,
        delegation_revision: mandate.delegation_revision(),
        delegation_pin: mandate.delegation_pin().to_owned(),
        delegation_use_context: mandate.use_context().to_owned(),
        delegation_criterion: "no-mandatory-fee-commercial-use".into(),
        delegation_deadline: context.delegation.deadline.clone(),
    })
}

#[cfg(test)]
mod tests;
