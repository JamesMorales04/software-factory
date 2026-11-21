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
    self, BoundIdentityFacts, IDENTITY_FACTS_CONTEXT, IdentityFactsReceipt, VerifiedIdentityFacts,
};
use crate::db::{LicenseDecisionRecord, StoredEnvironmentPlan, StoredEnvironmentSpec};
use crate::planning::environment::{EnvironmentPlanStatus, EnvironmentSpecStatus};

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

/// Exact plan licence-inventory evidence bundle.
///
/// The builder and Core both canonicalise exactly `{ "entries": entries }`;
/// entries are the plan's real `licenseInventory.entries`, never a free JSON
/// object. The stage stores the canonical hash but not the entries themselves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InventoryEvidenceBundle {
    pub entries: Vec<crate::planning::environment::LicenseEntry>,
}

impl InventoryEvidenceBundle {
    pub fn from_plan(plan: &crate::planning::environment::EnvironmentPlan) -> Self {
        Self {
            entries: plan.license_inventory.entries.clone(),
        }
    }

    fn validate(&self) -> Result<(), LicenseDecisionRefusal> {
        if self.entries.is_empty() {
            return Err(LicenseDecisionRefusal::EvidenceBundleMismatch);
        }
        let mut seen = std::collections::BTreeSet::new();
        for entry in &self.entries {
            if entry.name.trim().is_empty()
                || entry.version.trim().is_empty()
                || entry.license.trim().is_empty()
                || entry.source.trim().is_empty()
                || !seen.insert((
                    entry.name.as_str(),
                    entry.version.as_str(),
                    entry.source.as_str(),
                ))
            {
                return Err(LicenseDecisionRefusal::EvidenceBundleMismatch);
            }
        }
        Ok(())
    }

    /// Canonical digest of the exact inventory entries. Public so Core can
    /// rebuild the same evidence-bundle binding when it verifies a persisted
    /// decision overlay.
    pub fn canonical_sha256(&self) -> Result<String, LicenseDecisionRefusal> {
        self.validate()?;
        let value = serde_json::to_value(self)
            .map_err(|_| LicenseDecisionRefusal::EvidenceBundleMismatch)?;
        Ok(canonical_json_sha256(&value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StageKind {
    PlanInventoryEvidence,
    SpecInventoryEvidence,
}

/// The exact real stage of the environment at decision time.
///
/// The hashes are computed by [`PolicyBinding::from_plan_inventory_evidence`] or
/// [`PolicyBinding::from_spec_inventory_evidence`] from the actual loaded
/// `StoredEnvironmentPlan`/`StoredEnvironmentSpec`, the real evidence bundle and
/// the verified identity-facts receipt. No caller-supplied hash string can
/// create this value on the authority path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageBinding {
    stage: StageKind,
    plan_sha256: String,
    plan_revision: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    spec_sha256: Option<String>,
    inventory_sha256: String,
    evidence_bundle_sha256: String,
    identity_facts_receipt_digest: String,
    /// Present only when the decision was made from a `RESOLUTION`-stage
    /// observation of an exact failed realisation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolution_key: Option<String>,
    candidate: CandidateBinding,
}

impl StageBinding {
    pub fn stage(&self) -> StageKind {
        self.stage
    }

    pub fn plan_sha256(&self) -> &str {
        &self.plan_sha256
    }

    pub fn plan_revision(&self) -> u32 {
        self.plan_revision
    }

    pub fn spec_sha256(&self) -> Option<&str> {
        self.spec_sha256.as_deref()
    }

    pub fn inventory_sha256(&self) -> &str {
        &self.inventory_sha256
    }

    pub fn evidence_bundle_sha256(&self) -> &str {
        &self.evidence_bundle_sha256
    }

    pub fn identity_facts_receipt_digest(&self) -> &str {
        &self.identity_facts_receipt_digest
    }

    pub fn candidate(&self) -> &CandidateBinding {
        &self.candidate
    }

    pub fn resolution_key(&self) -> Option<&str> {
        self.resolution_key.as_deref()
    }

    fn from_plan(
        plan: &StoredEnvironmentPlan,
        evidence_bundle: &InventoryEvidenceBundle,
        candidate: CandidateBinding,
        identity_facts: &BoundIdentityFacts,
    ) -> Result<Self, LicenseDecisionRefusal> {
        let plan_sha256 = plan.plan_sha256.clone();
        let plan_revision = plan.revision;
        let inventory_sha256 = plan.plan.license_inventory_sha256.clone();
        if !is_sha256(&plan_sha256)
            || plan_revision == 0
            || !is_sha256(&inventory_sha256)
            || plan.plan.license_inventory.inventory_sha256 != inventory_sha256
            || plan.plan.base_commit.trim().is_empty()
            || !is_commit(&plan.plan.base_commit)
            || !matches!(
                plan.plan.status,
                EnvironmentPlanStatus::CoreValidated
                    | EnvironmentPlanStatus::LeadApproved
                    | EnvironmentPlanStatus::Frozen
            )
        {
            return Err(LicenseDecisionRefusal::PolicyBindingIncomplete);
        }
        if plan.plan.license_inventory.catalog_sha256 != default_catalog_sha256()
            || plan.plan.license_inventory.profile_sha256 != default_profile_sha256()
        {
            return Err(LicenseDecisionRefusal::PolicyMismatch);
        }
        if identity_facts.binding()
            != super::identity_facts::IdentityFactsRootBinding::SpecCacheObservation
        {
            return Err(LicenseDecisionRefusal::RootBindingMismatch);
        }
        if evidence_bundle.entries != plan.plan.license_inventory.entries {
            return Err(LicenseDecisionRefusal::EvidenceBundleMismatch);
        }
        let evidence_bundle_sha256 = evidence_bundle.canonical_sha256()?;
        if evidence_bundle_sha256 == plan_sha256 || evidence_bundle_sha256 == inventory_sha256 {
            return Err(LicenseDecisionRefusal::PolicyBindingIncomplete);
        }
        validate_candidate(&candidate, &plan.plan.base_commit)?;
        if identity_facts
            .verified()
            .receipt()
            .envelope
            .challenge
            .plan_sha256
            != plan_sha256
        {
            return Err(LicenseDecisionRefusal::PlanBindingMismatch);
        }
        let challenge = &identity_facts.verified().receipt().envelope.challenge;
        let resolution_key = match challenge.stage {
            super::identity_facts::IdentityFactsStage::Resolution => {
                if challenge.spec_sha256.is_empty() {
                    Some(challenge.resolution_key.clone())
                } else {
                    return Err(LicenseDecisionRefusal::RootBindingMismatch);
                }
            }
            super::identity_facts::IdentityFactsStage::Spec => None,
        };
        let receipt_digest = identity_facts_receipt_digest(identity_facts.verified());
        Ok(Self {
            stage: StageKind::PlanInventoryEvidence,
            plan_sha256,
            plan_revision,
            spec_sha256: None,
            inventory_sha256,
            evidence_bundle_sha256,
            identity_facts_receipt_digest: receipt_digest,
            resolution_key,
            candidate,
        })
    }

    fn from_spec(
        plan: &StoredEnvironmentPlan,
        spec: &StoredEnvironmentSpec,
        evidence_bundle: &InventoryEvidenceBundle,
        candidate: CandidateBinding,
        identity_facts: &BoundIdentityFacts,
    ) -> Result<Self, LicenseDecisionRefusal> {
        let plan_sha256 = plan.plan_sha256.clone();
        let plan_revision = plan.revision;
        let spec_sha256 = spec.spec_sha256.clone();
        let inventory_sha256 = plan.plan.license_inventory_sha256.clone();
        if !is_sha256(&plan_sha256)
            || plan_revision == 0
            || !is_sha256(&inventory_sha256)
            || !is_sha256(&spec_sha256)
            || plan.plan.license_inventory.inventory_sha256 != inventory_sha256
            || spec.spec.plan_sha256 != plan_sha256
            || spec.spec.license_inventory_sha256 != inventory_sha256
            || spec.spec.base_commit != plan.plan.base_commit
            || spec.spec.status != EnvironmentSpecStatus::Frozen
            || spec_sha256 == inventory_sha256
            || spec_sha256 == plan_sha256
        {
            return Err(LicenseDecisionRefusal::PolicyBindingIncomplete);
        }
        if plan.plan.license_inventory.catalog_sha256 != default_catalog_sha256()
            || plan.plan.license_inventory.profile_sha256 != default_profile_sha256()
        {
            return Err(LicenseDecisionRefusal::PolicyMismatch);
        }
        if identity_facts.binding() != super::identity_facts::IdentityFactsRootBinding::BuildReceipt
        {
            return Err(LicenseDecisionRefusal::RootBindingMismatch);
        }
        if evidence_bundle.entries != plan.plan.license_inventory.entries {
            return Err(LicenseDecisionRefusal::EvidenceBundleMismatch);
        }
        let evidence_bundle_sha256 = evidence_bundle.canonical_sha256()?;
        if evidence_bundle_sha256 == plan_sha256
            || evidence_bundle_sha256 == inventory_sha256
            || evidence_bundle_sha256 == spec_sha256
        {
            return Err(LicenseDecisionRefusal::PolicyBindingIncomplete);
        }
        validate_candidate(&candidate, &plan.plan.base_commit)?;
        let receipt = identity_facts.verified().receipt();
        if receipt.envelope.challenge.plan_sha256 != plan_sha256
            || receipt.envelope.challenge.spec_sha256 != spec_sha256
        {
            return Err(LicenseDecisionRefusal::SpecBindingMismatch);
        }
        Ok(Self {
            stage: StageKind::SpecInventoryEvidence,
            plan_sha256,
            plan_revision,
            spec_sha256: Some(spec_sha256),
            inventory_sha256,
            evidence_bundle_sha256,
            identity_facts_receipt_digest: identity_facts_receipt_digest(identity_facts.verified()),
            resolution_key: None,
            candidate,
        })
    }
}

fn validate_candidate(
    candidate: &CandidateBinding,
    plan_base_commit: &str,
) -> Result<(), LicenseDecisionRefusal> {
    candidate.validate()?;
    match candidate {
        CandidateBinding::Base { base_commit } if base_commit == plan_base_commit => Ok(()),
        CandidateBinding::Base { .. } => Err(LicenseDecisionRefusal::PolicyBindingIncomplete),
        CandidateBinding::Candidate { .. } => {
            // A free 64-hex string is not a candidate. The verified candidate
            // loader is not part of the current pre-candidate stage; fail closed
            // until a real candidate record can be bound and revalidated.
            Err(LicenseDecisionRefusal::CandidateUnverified)
        }
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
    /// Bind a pre-spec decision to the real retained plan, its real licence
    /// inventory, the real evidence bundle and the HMAC-verified receipt.
    #[allow(clippy::too_many_arguments)]
    pub fn from_plan_inventory_evidence(
        catalog_sha256: impl Into<String>,
        profile_sha256: impl Into<String>,
        policy_sha256: impl Into<String>,
        plan: &StoredEnvironmentPlan,
        evidence_bundle: &InventoryEvidenceBundle,
        candidate: CandidateBinding,
        identity_facts: &BoundIdentityFacts,
    ) -> Result<Self, LicenseDecisionRefusal> {
        let stage = StageBinding::from_plan(plan, evidence_bundle, candidate, identity_facts)?;
        Self::from_stage(catalog_sha256, profile_sha256, policy_sha256, stage)
    }

    /// Bind a decision to a real frozen spec and its real inventory. A
    /// spec-required consumer must not use this before a spec row exists.
    #[allow(clippy::too_many_arguments)]
    pub fn from_spec_inventory_evidence(
        catalog_sha256: impl Into<String>,
        profile_sha256: impl Into<String>,
        policy_sha256: impl Into<String>,
        plan: &StoredEnvironmentPlan,
        spec: &StoredEnvironmentSpec,
        evidence_bundle: &InventoryEvidenceBundle,
        candidate: CandidateBinding,
        identity_facts: &BoundIdentityFacts,
    ) -> Result<Self, LicenseDecisionRefusal> {
        let stage =
            StageBinding::from_spec(plan, spec, evidence_bundle, candidate, identity_facts)?;
        Self::from_stage(catalog_sha256, profile_sha256, policy_sha256, stage)
    }

    fn from_stage(
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
        if policy_sha256 != default_binding_sha256() {
            return Err(LicenseDecisionRefusal::PolicyMismatch);
        }
        Ok(Self {
            catalog_sha256,
            profile_sha256,
            policy_sha256,
            stage,
        })
    }

    #[cfg(test)]
    pub(crate) fn test_binding(
        plan_sha256: &str,
        plan_revision: u32,
        spec_sha256: Option<&str>,
        inventory_sha256: &str,
        evidence_bundle_sha256: &str,
        candidate: CandidateBinding,
        identity_facts_receipt_digest: &str,
    ) -> Self {
        Self {
            catalog_sha256: default_catalog_sha256().to_owned(),
            profile_sha256: default_profile_sha256().to_owned(),
            policy_sha256: default_binding_sha256().to_owned(),
            stage: StageBinding {
                stage: if spec_sha256.is_some() {
                    StageKind::SpecInventoryEvidence
                } else {
                    StageKind::PlanInventoryEvidence
                },
                plan_sha256: plan_sha256.to_owned(),
                plan_revision,
                spec_sha256: spec_sha256.map(str::to_owned),
                inventory_sha256: inventory_sha256.to_owned(),
                evidence_bundle_sha256: evidence_bundle_sha256.to_owned(),
                identity_facts_receipt_digest: identity_facts_receipt_digest.to_owned(),
                resolution_key: None,
                candidate,
            },
        }
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

    pub fn stage_json(&self) -> String {
        serde_json::to_string(&self.stage).expect("binding stage is serializable")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SoftwareTextEvidence {
    pub path: String,
    pub license: String,
    pub sha256: String,
}

/// One documentation output and its exact licence decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DocumentationReview {
    pub path: String,
    pub license: String,
    /// SHA-256 of the exact delivered documentation output file.
    pub output_sha256: String,
    /// Explicit path of the licensing text used for this document.
    pub license_text_path: String,
    pub license_text_sha256: String,
    pub obligations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
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
    #[serde(default)]
    pub software_texts: Vec<SoftwareTextEvidence>,
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
    pub output_sha256: String,
    pub license_text_path: String,
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
    EvidenceBundleMismatch,
    CandidateUnverified,
    SourceEvidenceMismatch,
    RootBindingMismatch,
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
            Self::EvidenceBundleMismatch => formatter
                .write_str("the evidence bundle is not the exact plan licence inventory entries"),
            Self::CandidateUnverified => formatter.write_str(
                "a free candidate hash is not a verified candidate; only the plan base is accepted",
            ),
            Self::SourceEvidenceMismatch => formatter.write_str(
                "the fixed-output source proof does not match the exact source/output evidence",
            ),
            Self::RootBindingMismatch => formatter.write_str(
                "the identity-facts observed root does not match the required stage root binding",
            ),
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

fn receipt_fact(
    facts: &[super::identity_facts::IdentityFact],
    store_path: &str,
    deriver: &str,
    nar_hash: &str,
) -> Result<(), LicenseDecisionRefusal> {
    let fact = facts
        .iter()
        .find(|fact| fact.store_path == store_path)
        .ok_or(LicenseDecisionRefusal::ReceiptMismatch)?;
    if fact.deriver.as_deref() != Some(deriver)
        || fact.nar_hash != nar_hash
        || !fact.outputs.iter().any(|output| output == store_path)
    {
        return Err(LicenseDecisionRefusal::ReceiptMismatch);
    }
    Ok(())
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

/// True when the verified fixed-output proof is exactly the source/output pair
/// recorded for one shared licence asset identity. Core applies this both
/// before dispatch (so an impossible proof never spends a model call) and
/// after the review, so the two checks cannot drift.
pub(crate) fn source_matches_asset_identity(
    source: &VerifiedSourceEvidence,
    identity: &serde_json::Value,
) -> bool {
    let source_doc = identity.get("source").unwrap_or(&serde_json::Value::Null);
    let output_doc = identity.get("output").unwrap_or(&serde_json::Value::Null);
    let asset_evidence = source_asset()
        .get("evidence")
        .unwrap_or(&serde_json::Value::Null);
    exact(output_doc, "drvPath") == Some(source.output_drv_path())
        && exact(output_doc, "storePath") == Some(source.output_store_path())
        && exact(source_doc, "drvPath") == Some(source.source_drv_path())
        && exact(source_doc, "storePath") == Some(source.source_store_path())
        && exact(source_doc, "narHash") == Some(source.nar_sri())
        && source_doc
            .get("narSize")
            .and_then(serde_json::Value::as_u64)
            == Some(source.nar_size())
        && exact(source_doc, "signature") == Some(source.signature())
        && exact(asset_evidence, "sourceFlatSha256") == Some(source.flat_sha256())
        && exact(asset_evidence, "outputDrvAtermSha256") == Some(source.output_drv_aterm_sha256())
        && exact(asset_evidence, "sourceDrvAtermSha256") == Some(source.source_drv_aterm_sha256())
}

/// True when the verified source proof belongs to at least one shared asset
/// identity under the plan pin. The producer refuses a call otherwise.
pub(crate) fn source_evidence_belongs_to_pin(source: &VerifiedSourceEvidence, pin: &str) -> bool {
    source_asset()
        .get("identities")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|identities| {
            identities.iter().any(|identity| {
                exact(identity, "pin") == Some(pin)
                    && source_matches_asset_identity(source, identity)
            })
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
        || receipt.envelope.challenge.stage != stored.stage
        || receipt.envelope.challenge.resolution_key != stored.resolution_key
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum Expr {
    Atom(String),
    With(String, String),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

fn tokenize_expression(expression: &str) -> Result<Vec<String>, LicenseDecisionRefusal> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for character in expression.chars() {
        match character {
            '(' | ')' => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                tokens.push(character.to_string());
            }
            character if character.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            character => current.push(character),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    if tokens.is_empty() {
        return Err(LicenseDecisionRefusal::InvalidExpression);
    }
    Ok(tokens)
}

fn is_operator(token: &str) -> bool {
    matches!(token.to_ascii_uppercase().as_str(), "AND" | "OR" | "WITH")
}

struct ExpressionParser {
    tokens: Vec<String>,
    position: usize,
}

impl ExpressionParser {
    fn new(expression: &str) -> Result<Self, LicenseDecisionRefusal> {
        Ok(Self {
            tokens: tokenize_expression(expression)?,
            position: 0,
        })
    }

    fn parse(mut self) -> Result<Expr, LicenseDecisionRefusal> {
        let expression = self.parse_or()?;
        if self.position != self.tokens.len() {
            return Err(LicenseDecisionRefusal::InvalidExpression);
        }
        Ok(expression)
    }

    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.position).map(String::as_str)
    }

    fn take(&mut self, expected: &str) -> bool {
        if self
            .peek()
            .is_some_and(|token| token.eq_ignore_ascii_case(expected))
        {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn parse_or(&mut self) -> Result<Expr, LicenseDecisionRefusal> {
        let mut parts = vec![self.parse_and()?];
        while self.take("OR") {
            parts.push(self.parse_and()?);
        }
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            Expr::Or(parts)
        })
    }

    fn parse_and(&mut self) -> Result<Expr, LicenseDecisionRefusal> {
        let mut parts = vec![self.parse_with()?];
        while self.take("AND") {
            parts.push(self.parse_with()?);
        }
        Ok(if parts.len() == 1 {
            parts.remove(0)
        } else {
            Expr::And(parts)
        })
    }

    fn parse_with(&mut self) -> Result<Expr, LicenseDecisionRefusal> {
        let base = self.parse_atom()?;
        if self.take("WITH") {
            let exception = self.parse_atom()?;
            let Expr::Atom(exception) = exception else {
                return Err(LicenseDecisionRefusal::InvalidExpression);
            };
            let Expr::Atom(base) = base else {
                return Err(LicenseDecisionRefusal::InvalidExpression);
            };
            Ok(Expr::With(base, exception))
        } else {
            Ok(base)
        }
    }

    fn parse_atom(&mut self) -> Result<Expr, LicenseDecisionRefusal> {
        if self.take("(") {
            let expression = self.parse_or()?;
            if !self.take(")") {
                return Err(LicenseDecisionRefusal::InvalidExpression);
            }
            return Ok(expression);
        }
        let Some(token) = self.peek().map(str::to_owned) else {
            return Err(LicenseDecisionRefusal::InvalidExpression);
        };
        if is_operator(&token) || token == ")" {
            return Err(LicenseDecisionRefusal::InvalidExpression);
        }
        self.position += 1;
        Ok(Expr::Atom(token))
    }
}

fn select_branches(expression: &Expr) -> Vec<String> {
    match expression {
        Expr::Atom(atom) => vec![atom.clone()],
        Expr::With(base, exception) => vec![format!("{base} WITH {exception}")],
        Expr::Or(parts) => parts.iter().flat_map(select_branches).collect(),
        Expr::And(parts) => {
            let mut branches = vec![String::new()];
            for part in parts {
                let alternatives = select_branches(part);
                let mut next = Vec::new();
                for prefix in &branches {
                    for alternative in &alternatives {
                        if prefix.is_empty() {
                            next.push(alternative.clone());
                        } else {
                            next.push(format!("{prefix} AND {alternative}"));
                        }
                    }
                }
                branches = next;
            }
            branches
        }
    }
}

/// Selectable branches of a real SPDX `AND`/`OR`/`WITH` expression.
///
/// `OR` yields alternatives. `AND` keeps every conjunct in each branch, so
/// choosing one atom of a conjunction is never a valid branch. `WITH` is one
/// branch of its base plus exception.
pub fn selectable_branches(expression: &str) -> Result<Vec<String>, LicenseDecisionRefusal> {
    let parsed = ExpressionParser::new(expression)?.parse()?;
    let branches = select_branches(&parsed);
    if branches.is_empty() {
        return Err(LicenseDecisionRefusal::InvalidExpression);
    }
    Ok(branches)
}

fn branch_atoms(branch: &str) -> Result<Vec<String>, LicenseDecisionRefusal> {
    let parsed = ExpressionParser::new(branch)?.parse()?;
    if matches!(parsed, Expr::Or(_)) {
        return Err(LicenseDecisionRefusal::InvalidExpression);
    }
    let mut atoms = Vec::new();
    fn collect(expression: &Expr, atoms: &mut Vec<String>) -> Result<(), LicenseDecisionRefusal> {
        match expression {
            Expr::Atom(atom) => {
                atoms.push(atom.clone());
                Ok(())
            }
            Expr::With(base, _) => {
                atoms.push(base.clone());
                Ok(())
            }
            Expr::And(parts) => {
                for part in parts {
                    collect(part, atoms)?;
                }
                Ok(())
            }
            Expr::Or(_) => Err(LicenseDecisionRefusal::InvalidExpression),
        }
    }
    collect(&parsed, &mut atoms)?;
    Ok(atoms)
}

/// Exact obligations for one selected branch: the union of every conjunct's
/// catalogue obligations, preserving first occurrence.
pub fn branch_obligations(branch: &str) -> Result<Vec<String>, LicenseDecisionRefusal> {
    let atoms = branch_atoms(branch)?;
    let mut obligations = Vec::new();
    for atom in atoms {
        let atom_obligations =
            catalogue_obligations(&atom).ok_or(LicenseDecisionRefusal::SoftwareUnknown)?;
        for obligation in atom_obligations {
            if !obligations.contains(&obligation) {
                obligations.push(obligation);
            }
        }
    }
    if obligations.is_empty() {
        return Err(LicenseDecisionRefusal::SoftwareUnknown);
    }
    Ok(obligations)
}

fn branch_permits_commercial_no_fee(branch: &str) -> Result<bool, LicenseDecisionRefusal> {
    for atom in branch_atoms(branch)? {
        let entry = catalogue_entry(&atom).ok_or(LicenseDecisionRefusal::SoftwareUnknown)?;
        if !family_permits_commercial_no_fee(&entry.family) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn exact_software_obligations(
    expression: &str,
    selected_branch: &str,
) -> Result<Vec<String>, LicenseDecisionRefusal> {
    let branches = selectable_branches(expression)?;
    if !branches.iter().any(|branch| branch == selected_branch) {
        return Err(LicenseDecisionRefusal::BranchNotInExpression);
    }
    branch_obligations(selected_branch)
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
    if expression != identity_license {
        return Err(LicenseDecisionRefusal::SoftwareDenied);
    }
    if !selectable_branches(expression)?
        .iter()
        .any(|branch| branch == selected_branch)
    {
        return Err(LicenseDecisionRefusal::BranchNotInExpression);
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
    let atoms = branch_atoms(selected_branch)?;
    let mut saw_copyleft = false;
    for atom in atoms {
        let entry = catalogue_entry(&atom).ok_or(LicenseDecisionRefusal::SoftwareUnknown)?;
        if entry.state != "active" || entry.obligations.is_empty() {
            return Err(LicenseDecisionRefusal::SoftwareDenied);
        }
        if entry.family == "source-available" {
            return Err(LicenseDecisionRefusal::SoftwareDenied);
        }
        if family_is_copyleft(&entry.family) {
            saw_copyleft = true;
        } else if !family_permits_commercial_no_fee(&entry.family) {
            return Err(LicenseDecisionRefusal::SoftwareDenied);
        }
    }
    if !saw_copyleft {
        return Err(LicenseDecisionRefusal::SoftwareDenied);
    }
    Ok(())
}

/// Separate exact fixed-output source proof input.
///
/// This is the untrusted DTO Core/Builders construct from the persisted source
/// recovery. Only [`VerifiedSourceEvidence`] can be used by the decision
/// verifier; the constructor derives Nix base32/fingerprint/CA from the bytes
/// and verifies the pinned Ed25519 cache signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEvidenceProof {
    pub source_store_path: String,
    pub source_drv_path: String,
    pub flat_sha256: String,
    pub nar_sri: String,
    pub nar_size: u64,
    pub signature: String,
    pub output_drv_path: String,
    pub output_store_path: String,
    /// Exact `inputDrvs` entries of the output derivation, from the real drv.
    pub output_drv_input_drvs: Vec<String>,
    /// SHA-256 of the exact output derivation ATerm bytes.
    pub output_drv_aterm_sha256: String,
    /// SHA-256 of the exact fixed-output source derivation ATerm bytes.
    pub source_drv_aterm_sha256: String,
    /// Optional asserted derived values; if present they must match the
    /// internally derived values.
    pub nar_nix32: Option<String>,
    pub fingerprint: Option<String>,
    pub ca: Option<String>,
}

const NIX_CACHE_KEY_NAME: &str = "cache.nixos.org-1";
const NIX_CACHE_PUBLIC_KEY: &str = "6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=";

/// Source proof that passed pinned Ed25519 verification and derived all Nix
/// base32/fingerprint/CA values. Fields are private; only [`Self::verify`] can
/// create it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSourceEvidence {
    source_store_path: String,
    source_drv_path: String,
    flat_sha256: String,
    nar_sri: String,
    nar_nix32: String,
    nar_size: u64,
    signature: String,
    fingerprint: String,
    ca: String,
    output_drv_path: String,
    output_store_path: String,
    output_drv_input_drvs: Vec<String>,
    output_drv_aterm_sha256: String,
    source_drv_aterm_sha256: String,
}

impl VerifiedSourceEvidence {
    pub fn verify(proof: &SourceEvidenceProof) -> Result<Self, LicenseDecisionRefusal> {
        use base64::Engine as _;
        use ring::signature::{ED25519, UnparsedPublicKey};
        let invalid = || LicenseDecisionRefusal::SourceEvidenceMismatch;
        if !is_store_path(&proof.source_store_path)
            || !proof.source_drv_path.starts_with("/nix/store/")
            || !proof.source_drv_path.ends_with(".drv")
            || !proof.output_drv_path.starts_with("/nix/store/")
            || !proof.output_drv_path.ends_with(".drv")
            || !is_store_path(&proof.output_store_path)
            || !is_sha256(&proof.flat_sha256)
            || !is_sha256(&proof.output_drv_aterm_sha256)
            || !is_sha256(&proof.source_drv_aterm_sha256)
            || proof.nar_size == 0
            || proof.output_drv_input_drvs.is_empty()
        {
            return Err(invalid());
        }
        let mut seen = BTreeSet::new();
        if proof.output_drv_input_drvs.iter().any(|drv| {
            !drv.starts_with("/nix/store/") || !drv.ends_with(".drv") || !seen.insert(drv)
        }) || !seen.contains(&proof.source_drv_path)
        {
            return Err(invalid());
        }
        let nar_bytes = proof
            .nar_sri
            .strip_prefix("sha256-")
            .and_then(|encoded| {
                base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .ok()
            })
            .filter(|bytes| bytes.len() == 32)
            .ok_or_else(invalid)?;
        let flat_bytes = hex_decode(&proof.flat_sha256).ok_or_else(invalid)?;
        let nar_nix32 = nix_base32(&nar_bytes);
        let flat_nix32 = nix_base32(&flat_bytes);
        let fingerprint = format!(
            "1;{};sha256:{};{};",
            proof.source_store_path, nar_nix32, proof.nar_size
        );
        let ca = format!("fixed:sha256:{flat_nix32}");
        if proof
            .nar_nix32
            .as_deref()
            .is_some_and(|value| value != nar_nix32)
            || proof
                .fingerprint
                .as_deref()
                .is_some_and(|value| value != fingerprint)
            || proof.ca.as_deref().is_some_and(|value| value != ca)
        {
            return Err(invalid());
        }
        let (signature_name, signature_b64) =
            proof.signature.split_once(':').ok_or_else(invalid)?;
        if signature_name != NIX_CACHE_KEY_NAME {
            return Err(invalid());
        }
        let signature = base64::engine::general_purpose::STANDARD
            .decode(signature_b64)
            .map_err(|_| invalid())?;
        let public_key = base64::engine::general_purpose::STANDARD
            .decode(NIX_CACHE_PUBLIC_KEY)
            .map_err(|_| invalid())?;
        if signature.len() != 64 || public_key.len() != 32 {
            return Err(invalid());
        }
        UnparsedPublicKey::new(&ED25519, public_key)
            .verify(fingerprint.as_bytes(), &signature)
            .map_err(|_| invalid())?;
        Ok(Self {
            source_store_path: proof.source_store_path.clone(),
            source_drv_path: proof.source_drv_path.clone(),
            flat_sha256: proof.flat_sha256.clone(),
            nar_sri: proof.nar_sri.clone(),
            nar_nix32,
            nar_size: proof.nar_size,
            signature: proof.signature.clone(),
            fingerprint,
            ca,
            output_drv_path: proof.output_drv_path.clone(),
            output_store_path: proof.output_store_path.clone(),
            output_drv_input_drvs: proof.output_drv_input_drvs.clone(),
            output_drv_aterm_sha256: proof.output_drv_aterm_sha256.clone(),
            source_drv_aterm_sha256: proof.source_drv_aterm_sha256.clone(),
        })
    }

    /// Reconstruct a proof from its canonical **verified** representation. The
    /// signature, derived values and exact inputDrvs membership are re-verified
    /// and the representation must round-trip byte-for-byte, so a persisted
    /// proof cannot drop or add a fact.
    pub fn from_verified_json(value: &serde_json::Value) -> Result<Self, LicenseDecisionRefusal> {
        let text = |key: &str| {
            value
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .ok_or(LicenseDecisionRefusal::SourceEvidenceMismatch)
        };
        let drvs = value
            .get("outputDrvInputDrvs")
            .and_then(serde_json::Value::as_array)
            .ok_or(LicenseDecisionRefusal::SourceEvidenceMismatch)?
            .iter()
            .map(|entry| {
                entry
                    .as_str()
                    .map(str::to_owned)
                    .ok_or(LicenseDecisionRefusal::SourceEvidenceMismatch)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let proof = SourceEvidenceProof {
            source_store_path: text("sourceStorePath")?,
            source_drv_path: text("sourceDrvPath")?,
            flat_sha256: text("flatSha256")?,
            nar_sri: text("narSri")?,
            nar_size: value
                .get("narSize")
                .and_then(serde_json::Value::as_u64)
                .ok_or(LicenseDecisionRefusal::SourceEvidenceMismatch)?,
            signature: text("signature")?,
            output_drv_path: text("outputDrvPath")?,
            output_store_path: text("outputStorePath")?,
            output_drv_input_drvs: drvs,
            output_drv_aterm_sha256: text("outputDrvAtermSha256")?,
            source_drv_aterm_sha256: text("sourceDrvAtermSha256")?,
            nar_nix32: Some(text("narNix32")?),
            fingerprint: Some(text("fingerprint")?),
            ca: Some(text("ca")?),
        };
        let verified = Self::verify(&proof)?;
        if verified.to_json() != *value {
            return Err(LicenseDecisionRefusal::SourceEvidenceMismatch);
        }
        Ok(verified)
    }

    pub fn source_store_path(&self) -> &str {
        &self.source_store_path
    }

    pub fn source_drv_path(&self) -> &str {
        &self.source_drv_path
    }

    pub fn flat_sha256(&self) -> &str {
        &self.flat_sha256
    }

    pub fn nar_sri(&self) -> &str {
        &self.nar_sri
    }

    pub fn nar_nix32(&self) -> &str {
        &self.nar_nix32
    }

    pub fn nar_size(&self) -> u64 {
        self.nar_size
    }

    pub fn signature(&self) -> &str {
        &self.signature
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn ca(&self) -> &str {
        &self.ca
    }

    pub fn output_drv_path(&self) -> &str {
        &self.output_drv_path
    }

    pub fn output_store_path(&self) -> &str {
        &self.output_store_path
    }

    pub fn output_drv_input_drvs(&self) -> &[String] {
        &self.output_drv_input_drvs
    }

    pub fn output_drv_aterm_sha256(&self) -> &str {
        &self.output_drv_aterm_sha256
    }

    pub fn source_drv_aterm_sha256(&self) -> &str {
        &self.source_drv_aterm_sha256
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "sourceStorePath": self.source_store_path,
            "sourceDrvPath": self.source_drv_path,
            "flatSha256": self.flat_sha256,
            "narSri": self.nar_sri,
            "narNix32": self.nar_nix32,
            "narSize": self.nar_size,
            "signature": self.signature,
            "fingerprint": self.fingerprint,
            "ca": self.ca,
            "outputDrvPath": self.output_drv_path,
            "outputStorePath": self.output_store_path,
            "outputDrvInputDrvs": self.output_drv_input_drvs,
            "outputDrvAtermSha256": self.output_drv_aterm_sha256,
            "sourceDrvAtermSha256": self.source_drv_aterm_sha256,
        })
    }

    pub fn canonical_sha256(&self) -> String {
        canonical_json_sha256(&self.to_json())
    }
}

fn is_store_path(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("/nix/store/") else {
        return false;
    };
    let Some((hash, name)) = rest.split_once('-') else {
        return false;
    };
    hash.len() == 32
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && !name.is_empty()
        && !name.contains('/')
}

fn hex_decode(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for chunk in value.as_bytes().chunks(2) {
        let text = std::str::from_utf8(chunk).ok()?;
        bytes.push(u8::from_str_radix(text, 16).ok()?);
    }
    Some(bytes)
}

fn nix_base32(bytes: &[u8]) -> String {
    const CHARS: &[u8; 32] = b"0123456789abcdfghijklmnpqrsvwxyz";
    let length = bytes.len().saturating_mul(8).div_ceil(5);
    let mut output = String::with_capacity(length);
    for index in (0..length).rev() {
        let bit = index * 5;
        let byte = bit / 8;
        let shift = bit % 8;
        let mut value = u16::from(bytes[byte]) >> shift;
        if byte + 1 < bytes.len() {
            value |= u16::from(bytes[byte + 1]) << (8 - shift);
        }
        output.push(CHARS[(value & 0x1f) as usize] as char);
    }
    output
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
    pub identity_facts: &'a BoundIdentityFacts,
    pub source_evidence: &'a VerifiedSourceEvidence,
    pub binding: &'a PolicyBinding,
}

/// Validate one model review against the exact shared evidence, the identity
/// receipt, the Lead actor, the Owner delegation and the current policy binding.
pub fn verify_license_evidence_review(
    review: &LicenseDecisionReview,
    context: &LicenseDecisionContext<'_>,
) -> Result<VerifiedLicenseDecision, LicenseDecisionRefusal> {
    verify_against_source_asset(review, context, source_asset())
}

fn verify_against_source_asset(
    review: &LicenseDecisionReview,
    context: &LicenseDecisionContext<'_>,
    asset: &serde_json::Value,
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
        != identity_facts_receipt_digest(context.identity_facts.verified())
    {
        return Err(LicenseDecisionRefusal::ReceiptDigestMismatch);
    }
    let receipt = context.identity_facts.verified().receipt();
    if receipt.envelope.context != IDENTITY_FACTS_CONTEXT
        || receipt.envelope.challenge.pin != review.pin
    {
        return Err(LicenseDecisionRefusal::ReceiptMismatch);
    }
    match (
        context.binding.stage().spec_sha256(),
        context.identity_facts.binding(),
    ) {
        (None, super::identity_facts::IdentityFactsRootBinding::SpecCacheObservation) => {}
        (Some(_), super::identity_facts::IdentityFactsRootBinding::BuildReceipt) => {}
        _ => return Err(LicenseDecisionRefusal::RootBindingMismatch),
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
        CandidateBinding::Candidate { .. } => {
            // A free 64-hex string is not a candidate. The verified candidate
            // loader is not part of the current pre-candidate stage; fail closed
            // until a real candidate record can be bound and revalidated.
            return Err(LicenseDecisionRefusal::CandidateUnverified);
        }
        CandidateBinding::Base { base_commit } => {
            if base_commit != context.base_commit {
                return Err(LicenseDecisionRefusal::PolicyBindingIncomplete);
            }
        }
    }

    // Exact identity and canonical shared asset.
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
    // The HMAC receipt must contain the exact reviewed output fact. A
    // plan-wide receipt may contain many facts and its observed root may be the
    // plan root; only exact membership is required here. The source is a
    // separate fixed-output proof, because the fetched source is not guaranteed
    // to be one of the realised environment manifest facts.
    receipt_fact(
        &receipt.envelope.manifest.facts,
        &review.output_store_path,
        &review.output_drv_path,
        &review.output_nar_hash,
    )?;
    let source = context.source_evidence;
    if !source_matches_asset_identity(source, identity)
        || source.source_store_path() != review.source_store_path
        || source.source_drv_path() != review.source_drv_path
        || source.output_drv_path() != review.output_drv_path
        || source.output_store_path() != review.output_store_path
    {
        return Err(LicenseDecisionRefusal::SourceEvidenceMismatch);
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
    // Validate the AST selection and exact obligations before reading text
    // evidence, so an invalid branch cannot be disguised as a text mismatch.
    let software_obligations =
        exact_software_obligations(&review.license_expression, &review.selected_branch)?;
    if review.software_obligations != software_obligations {
        return Err(LicenseDecisionRefusal::ObligationsMismatch);
    }

    let license_entries = license_evidence
        .as_array()
        .ok_or(LicenseDecisionRefusal::AssetCanonicalMismatch)?;
    let selected_atoms = branch_atoms(&review.selected_branch).unwrap_or_default();
    if selected_atoms.is_empty() {
        return Err(LicenseDecisionRefusal::BranchNotInExpression);
    }
    let mut text_by_path: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    let mut text_by_license: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for entry in license_entries {
        let sha = exact(entry, "sha256").ok_or(LicenseDecisionRefusal::AssetCanonicalMismatch)?;
        if !is_sha256(sha) {
            return Err(LicenseDecisionRefusal::AssetCanonicalMismatch);
        }
        if let (Some(path), Some(license)) = (exact(entry, "path"), exact(entry, "license"))
            && exact(entry, "purpose") == Some("license-text")
        {
            text_by_path.insert(path, (license, sha));
            text_by_license.entry(license).or_default().insert(sha);
        }
    }
    let mut seen_texts = BTreeSet::new();
    for text in &review.software_texts {
        if !seen_texts.insert(text.sha256.as_str()) {
            return Err(LicenseDecisionRefusal::LicenseTextDigest);
        }
        let Some((license, sha)) = text_by_path.get(text.path.as_str()) else {
            return Err(LicenseDecisionRefusal::LicenseTextDigest);
        };
        if *license != text.license || *sha != text.sha256 {
            return Err(LicenseDecisionRefusal::LicenseTextDigest);
        }
    }
    for atom in &selected_atoms {
        let covered = review.software_texts.iter().any(|text| {
            text_license_matches(&text.license, atom)
                && text_by_path
                    .get(text.path.as_str())
                    .is_some_and(|(_, sha)| *sha == text.sha256)
        }) || (selected_atoms.len() == 1
            && review.software_texts.is_empty()
            && text_by_license.iter().any(|(license, digests)| {
                text_license_matches(license, atom)
                    && digests.contains(review.software_text_sha256.as_str())
            }));
        if !covered {
            return Err(LicenseDecisionRefusal::LicenseTextDigest);
        }
    }
    let trusted_text = true;
    for atom in &selected_atoms {
        let licensing_fact = license_entries.iter().any(|entry| {
            exact(entry, "role") == Some("licensing")
                && exact(entry, "license") == Some(atom.as_str())
        });
        if !licensing_fact {
            return Err(LicenseDecisionRefusal::MissingLicensingFact);
        }
    }

    let commercial_ok = review.commercial_use
        && !review.payment_required
        && branch_permits_commercial_no_fee(&review.selected_branch)?;
    let expression_decision = evaluate(
        &review.license_expression,
        LicenseContext::Closure,
        default_catalog(),
        default_profile(),
    );
    let software_verdict = match evaluate(
        &review.selected_branch,
        LicenseContext::Closure,
        default_catalog(),
        default_profile(),
    ) {
        LicenseDecision::Allowed => "ALLOWED",
        LicenseDecision::Unknown => "UNKNOWN_CLASSIFIED",
        LicenseDecision::Denied => {
            if expression_decision != LicenseDecision::Denied {
                return Err(LicenseDecisionRefusal::SoftwareDenied);
            }
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
                exact(entry, "sha256").unwrap_or_default().to_owned(),
            )
        })
        .collect::<BTreeSet<_>>();
    let evidence_by_path = license_entries
        .iter()
        .filter_map(|entry| {
            Some((
                exact(entry, "path")?,
                (
                    exact(entry, "license").unwrap_or_default(),
                    exact(entry, "sha256").unwrap_or_default(),
                ),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut actual_docs = BTreeSet::new();
    let mut documentation = Vec::new();
    let mut documentation_obligation_set = BTreeSet::new();
    for document in &review.documentation {
        if !actual_docs.insert((
            document.path.clone(),
            document.license.clone(),
            document.output_sha256.clone(),
        )) {
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
        let trusted = evidence_by_path
            .get(document.license_text_path.as_str())
            .is_some_and(|(license, digest)| {
                *license == document.license && *digest == document.license_text_sha256
            });
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
            output_sha256: document.output_sha256.clone(),
            license_text_path: document.license_text_path.clone(),
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

    // HYBRID decision: the Lead asserts the commercial/no-mandatory-fee reading
    // of the exact authenticated licence text, while Core deterministically
    // enforces the current catalogue family, exact obligations, denied IDs,
    // stage, actor, delegation and expiry limits. This is not a legal opinion or
    // a deterministic proof of the text's legal interpretation.
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

    let receipt_digest = identity_facts_receipt_digest(context.identity_facts.verified());
    let source_evidence_digest = source.canonical_sha256();
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
        "sourceEvidence": source.to_json(),
        "sourceEvidenceSha256": source_evidence_digest,
        "softwareTexts": review.software_texts,
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
        "softwareTexts": review.software_texts,
        "documentation": documentation,
        "documentationObligations": documentation_obligations,
        "softwareVerdict": software_verdict,
        "evidenceSha256": evidence_sha256,
        "identityFactsReceiptDigest": receipt_digest,
        "sourceEvidence": source.to_json(),
        "sourceEvidenceSha256": source_evidence_digest,
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

/// Exact context a persisted decision must still satisfy before it can be
/// delivered to the builder's license overlay. Core supplies the current plan
/// and, when it exists, the frozen spec.
pub struct LicenseDecisionOverlayContext<'a> {
    pub project_id: &'a str,
    pub plan_sha256: &'a str,
    pub plan_revision: u32,
    pub spec_sha256: Option<&'a str>,
    pub inventory_sha256: &'a str,
    pub evidence_bundle_sha256: &'a str,
    pub base_commit: &'a str,
    pub delegation_revoked: bool,
    /// The current verified identity-facts receipt. The decision must be the
    /// exact decision made from this receipt, not merely the same plan.
    pub identity_facts: &'a BoundIdentityFacts,
    pub now: chrono::DateTime<chrono::Utc>,
}

/// A persisted ACCEPT decision re-verified against the exact current plan,
/// policy, delegation lifetime, source proof and identity. Its field is
/// private: only [`verify_license_decision_overlay`] constructs it, and the
/// broker overlay is emitted from the verified value rather than from raw DB
/// JSON. This is an execution overlay, never a second approval path.
pub struct VerifiedLicenseDecisionOverlay {
    receipt: serde_json::Value,
}

impl VerifiedLicenseDecisionOverlay {
    pub fn to_json(&self) -> &serde_json::Value {
        &self.receipt
    }
}

/// Re-verify one persisted decision for the exact plan/spec and emit the
/// existing builder overlay. A stale, revoked, expired or differently-bound
/// decision is refused; it never reaches the builder.
pub fn verify_license_decision_overlay(
    record: &LicenseDecisionRecord,
    context: &LicenseDecisionOverlayContext<'_>,
) -> Result<VerifiedLicenseDecisionOverlay, LicenseDecisionRefusal> {
    if record.project_id != context.project_id {
        return Err(LicenseDecisionRefusal::ProjectMismatch);
    }
    if record.status != "ACTIVE" || record.decision != "ACCEPT" || record.revoked_at.is_some() {
        return Err(LicenseDecisionRefusal::DelegationMismatch);
    }
    if context.delegation_revoked {
        return Err(LicenseDecisionRefusal::DelegationMismatch);
    }
    if chrono::DateTime::parse_from_rfc3339(&record.delegation_deadline)
        .map(|deadline| deadline <= context.now)
        .unwrap_or(true)
    {
        return Err(LicenseDecisionRefusal::DelegationMismatch);
    }
    if record.delegation_criterion != "no-mandatory-fee-commercial-use"
        || record.delegation_matter != "LICENSE_IDENTITY_EVIDENCE_DELEGATION"
    {
        return Err(LicenseDecisionRefusal::DelegationMismatch);
    }
    if record.plan_sha256 != context.plan_sha256 || record.plan_revision != context.plan_revision {
        return Err(LicenseDecisionRefusal::PlanBindingMismatch);
    }
    if record.spec_sha256.as_deref() != context.spec_sha256 {
        return Err(LicenseDecisionRefusal::SpecBindingMismatch);
    }
    if record.inventory_sha256 != context.inventory_sha256
        || record.evidence_bundle_sha256 != context.evidence_bundle_sha256
    {
        return Err(LicenseDecisionRefusal::EvidenceBundleMismatch);
    }
    if record.catalog_sha256 != default_catalog_sha256()
        || record.profile_sha256 != default_profile_sha256()
        || record.policy_sha256 != default_binding_sha256()
    {
        return Err(LicenseDecisionRefusal::PolicyMismatch);
    }
    let candidate: CandidateBinding = serde_json::from_value(record.candidate.clone())
        .map_err(|_| LicenseDecisionRefusal::CandidateUnverified)?;
    if candidate.base_commit() != Some(context.base_commit) {
        return Err(LicenseDecisionRefusal::CandidateUnverified);
    }
    let expected_stage = if context.spec_sha256.is_some() {
        "specInventoryEvidence"
    } else {
        "planInventoryEvidence"
    };
    if record
        .stage
        .get("stage")
        .and_then(serde_json::Value::as_str)
        != Some(expected_stage)
        || record
            .stage
            .get("planSha256")
            .and_then(serde_json::Value::as_str)
            != Some(context.plan_sha256)
        || record
            .stage
            .get("planRevision")
            .and_then(serde_json::Value::as_u64)
            != Some(u64::from(context.plan_revision))
        || record
            .stage
            .get("inventorySha256")
            .and_then(serde_json::Value::as_str)
            != Some(context.inventory_sha256)
        || record
            .stage
            .get("evidenceBundleSha256")
            .and_then(serde_json::Value::as_str)
            != Some(context.evidence_bundle_sha256)
        || record
            .stage
            .get("specSha256")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            != context.spec_sha256.map(str::to_owned)
    {
        return Err(LicenseDecisionRefusal::PlanBindingMismatch);
    }

    let receipt = context.identity_facts.receipt();
    let digest = identity_facts_receipt_digest(context.identity_facts.verified());
    if record.identity_facts_receipt_digest != digest
        || record
            .stage
            .get("identityFactsReceiptDigest")
            .and_then(serde_json::Value::as_str)
            != Some(digest.as_str())
        || receipt.envelope.challenge.plan_sha256 != context.plan_sha256
        || receipt.envelope.challenge.pin != record.delegation_pin
    {
        return Err(LicenseDecisionRefusal::ReceiptDigestMismatch);
    }
    match receipt.envelope.challenge.stage {
        super::identity_facts::IdentityFactsStage::Spec => {
            if record.stage.get("resolutionKey").is_some() {
                return Err(LicenseDecisionRefusal::RootBindingMismatch);
            }
            if let Some(spec_sha256) = context.spec_sha256
                && receipt.envelope.challenge.spec_sha256 != spec_sha256
            {
                return Err(LicenseDecisionRefusal::ReceiptMismatch);
            }
        }
        super::identity_facts::IdentityFactsStage::Resolution => {
            if record
                .stage
                .get("resolutionKey")
                .and_then(serde_json::Value::as_str)
                != Some(receipt.envelope.challenge.resolution_key.as_str())
                || !receipt.envelope.challenge.spec_sha256.is_empty()
            {
                return Err(LicenseDecisionRefusal::RootBindingMismatch);
            }
        }
    }

    // The stored identity must still be the exact shared asset identity.
    let identity = &record.identity;
    let identities = source_asset()
        .get("identities")
        .and_then(serde_json::Value::as_array)
        .ok_or(LicenseDecisionRefusal::UnknownIdentity)?;
    let expected = identities
        .iter()
        .find(|entry| {
            exact(entry, "pin") == exact(identity, "pin")
                && exact(entry, "name") == exact(identity, "name")
                && exact(entry, "version") == exact(identity, "version")
                && exact(
                    entry.get("output").unwrap_or(&serde_json::Value::Null),
                    "storePath",
                ) == exact(
                    identity.get("output").unwrap_or(&serde_json::Value::Null),
                    "storePath",
                )
                && exact(
                    entry.get("output").unwrap_or(&serde_json::Value::Null),
                    "narHash",
                ) == exact(
                    identity.get("output").unwrap_or(&serde_json::Value::Null),
                    "narHash",
                )
        })
        .ok_or(LicenseDecisionRefusal::UnknownIdentity)?;
    if canonical_json_sha256(expected) != canonical_json_sha256(identity) {
        return Err(LicenseDecisionRefusal::IdentityMismatch);
    }
    let output = identity
        .get("output")
        .ok_or(LicenseDecisionRefusal::IdentityMismatch)?;
    receipt_fact(
        &receipt.envelope.manifest.facts,
        exact(output, "storePath").ok_or(LicenseDecisionRefusal::IdentityMismatch)?,
        exact(output, "drvPath").ok_or(LicenseDecisionRefusal::IdentityMismatch)?,
        exact(output, "narHash").ok_or(LicenseDecisionRefusal::IdentityMismatch)?,
    )?;
    let asset_evidence = source_asset()
        .get("evidence")
        .ok_or(LicenseDecisionRefusal::AssetCanonicalHashMissing)?;
    if record.evidence_sha256
        != exact(asset_evidence, "licenseEvidenceCanonicalSha256")
            .ok_or(LicenseDecisionRefusal::AssetCanonicalHashMissing)?
        || record.delegation_pin != exact(identity, "pin").unwrap_or_default()
        || record.delegation_use_context != record.use_context
        || record.license_expression != exact(identity, "license").unwrap_or_default()
    {
        return Err(LicenseDecisionRefusal::EvidenceBundleMismatch);
    }

    // The decision carries the exact verified proof with the real output-drv
    // inputDrvs and ATerm digests. Re-verify it; never synthesise an input.
    let stored_proof = record
        .provenance
        .get("sourceEvidence")
        .ok_or(LicenseDecisionRefusal::SourceEvidenceMismatch)?;
    let source = VerifiedSourceEvidence::from_verified_json(stored_proof)?;
    if !source_matches_asset_identity(&source, expected) {
        return Err(LicenseDecisionRefusal::SourceEvidenceMismatch);
    }
    let source_digest = source.canonical_sha256();
    if record
        .provenance
        .get("sourceEvidenceSha256")
        .and_then(serde_json::Value::as_str)
        != Some(source_digest.as_str())
    {
        return Err(LicenseDecisionRefusal::SourceEvidenceMismatch);
    }

    let mut receipt = serde_json::json!({
        "id": record.id,
        "projectId": record.project_id,
        "planSha256": record.plan_sha256,
        "planRevision": record.plan_revision,
        "specSha256": record.spec_sha256,
        "inventorySha256": record.inventory_sha256,
        "evidenceBundleSha256": record.evidence_bundle_sha256,
        "candidate": record.candidate,
        "identity": identity,
        "licenseExpression": record.license_expression,
        "selectedBranch": record.selected_branch,
        "softwareObligations": record.software_obligations,
        "documentation": record.documentation,
        "documentationObligations": record.documentation_obligations,
        "softwareVerdict": record.software_verdict,
        "evidenceSha256": record.evidence_sha256,
        "identityFactsReceiptDigest": record.identity_facts_receipt_digest,
        "catalogSha256": record.catalog_sha256,
        "profileSha256": record.profile_sha256,
        "policySha256": record.policy_sha256,
        "stage": record.stage,
        "useContext": record.use_context,
        "commercialUse": record.commercial_use,
        "paymentRequired": record.payment_required,
        "decision": record.decision,
        "revision": record.revision,
        "status": record.status,
        "revokedAt": record.revoked_at,
        "delegationDeadline": record.delegation_deadline,
        "sourceEvidence": source.to_json(),
        "sourceEvidenceSha256": source_digest,
    });
    // Recompute one canonical digest over the emitted overlay (without the
    // digest itself) so the builder verifies the exact delivered bytes.
    let content_sha256 = canonical_json_sha256(&receipt);
    receipt["contentSha256"] = serde_json::Value::String(content_sha256);
    Ok(VerifiedLicenseDecisionOverlay { receipt })
}

#[cfg(test)]
mod tests;
