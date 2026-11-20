//! Class-level Lead delegation for Nix identity classification (ADR-0045).
//!
//! The Owner grants a class of decision, never an identity list. A single
//! deterministic Core decision binds the exact identity, the validated typed
//! evidence bundle and the mandate together. Anything incomplete fails closed:
//! missing evidence is not an Owner escalation and never an acceptance.
//!
//! Limits stated plainly: a digest supplied by a model is a provenance claim, not
//! proof that a document was downloaded or is true. This module enforces the
//! shape, the instance binding, the mandate and the current policy; it does not
//! invent a semantic proof of commercial permission.

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use super::license_policy::{
    LicenseContext, LicenseDecision, default_catalog, default_profile, evaluate,
};

/// The only decision class this contract covers.
pub const CLASS_SCOPE: &str = "nix-identity-classification";

/// Explicit prohibitions that complement the positive policy evaluation. They
/// never replace it and never encode an identity allowlist.
pub const DENIED_EVIDENCE_LICENSE_IDS: [&str; 2] = ["NOASSERTION", "NONE"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LeadClassificationDecision {
    Accept,
    Reject,
    NeedsEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceSourceKind {
    Pin,
    Derivation,
    Output,
    UpstreamDocumentation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClassificationMandate {
    pub scope: String,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EvidenceSource {
    pub kind: EvidenceSourceKind,
    pub reference: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IdentityObservation {
    pub pin: String,
    pub store_path: String,
    pub deriver: String,
    pub nar_hash: String,
    pub license: Option<String>,
    pub unfree: Option<bool>,
}

/// Everything the Lead submits for one identity. The licence text digest must
/// match a digested upstream-documentation source, and the intended use and the
/// obligation review are declared explicitly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EvidenceBundle {
    pub observation: IdentityObservation,
    pub sources: Vec<EvidenceSource>,
    pub licence_text_digest: String,
    pub intended_use: String,
    pub obligations_reviewed: bool,
}

/// Advisory dictum without any authority field. It records the Lead's reasoning
/// and the exact binding, and no consumer may read it as an approval. A caller
/// cannot add authority by construction or by deserialisation because no such
/// field exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClassificationDictum {
    pub pin: String,
    pub store_path: String,
    pub deriver: String,
    pub nar_hash: String,
    pub license: String,
    pub licence_text_digest: String,
    pub intended_use: String,
    pub obligations_reviewed: bool,
    pub mandate_revision: u64,
    /// Digest of the canonical bundle, binding class/use/obligations to the same
    /// candidate.
    pub bundle_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassificationRefusal {
    NoMandate,
    OutOfClass,
    MandateRevoked,
    IncompleteIdentity,
    NoEvidence,
    InvalidDigest,
    MissingPrimarySource,
    DifferentInstance,
    MissingLicenceText,
    MissingUse,
    ObligationsNotReviewed,
    MissingEvidence,
    LicenseDenied,
    LicenseUnknown,
    Unfree,
    NeedsEvidence,
    Rejected,
    ActorMismatch,
    ActorPurpose,
    ActorStale,
    ActorScope,
    LeadTerminal,
    DelegationMismatch,
    DelegationExpired,
    DelegationOption,
}

impl std::fmt::Display for ClassificationRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NoMandate => "no active classification mandate",
            Self::OutOfClass => "the mandate is outside the classification class",
            Self::MandateRevoked => "the classification mandate is revoked",
            Self::IncompleteIdentity => "the observed identity is incomplete",
            Self::NoEvidence => "no evidence bundle was supplied",
            Self::InvalidDigest => "an evidence digest is not a valid sha256 digest",
            Self::MissingPrimarySource => "no primary source binds the exact instance",
            Self::DifferentInstance => "evidence describes a different instance",
            Self::MissingLicenceText => "no digested upstream licence text is supplied",
            Self::MissingUse => "the intended use is not declared",
            Self::ObligationsNotReviewed => "the applicable obligations were not reviewed",
            Self::MissingEvidence => "no independently observed licence value",
            Self::LicenseDenied => "the observed licence is denied",
            Self::LicenseUnknown => "the observed licence is not in the current policy",
            Self::Unfree => "the observed identity is unfree",
            Self::NeedsEvidence => "the Lead requires more evidence",
            Self::Rejected => "the Lead rejected the identity",
            Self::ActorMismatch => {
                "the Lead actor is not bound to the exact request and reservation"
            }
            Self::ActorPurpose => "the Lead actor reservation has the wrong purpose",
            Self::ActorStale => "the Lead actor reservation is not a SETTLED LEAD reservation",
            Self::ActorScope => "the Lead actor reservation is outside the exact project and input",
            Self::LeadTerminal => "the Lead request is terminal and cannot authorize a decision",
            Self::DelegationMismatch => {
                "the licence delegation does not bind the exact request pin and revision"
            }
            Self::DelegationExpired => "the Owner licence delegation has expired",
            Self::DelegationOption => {
                "the Owner delegation did not select the exact licence delegation option"
            }
        })
    }
}

impl std::error::Error for ClassificationRefusal {}

pub(crate) fn is_sha256(value: &str) -> bool {
    let value = value.trim();
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return true;
    }
    let Some(rest) = value.strip_prefix("sha256-") else {
        return false;
    };
    match base64::engine::general_purpose::STANDARD.decode(rest) {
        Ok(decoded) => decoded.len() == 32,
        Err(_) => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn actor_binding_sha256(
    project_id: &str,
    principal_id: &str,
    lead_request_id: &str,
    reservation_id: &str,
    request_sha256: &str,
    output_sha256: &str,
    purpose: &str,
    role: &str,
    state: &str,
) -> String {
    let payload = serde_json::json!({
        "projectId": project_id,
        "principalId": principal_id,
        "leadRequestId": lead_request_id,
        "reservationId": reservation_id,
        "requestSha256": request_sha256,
        "outputSha256": output_sha256,
        "purpose": purpose,
        "role": role,
        "state": state,
    });
    sha256_hex(&serde_json::to_vec(&payload).expect("actor binding payload is serializable"))
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn exact_store_path(value: &str) -> bool {
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

fn identity_complete(observation: &IdentityObservation) -> bool {
    let pin = observation
        .pin
        .strip_prefix("github:NixOS/nixpkgs/")
        .unwrap_or("");
    let rev_ok = pin.len() == 40 && pin.bytes().all(|byte| byte.is_ascii_hexdigit());
    rev_ok
        && exact_store_path(&observation.store_path)
        && exact_store_path(observation.deriver.strip_suffix(".drv").unwrap_or(""))
        && observation.deriver.ends_with(".drv")
        && is_sha256(&observation.nar_hash)
}

fn evidence_binds_instance(bundle: &EvidenceBundle) -> Result<(), ClassificationRefusal> {
    let observation = &bundle.observation;
    for source in &bundle.sources {
        match source.kind {
            EvidenceSourceKind::Pin if source.reference != observation.pin => {
                return Err(ClassificationRefusal::DifferentInstance);
            }
            EvidenceSourceKind::Derivation if source.reference != observation.deriver => {
                return Err(ClassificationRefusal::DifferentInstance);
            }
            EvidenceSourceKind::Output if source.reference != observation.store_path => {
                return Err(ClassificationRefusal::DifferentInstance);
            }
            _ => {}
        }
    }
    // A pin source is context, never a primary instance source.
    let primary = bundle.sources.iter().any(|source| match source.kind {
        EvidenceSourceKind::Derivation => source.reference == observation.deriver,
        EvidenceSourceKind::Output => source.reference == observation.store_path,
        EvidenceSourceKind::Pin | EvidenceSourceKind::UpstreamDocumentation => false,
    });
    if !primary {
        return Err(ClassificationRefusal::MissingPrimarySource);
    }
    Ok(())
}

fn licence_text_binds(bundle: &EvidenceBundle) -> Result<(), ClassificationRefusal> {
    if !is_sha256(&bundle.licence_text_digest) {
        return Err(ClassificationRefusal::InvalidDigest);
    }
    let documented = bundle.sources.iter().any(|source| {
        source.kind == EvidenceSourceKind::UpstreamDocumentation
            && !source.reference.trim().is_empty()
            && source.digest == bundle.licence_text_digest
            && is_sha256(&source.digest)
    });
    if !documented {
        return Err(ClassificationRefusal::MissingLicenceText);
    }
    Ok(())
}

fn policy_allows(license: &str) -> Result<(), ClassificationRefusal> {
    if matches!(
        license,
        value if DENIED_EVIDENCE_LICENSE_IDS.contains(&value)
    ) {
        return Err(ClassificationRefusal::LicenseDenied);
    }
    match evaluate(
        license,
        LicenseContext::Closure,
        default_catalog(),
        default_profile(),
    ) {
        LicenseDecision::Allowed => Ok(()),
        LicenseDecision::Denied => Err(ClassificationRefusal::LicenseDenied),
        _ => Err(ClassificationRefusal::LicenseUnknown),
    }
}

/// Deterministic classification body over an already-validated mandate.
///
/// This function proves only that the evidence bundle is internally exact and
/// that the current policy classifies it. It is not a public authority path:
/// callers must use [`classify_verified_with_evidence`], whose mandate can only
/// be built from the exact database reservation and Owner delegation.
pub(crate) fn classify_with_evidence(
    mandate: &ClassificationMandate,
    bundle: Option<&EvidenceBundle>,
    decision: LeadClassificationDecision,
) -> Result<ClassificationDictum, ClassificationRefusal> {
    if mandate.revision == 0 || mandate.scope.trim().is_empty() {
        return Err(ClassificationRefusal::NoMandate);
    }
    if mandate.scope != CLASS_SCOPE {
        return Err(ClassificationRefusal::OutOfClass);
    }
    let Some(bundle) = bundle else {
        return Err(ClassificationRefusal::NoEvidence);
    };
    let observation = &bundle.observation;
    if !identity_complete(observation) {
        return Err(ClassificationRefusal::IncompleteIdentity);
    }
    if bundle.sources.is_empty() {
        return Err(ClassificationRefusal::NoEvidence);
    }
    if bundle
        .sources
        .iter()
        .any(|source| !is_sha256(&source.digest))
    {
        return Err(ClassificationRefusal::InvalidDigest);
    }
    evidence_binds_instance(bundle)?;
    licence_text_binds(bundle)?;
    if bundle.intended_use.trim().is_empty() {
        return Err(ClassificationRefusal::MissingUse);
    }
    if !bundle.obligations_reviewed {
        return Err(ClassificationRefusal::ObligationsNotReviewed);
    }
    match decision {
        LeadClassificationDecision::Accept => {}
        LeadClassificationDecision::Reject => return Err(ClassificationRefusal::Rejected),
        LeadClassificationDecision::NeedsEvidence => {
            return Err(ClassificationRefusal::NeedsEvidence);
        }
    }
    let license = observation
        .license
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (Some(license), Some(unfree)) = (license, observation.unfree) else {
        return Err(ClassificationRefusal::MissingEvidence);
    };
    if unfree {
        return Err(ClassificationRefusal::Unfree);
    }
    policy_allows(license)?;
    let canonical = serde_json::to_vec(bundle).map_err(|_| ClassificationRefusal::NoEvidence)?;
    let bundle_sha256 = {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(&canonical);
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    Ok(ClassificationDictum {
        pin: observation.pin.clone(),
        store_path: observation.store_path.clone(),
        deriver: observation.deriver.clone(),
        nar_hash: observation.nar_hash.clone(),
        license: license.to_owned(),
        licence_text_digest: bundle.licence_text_digest.clone(),
        intended_use: bundle.intended_use.clone(),
        obligations_reviewed: bundle.obligations_reviewed,
        mandate_revision: mandate.revision,
        bundle_sha256,
    })
}

/// Private Core-verified Lead actor binding for a licence decision.
///
/// This type is deliberately not `Deserialize` and its fields are private. Only
/// Core's decision handler can construct it from the exact Lead request, the
/// SETTLED `license_evidence_review` reservation and the output digest. A
/// model, chat message or HTTP payload cannot manufacture it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenseActorBinding {
    project_id: String,
    principal_id: String,
    lead_request_id: String,
    reservation_id: String,
    request_sha256: String,
    output_sha256: String,
    purpose: String,
    role: String,
    state: String,
}

impl LicenseActorBinding {
    /// Construct the actor only from a database-loaded SETTLED reservation.
    pub fn from_reservation(
        reservation: &crate::db::LicenseEvidenceReservation,
        lead_request_id: &str,
    ) -> Result<Self, ClassificationRefusal> {
        if reservation.objective_id != lead_request_id {
            return Err(ClassificationRefusal::ActorMismatch);
        }
        if reservation.project_id.trim().is_empty() || reservation.principal_id.trim().is_empty() {
            return Err(ClassificationRefusal::ActorScope);
        }
        if reservation.expires_at_ms <= chrono::Utc::now().timestamp_millis() {
            return Err(ClassificationRefusal::ActorStale);
        }
        if matches!(reservation.lead_state.as_str(), "BLOCKED" | "FAILED") || reservation.withdrawn
        {
            return Err(ClassificationRefusal::LeadTerminal);
        }
        let output_sha256 = sha256_hex(reservation.response_text.as_bytes());
        Self::verify(
            &reservation.project_id,
            &reservation.principal_id,
            lead_request_id,
            &reservation.reservation_id,
            &reservation.request_sha256,
            &output_sha256,
            &reservation.purpose,
            &reservation.role,
            &reservation.state,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn verify(
        project_id: &str,
        principal_id: &str,
        lead_request_id: &str,
        reservation_id: &str,
        request_sha256: &str,
        output_sha256: &str,
        purpose: &str,
        role: &str,
        state: &str,
    ) -> Result<Self, ClassificationRefusal> {
        if lead_request_id.trim().is_empty()
            || reservation_id.trim().is_empty()
            || project_id.trim().is_empty()
            || principal_id.trim().is_empty()
        {
            return Err(ClassificationRefusal::ActorMismatch);
        }
        if purpose != "license_evidence_review" {
            return Err(ClassificationRefusal::ActorPurpose);
        }
        if role != "LEAD" || state != "SETTLED" {
            return Err(ClassificationRefusal::ActorStale);
        }
        if !is_sha256(request_sha256) || !is_sha256(output_sha256) {
            return Err(ClassificationRefusal::InvalidDigest);
        }
        Ok(Self {
            project_id: project_id.into(),
            principal_id: principal_id.into(),
            lead_request_id: lead_request_id.into(),
            reservation_id: reservation_id.into(),
            request_sha256: request_sha256.into(),
            output_sha256: output_sha256.into(),
            purpose: purpose.into(),
            role: role.into(),
            state: state.into(),
        })
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    pub fn principal_id(&self) -> &str {
        &self.principal_id
    }

    pub fn lead_request_id(&self) -> &str {
        &self.lead_request_id
    }

    pub fn reservation_id(&self) -> &str {
        &self.reservation_id
    }

    pub fn request_sha256(&self) -> &str {
        &self.request_sha256
    }

    pub fn output_sha256(&self) -> &str {
        &self.output_sha256
    }

    pub fn binding_sha256(&self) -> String {
        actor_binding_sha256(
            &self.project_id,
            &self.principal_id,
            &self.lead_request_id,
            &self.reservation_id,
            &self.request_sha256,
            &self.output_sha256,
            &self.purpose,
            &self.role,
            &self.state,
        )
    }
}

/// Private Core-verified Owner-delegation + Lead-actor mandate.
///
/// The delegation is an `agentic_owner_decisions` record read by Core; the
/// actor is the exact Lead reservation above. The fields are private and the
/// type is not `Deserialize`; a caller cannot widen the scope or mint the
/// authority from JSON or chat. Revocation is read from the durable
/// append-only revocation record, never from a boolean in a payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedLicenseMandate {
    mandate: ClassificationMandate,
    actor: LicenseActorBinding,
    delegation_id: String,
    delegation_revision: u64,
    delegation_pin: String,
    use_context: String,
    project_id: String,
}

impl VerifiedLicenseMandate {
    /// Core-facing constructor: the actor is the database-loaded reservation;
    /// the delegation fields are loaded from the owner-decision row by Core.
    pub fn from_reservation(
        reservation: &crate::db::LicenseEvidenceReservation,
        lead_request_id: &str,
        delegation: &crate::db::AgenticOwnerDecisionRecord,
        delegation_revoked: bool,
        request_pin: &str,
    ) -> Result<Self, ClassificationRefusal> {
        let actor = LicenseActorBinding::from_reservation(reservation, lead_request_id)?;
        if actor.project_id != delegation.project_id {
            return Err(ClassificationRefusal::ActorScope);
        }
        if delegation.id.trim().is_empty() {
            return Err(ClassificationRefusal::DelegationMismatch);
        }
        if delegation.matter != "LICENSE_IDENTITY_EVIDENCE_DELEGATION" {
            return Err(ClassificationRefusal::OutOfClass);
        }
        if delegation.status != "APPROVED" || delegation_revoked {
            return Err(ClassificationRefusal::MandateRevoked);
        }
        if delegation.selected_option.as_deref() != Some("APPROVE_DELEGATION")
            || !delegation
                .options
                .iter()
                .any(|option| option == "APPROVE_DELEGATION")
        {
            return Err(ClassificationRefusal::DelegationOption);
        }
        let deadline = chrono::DateTime::parse_from_rfc3339(&delegation.deadline)
            .map_err(|_| ClassificationRefusal::DelegationExpired)?;
        if deadline <= chrono::Utc::now() {
            return Err(ClassificationRefusal::DelegationExpired);
        }
        let evidence = &delegation.evidence;
        if exact(evidence, "domain") != Some("licenses-identity-evidence") {
            return Err(ClassificationRefusal::OutOfClass);
        }
        if exact(evidence, "pin") != Some(request_pin) {
            return Err(ClassificationRefusal::DelegationMismatch);
        }
        let revision = evidence
            .get("revision")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        if revision == 0 {
            return Err(ClassificationRefusal::DelegationMismatch);
        }
        let use_context = exact(evidence, "useContext")
            .filter(|value| !value.trim().is_empty())
            .ok_or(ClassificationRefusal::OutOfClass)?;
        if exact(evidence, "economicCriterion") != Some("no-mandatory-fee-commercial-use") {
            return Err(ClassificationRefusal::OutOfClass);
        }
        Ok(Self {
            mandate: ClassificationMandate {
                scope: CLASS_SCOPE.into(),
                revision,
            },
            actor,
            delegation_id: delegation.id.clone(),
            delegation_revision: revision,
            delegation_pin: request_pin.into(),
            use_context: use_context.into(),
            project_id: delegation.project_id.clone(),
        })
    }

    pub fn actor_binding_sha256(&self) -> String {
        self.actor.binding_sha256()
    }

    pub fn actor(&self) -> &LicenseActorBinding {
        &self.actor
    }

    pub fn mandate_scope(&self) -> &str {
        &self.mandate.scope
    }

    pub fn delegation_id(&self) -> &str {
        &self.delegation_id
    }

    pub fn delegation_revision(&self) -> u64 {
        self.delegation_revision
    }

    pub fn delegation_pin(&self) -> &str {
        &self.delegation_pin
    }

    pub fn use_context(&self) -> &str {
        &self.use_context
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }
}

fn exact<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(serde_json::Value::as_str)
}

/// Public authority path for a class-level classification: only a mandate built
/// from the exact reservation and Owner delegation can reach the policy body.
pub fn classify_verified_with_evidence(
    mandate: &VerifiedLicenseMandate,
    bundle: Option<&EvidenceBundle>,
    decision: LeadClassificationDecision,
) -> Result<ClassificationDictum, ClassificationRefusal> {
    let Some(bundle) = bundle else {
        return Err(ClassificationRefusal::NoEvidence);
    };
    if bundle.observation.pin != mandate.delegation_pin {
        return Err(ClassificationRefusal::DelegationMismatch);
    }
    if bundle.intended_use != mandate.use_context {
        return Err(ClassificationRefusal::DelegationMismatch);
    }
    classify_with_evidence(&mandate.mandate, Some(bundle), decision)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PIN: &str = "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d";
    const DIGEST: &str = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    const CASES: [(&str, &str); 3] = [
        (
            "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-a-1",
            "/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-a-1.drv",
        ),
        (
            "/nix/store/cccccccccccccccccccccccccccccccc-c-1",
            "/nix/store/dddddddddddddddddddddddddddddddd-c-1.drv",
        ),
        (
            "/nix/store/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-e-1",
            "/nix/store/ffffffffffffffffffffffffffffffff-e-1.drv",
        ),
    ];

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
            request_sha256: "a".repeat(64),
            state: "SETTLED".into(),
            response_text: "the exact model output".into(),
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
                "revision": 3,
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

    #[test]
    fn actor_binding_requires_exact_lead_purpose_settled_and_project() {
        let valid = LicenseActorBinding::from_reservation(&reservation(), "lead-1")
            .expect("valid actor binding");
        assert_eq!(valid.binding_sha256().len(), 64);
        assert_eq!(valid.project_id(), "project-1");
        assert_eq!(valid.principal_id(), "operator");

        let mut wrong_purpose = reservation();
        wrong_purpose.purpose = "lead_environment_review".into();
        assert_eq!(
            LicenseActorBinding::from_reservation(&wrong_purpose, "lead-1"),
            Err(ClassificationRefusal::ActorPurpose)
        );
        let mut worker = reservation();
        worker.role = "WORKER".into();
        assert_eq!(
            LicenseActorBinding::from_reservation(&worker, "lead-1"),
            Err(ClassificationRefusal::ActorStale)
        );
        let mut reserved = reservation();
        reserved.state = "RESERVED".into();
        assert_eq!(
            LicenseActorBinding::from_reservation(&reserved, "lead-1"),
            Err(ClassificationRefusal::ActorStale)
        );
        let mut terminal = reservation();
        terminal.lead_state = "BLOCKED".into();
        assert_eq!(
            LicenseActorBinding::from_reservation(&terminal, "lead-1"),
            Err(ClassificationRefusal::LeadTerminal)
        );
        let mut expired = reservation();
        expired.expires_at_ms = chrono::Utc::now().timestamp_millis() - 1;
        assert_eq!(
            LicenseActorBinding::from_reservation(&expired, "lead-1"),
            Err(ClassificationRefusal::ActorStale)
        );
        let mut no_principal = reservation();
        no_principal.principal_id = " ".into();
        assert_eq!(
            LicenseActorBinding::from_reservation(&no_principal, "lead-1"),
            Err(ClassificationRefusal::ActorScope)
        );
    }

    #[test]
    fn verified_mandate_requires_active_delegation_option_project_and_pin() {
        let delegation = delegation();
        let valid = VerifiedLicenseMandate::from_reservation(
            &reservation(),
            "lead-1",
            &delegation,
            false,
            PIN,
        )
        .expect("valid verified mandate");
        assert_eq!(valid.actor_binding_sha256().len(), 64);
        assert_eq!(valid.project_id(), "project-1");
        assert_eq!(valid.delegation_revision(), 3);
        assert_eq!(valid.mandate_scope(), CLASS_SCOPE);
        assert_eq!(valid.use_context(), "commercial-development");

        let mut revoked = delegation.clone();
        revoked.status = "DENIED".into();
        assert_eq!(
            VerifiedLicenseMandate::from_reservation(
                &reservation(),
                "lead-1",
                &revoked,
                false,
                PIN
            ),
            Err(ClassificationRefusal::MandateRevoked)
        );
        assert_eq!(
            VerifiedLicenseMandate::from_reservation(
                &reservation(),
                "lead-1",
                &delegation,
                true,
                PIN
            ),
            Err(ClassificationRefusal::MandateRevoked)
        );
        let mut other_project = delegation.clone();
        other_project.project_id = "project-2".into();
        assert_eq!(
            VerifiedLicenseMandate::from_reservation(
                &reservation(),
                "lead-1",
                &other_project,
                false,
                PIN
            ),
            Err(ClassificationRefusal::ActorScope)
        );
        let mut wrong_option = delegation.clone();
        wrong_option.selected_option = Some("DENY".into());
        assert_eq!(
            VerifiedLicenseMandate::from_reservation(
                &reservation(),
                "lead-1",
                &wrong_option,
                false,
                PIN
            ),
            Err(ClassificationRefusal::DelegationOption)
        );
        let mut expired = delegation.clone();
        expired.deadline = (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
        assert_eq!(
            VerifiedLicenseMandate::from_reservation(
                &reservation(),
                "lead-1",
                &expired,
                false,
                PIN
            ),
            Err(ClassificationRefusal::DelegationExpired)
        );
        assert_eq!(
            VerifiedLicenseMandate::from_reservation(
                &reservation(),
                "lead-1",
                &delegation,
                false,
                "github:NixOS/nixpkgs/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            ),
            Err(ClassificationRefusal::DelegationMismatch)
        );
        let mut other_domain = delegation.clone();
        other_domain.evidence["domain"] = serde_json::json!("other");
        assert_eq!(
            VerifiedLicenseMandate::from_reservation(
                &reservation(),
                "lead-1",
                &other_domain,
                false,
                PIN
            ),
            Err(ClassificationRefusal::OutOfClass)
        );
        let mut wrong_criterion = delegation;
        wrong_criterion.evidence["economicCriterion"] = serde_json::json!("any");
        assert_eq!(
            VerifiedLicenseMandate::from_reservation(
                &reservation(),
                "lead-1",
                &wrong_criterion,
                false,
                PIN
            ),
            Err(ClassificationRefusal::OutOfClass)
        );
    }

    fn observation(
        path: &str,
        deriver: &str,
        license: Option<&str>,
        unfree: Option<bool>,
    ) -> IdentityObservation {
        IdentityObservation {
            pin: PIN.into(),
            store_path: path.into(),
            deriver: deriver.into(),
            nar_hash: DIGEST.into(),
            license: license.map(str::to_owned),
            unfree,
        }
    }

    fn bundle(
        path: &str,
        deriver: &str,
        license: Option<&str>,
        unfree: Option<bool>,
    ) -> EvidenceBundle {
        EvidenceBundle {
            observation: observation(path, deriver, license, unfree),
            sources: vec![
                EvidenceSource {
                    kind: EvidenceSourceKind::Output,
                    reference: path.into(),
                    digest: DIGEST.into(),
                },
                EvidenceSource {
                    kind: EvidenceSourceKind::UpstreamDocumentation,
                    reference: "https://example.invalid/LICENSE".into(),
                    digest: DIGEST.into(),
                },
            ],
            licence_text_digest: DIGEST.into(),
            intended_use: "develop a commercial product offline".into(),
            obligations_reviewed: true,
        }
    }

    fn mandate() -> ClassificationMandate {
        ClassificationMandate {
            scope: CLASS_SCOPE.into(),
            revision: 4,
        }
    }

    #[test]
    fn a_bound_identity_yields_an_advisory_dictum() {
        for (path, deriver) in CASES {
            let dictum = classify_with_evidence(
                &mandate(),
                Some(&bundle(path, deriver, Some("MIT"), Some(false))),
                LeadClassificationDecision::Accept,
            )
            .expect("a bound identity yields an advisory dictum");
            assert_eq!(dictum.store_path, path);
            assert_eq!(dictum.deriver, deriver);
            assert_eq!(dictum.mandate_revision, 4);
        }
    }

    #[test]
    fn no_evidence_or_pin_only_is_refused() {
        let (path, deriver) = CASES[0];
        assert_eq!(
            classify_with_evidence(&mandate(), None, LeadClassificationDecision::Accept),
            Err(ClassificationRefusal::NoEvidence)
        );
        let mut pin_only = bundle(path, deriver, Some("MIT"), Some(false));
        pin_only.sources = vec![EvidenceSource {
            kind: EvidenceSourceKind::Pin,
            reference: PIN.into(),
            digest: DIGEST.into(),
        }];
        assert_eq!(
            classify_with_evidence(
                &mandate(),
                Some(&pin_only),
                LeadClassificationDecision::Accept
            ),
            Err(ClassificationRefusal::MissingPrimarySource)
        );
    }

    #[test]
    fn pin_only_with_documentation_is_not_a_primary_source() {
        let (path, deriver) = CASES[0];
        let mut evidence = bundle(path, deriver, Some("MIT"), Some(false));
        evidence.sources = vec![
            EvidenceSource {
                kind: EvidenceSourceKind::Pin,
                reference: PIN.into(),
                digest: DIGEST.into(),
            },
            EvidenceSource {
                kind: EvidenceSourceKind::UpstreamDocumentation,
                reference: "https://example.invalid/LICENSE".into(),
                digest: DIGEST.into(),
            },
        ];
        assert_eq!(
            classify_with_evidence(
                &mandate(),
                Some(&evidence),
                LeadClassificationDecision::Accept
            ),
            Err(ClassificationRefusal::MissingPrimarySource)
        );
    }

    #[test]
    fn a_contradictory_pin_is_refused() {
        let (path, deriver) = CASES[0];
        let mut evidence = bundle(path, deriver, Some("MIT"), Some(false));
        evidence.sources.push(EvidenceSource {
            kind: EvidenceSourceKind::Pin,
            reference: "github:NixOS/nixpkgs/0000000000000000000000000000000000000000".into(),
            digest: DIGEST.into(),
        });
        assert_eq!(
            classify_with_evidence(
                &mandate(),
                Some(&evidence),
                LeadClassificationDecision::Accept
            ),
            Err(ClassificationRefusal::DifferentInstance)
        );
    }

    #[test]
    fn a_short_sri_digest_is_invalid() {
        let (path, deriver) = CASES[0];
        let mut evidence = bundle(path, deriver, Some("MIT"), Some(false));
        evidence.licence_text_digest = "sha256-abc".into();
        evidence.sources[1].digest = "sha256-abc".into();
        assert_eq!(
            classify_with_evidence(
                &mandate(),
                Some(&evidence),
                LeadClassificationDecision::Accept
            ),
            Err(ClassificationRefusal::InvalidDigest)
        );
    }

    #[test]
    fn malformed_base64_is_invalid() {
        let (path, deriver) = CASES[0];
        for digest in [
            // '=' in the middle, followed by a letter
            format!("sha256-{}=A", "A".repeat(42)),
            // non-canonical padding bits: 'AB==' decodes to one byte with set pad bits
            "sha256-AB==".to_string(),
            // correct length but wrong content length (44 chars does not imply 32 bytes)
            format!("sha256-{}", "A".repeat(44)),
        ] {
            let mut evidence = bundle(path, deriver, Some("MIT"), Some(false));
            evidence.licence_text_digest = digest.clone();
            evidence.sources[1].digest = digest;
            assert_eq!(
                classify_with_evidence(
                    &mandate(),
                    Some(&evidence),
                    LeadClassificationDecision::Accept
                ),
                Err(ClassificationRefusal::InvalidDigest)
            );
        }
    }

    #[test]
    fn another_instance_or_invalid_digest_is_refused() {
        let (path, deriver) = CASES[0];
        let mut other = bundle(path, deriver, Some("MIT"), Some(false));
        other.sources[0].reference = CASES[1].0.into();
        assert_eq!(
            classify_with_evidence(&mandate(), Some(&other), LeadClassificationDecision::Accept),
            Err(ClassificationRefusal::DifferentInstance)
        );
        let mut bad_digest = bundle(path, deriver, Some("MIT"), Some(false));
        bad_digest.sources[0].digest = "not-a-digest".into();
        assert_eq!(
            classify_with_evidence(
                &mandate(),
                Some(&bad_digest),
                LeadClassificationDecision::Accept
            ),
            Err(ClassificationRefusal::InvalidDigest)
        );
    }

    #[test]
    fn missing_text_use_or_obligations_is_refused() {
        let (path, deriver) = CASES[0];
        for mutated in [
            {
                let mut b = bundle(path, deriver, Some("MIT"), Some(false));
                b.licence_text_digest = "sha256-ZZZ".into();
                b
            },
            {
                let mut b = bundle(path, deriver, Some("MIT"), Some(false));
                b.intended_use = "  ".into();
                b
            },
            {
                let mut b = bundle(path, deriver, Some("MIT"), Some(false));
                b.obligations_reviewed = false;
                b
            },
        ] {
            assert!(matches!(
                classify_with_evidence(
                    &mandate(),
                    Some(&mutated),
                    LeadClassificationDecision::Accept
                ),
                Err(ClassificationRefusal::InvalidDigest)
                    | Err(ClassificationRefusal::MissingUse)
                    | Err(ClassificationRefusal::ObligationsNotReviewed)
            ));
        }
    }

    #[test]
    fn policy_and_mandate_refusals_are_preserved() {
        let (path, deriver) = CASES[0];
        let cases = [
            (
                bundle(path, deriver, Some("MIT"), Some(true)),
                ClassificationRefusal::Unfree,
            ),
            (
                bundle(path, deriver, Some("NOASSERTION"), Some(false)),
                ClassificationRefusal::LicenseDenied,
            ),
            (
                bundle(path, deriver, Some("Not-A-Real-Licence"), Some(false)),
                ClassificationRefusal::LicenseUnknown,
            ),
            (
                bundle(path, deriver, None, None),
                ClassificationRefusal::MissingEvidence,
            ),
        ];
        for (evidence, refusal) in cases {
            assert_eq!(
                classify_with_evidence(
                    &mandate(),
                    Some(&evidence),
                    LeadClassificationDecision::Accept
                ),
                Err(refusal)
            );
        }
        let mut out_of_class = mandate();
        out_of_class.scope = "some-other-class".into();
        assert_eq!(
            classify_with_evidence(
                &out_of_class,
                Some(&bundle(path, deriver, Some("MIT"), Some(false))),
                LeadClassificationDecision::Accept
            ),
            Err(ClassificationRefusal::OutOfClass)
        );
        assert_eq!(
            classify_with_evidence(
                &mandate(),
                Some(&bundle(path, deriver, Some("MIT"), Some(false))),
                LeadClassificationDecision::NeedsEvidence
            ),
            Err(ClassificationRefusal::NeedsEvidence)
        );
    }
}
