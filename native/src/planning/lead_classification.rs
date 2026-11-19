//! Class-level Lead delegation for Nix identity classification (ADR-0045).
//!
//! The Owner grants a class of decision, never an identity list. Any identity
//! with an exact binding and independently observed licence evidence may be
//! classified by the Lead; missing evidence is a fail-closed refusal, never an
//! Owner escalation, and the licence must pass the current policy.

use serde::{Deserialize, Serialize};

use super::license_policy::{
    LicenseContext, LicenseDecision, default_catalog, default_profile, evaluate,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LeadClassificationDecision {
    Accept,
    Reject,
    NeedsEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClassificationMandate {
    pub scope: String,
    pub revision: u64,
    pub revoked: bool,
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
    pub evidence_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ApprovedIdentity {
    pub pin: String,
    pub store_path: String,
    pub deriver: String,
    pub nar_hash: String,
    pub mandate_revision: u64,
    pub evidence_ref: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassificationRefusal {
    NoMandate,
    MandateRevoked,
    ActorOutOfScope,
    IncompleteIdentity,
    MissingEvidence,
    LicenseDenied,
    LicenseUnknown,
    Unfree,
    NeedsEvidence,
    Rejected,
}

impl std::fmt::Display for ClassificationRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NoMandate => "no active classification mandate",
            Self::MandateRevoked => "the classification mandate is revoked",
            Self::ActorOutOfScope => "the Lead actor or delegation is out of scope",
            Self::IncompleteIdentity => "the observed identity is incomplete",
            Self::MissingEvidence => "no independently observed licence evidence",
            Self::LicenseDenied => "the observed licence is denied",
            Self::LicenseUnknown => "the observed licence is not in the current policy",
            Self::Unfree => "the observed identity is unfree",
            Self::NeedsEvidence => "the Lead requires more evidence",
            Self::Rejected => "the Lead rejected the identity",
        })
    }
}

impl std::error::Error for ClassificationRefusal {}

fn identity_complete(observation: &IdentityObservation) -> bool {
    observation.pin.starts_with("github:NixOS/nixpkgs/")
        && observation.store_path.starts_with("/nix/store/")
        && !observation.store_path.trim().is_empty()
        && observation.deriver.ends_with(".drv")
        && !observation.nar_hash.trim().is_empty()
        && !observation.evidence_ref.trim().is_empty()
}

/// Deterministic Core decision for a class-level Lead classification.
///
/// `actor_in_scope` must come from Core's own verification of the Lead actor and
/// its active delegation; it is never read from a request payload.
pub fn classify_identity(
    mandate: &ClassificationMandate,
    actor_in_scope: bool,
    observation: &IdentityObservation,
    decision: LeadClassificationDecision,
) -> Result<ApprovedIdentity, ClassificationRefusal> {
    if mandate.scope.trim().is_empty() || mandate.revision == 0 {
        return Err(ClassificationRefusal::NoMandate);
    }
    if mandate.revoked {
        return Err(ClassificationRefusal::MandateRevoked);
    }
    if !actor_in_scope {
        return Err(ClassificationRefusal::ActorOutOfScope);
    }
    if !identity_complete(observation) {
        return Err(ClassificationRefusal::IncompleteIdentity);
    }
    let license = observation
        .license
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (Some(license), Some(unfree)) = (license, observation.unfree) else {
        return Err(match decision {
            LeadClassificationDecision::NeedsEvidence => ClassificationRefusal::NeedsEvidence,
            _ => ClassificationRefusal::MissingEvidence,
        });
    };
    match decision {
        LeadClassificationDecision::Accept => {}
        LeadClassificationDecision::Reject => return Err(ClassificationRefusal::Rejected),
        LeadClassificationDecision::NeedsEvidence => {
            return Err(ClassificationRefusal::NeedsEvidence)
        }
    }
    if unfree {
        return Err(ClassificationRefusal::Unfree);
    }
    if matches!(license, "NOASSERTION" | "NONE") {
        return Err(ClassificationRefusal::LicenseDenied);
    }
    match evaluate(
        license,
        LicenseContext::Closure,
        default_catalog(),
        default_profile(),
    ) {
        LicenseDecision::Allowed => {}
        LicenseDecision::Denied => return Err(ClassificationRefusal::LicenseDenied),
        _ => return Err(ClassificationRefusal::LicenseUnknown),
    }
    Ok(ApprovedIdentity {
        pin: observation.pin.clone(),
        store_path: observation.store_path.clone(),
        deriver: observation.deriver.clone(),
        nar_hash: observation.nar_hash.clone(),
        mandate_revision: mandate.revision,
        evidence_ref: observation.evidence_ref.clone(),
    })
}


/// Explicit prohibitions that complement the positive policy evaluation. They
/// never replace it and never encode an identity allowlist.
pub const DENIED_EVIDENCE_LICENSE_IDS: [&str; 2] = ["NOASSERTION", "NONE"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceSourceKind {
    Pin,
    Derivation,
    Output,
    UpstreamDocumentation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EvidenceSource {
    pub kind: EvidenceSourceKind,
    pub reference: String,
    pub digest: String,
}

/// Evidence gathered by the Lead through the Librarian for one observation.
///
/// The sources must describe the *same* derivation and output as the observation;
/// evidence about another instance is never transferable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EvidenceBundle {
    pub observation: IdentityObservation,
    pub sources: Vec<EvidenceSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceRefusal {
    MissingPrimarySource,
    DifferentInstance,
    EmptyDigest,
}

/// True only for a primary source bound to this exact pin, derivation or output.
pub fn evidence_matches_instance(bundle: &EvidenceBundle) -> Result<(), EvidenceRefusal> {
    if bundle.sources.iter().any(|source| source.digest.trim().is_empty()) {
        return Err(EvidenceRefusal::EmptyDigest);
    }
    let observation = &bundle.observation;
    // A source that names a derivation or output of a different instance is a
    // transfer attempt, not a missing source.
    for source in &bundle.sources {
        match source.kind {
            EvidenceSourceKind::Derivation if source.reference != observation.deriver => {
                return Err(EvidenceRefusal::DifferentInstance)
            }
            EvidenceSourceKind::Output if source.reference != observation.store_path => {
                return Err(EvidenceRefusal::DifferentInstance)
            }
            _ => {}
        }
    }
    let primary = bundle.sources.iter().any(|source| match source.kind {
        EvidenceSourceKind::Pin => source.reference == observation.pin,
        EvidenceSourceKind::Derivation => source.reference == observation.deriver,
        EvidenceSourceKind::Output => source.reference == observation.store_path,
        EvidenceSourceKind::UpstreamDocumentation => false,
    });
    if !primary {
        return Err(EvidenceRefusal::MissingPrimarySource);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PIN: &str = "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d";

    fn observation(path: &str, deriver: &str, license: Option<&str>, unfree: Option<bool>) -> IdentityObservation {
        IdentityObservation {
            pin: PIN.into(),
            store_path: path.into(),
            deriver: deriver.into(),
            nar_hash: "sha256-x".into(),
            license: license.map(str::to_owned),
            unfree,
            evidence_ref: "coverage:fixture".into(),
        }
    }

    fn mandate() -> ClassificationMandate {
        ClassificationMandate { scope: "nix-identity-classification".into(), revision: 3, revoked: false }
    }

    const CASES: [(&str, &str); 3] = [
        ("/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-a-1", "/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-a-1.drv"),
        ("/nix/store/cccccccccccccccccccccccccccccccc-c-1", "/nix/store/dddddddddddddddddddddddddddddddd-c-1.drv"),
        ("/nix/store/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-e-1", "/nix/store/ffffffffffffffffffffffffffffffff-e-1.drv"),
    ];

    #[test]
    fn any_eligible_identity_is_classified_under_the_class_mandate() {
        for (path, deriver) in CASES {
            let grant = classify_identity(
                &mandate(), true,
                &observation(path, deriver, Some("MIT"), Some(false)),
                LeadClassificationDecision::Accept,
            ).expect("eligible identity");
            assert_eq!(grant.store_path, path);
            assert_eq!(grant.deriver, deriver);
            assert_eq!(grant.mandate_revision, 3);
        }
    }


    #[test]
    fn evidence_must_describe_the_same_instance() {
        let (path, deriver) = CASES[0];
        let observation = observation(path, deriver, Some("MIT"), Some(false));
        let same = EvidenceBundle {
            observation: observation.clone(),
            sources: vec![
                EvidenceSource { kind: EvidenceSourceKind::Output, reference: path.into(), digest: "sha256-a".into() },
                EvidenceSource { kind: EvidenceSourceKind::UpstreamDocumentation, reference: "https://example.invalid/LICENSE".into(), digest: "sha256-b".into() },
            ],
        };
        assert_eq!(evidence_matches_instance(&same), Ok(()));
        let other = EvidenceBundle {
            observation: observation.clone(),
            sources: vec![EvidenceSource { kind: EvidenceSourceKind::Output, reference: CASES[1].0.into(), digest: "sha256-c".into() }],
        };
        assert_eq!(evidence_matches_instance(&other), Err(EvidenceRefusal::DifferentInstance));
        let docs_only = EvidenceBundle {
            observation,
            sources: vec![EvidenceSource { kind: EvidenceSourceKind::UpstreamDocumentation, reference: "https://example.invalid/LICENSE".into(), digest: "sha256-d".into() }],
        };
        assert_eq!(evidence_matches_instance(&docs_only), Err(EvidenceRefusal::MissingPrimarySource));
    }

    #[test]
    fn missing_evidence_is_fail_closed() {
        let (path, deriver) = CASES[0];
        assert_eq!(
            classify_identity(&mandate(), true, &observation(path, deriver, None, None), LeadClassificationDecision::Accept),
            Err(ClassificationRefusal::MissingEvidence)
        );
        assert_eq!(
            classify_identity(&mandate(), true, &observation(path, deriver, None, None), LeadClassificationDecision::NeedsEvidence),
            Err(ClassificationRefusal::NeedsEvidence)
        );
    }

    #[test]
    fn unfree_denied_unknown_revoked_or_out_of_scope_are_refused() {
        let (path, deriver) = CASES[0];
        assert_eq!(
            classify_identity(&mandate(), true, &observation(path, deriver, Some("MIT"), Some(true)), LeadClassificationDecision::Accept),
            Err(ClassificationRefusal::Unfree)
        );
        assert_eq!(
            classify_identity(&mandate(), true, &observation(path, deriver, Some("NOASSERTION"), Some(false)), LeadClassificationDecision::Accept),
            Err(ClassificationRefusal::LicenseDenied)
        );
        assert_eq!(
            classify_identity(&mandate(), true, &observation(path, deriver, Some("Not-A-Real-Licence"), Some(false)), LeadClassificationDecision::Accept),
            Err(ClassificationRefusal::LicenseUnknown)
        );
        let mut revoked = mandate();
        revoked.revoked = true;
        assert_eq!(
            classify_identity(&revoked, true, &observation(path, deriver, Some("MIT"), Some(false)), LeadClassificationDecision::Accept),
            Err(ClassificationRefusal::MandateRevoked)
        );
        assert_eq!(
            classify_identity(&mandate(), false, &observation(path, deriver, Some("MIT"), Some(false)), LeadClassificationDecision::Accept),
            Err(ClassificationRefusal::ActorOutOfScope)
        );
    }
}
