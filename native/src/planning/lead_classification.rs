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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClassificationMandate {
    pub scope: String,
    pub revision: u64,
    pub revoked: bool,
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
        })
    }
}

impl std::error::Error for ClassificationRefusal {}

fn is_sha256(value: &str) -> bool {
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

/// Deterministic Core decision for a class-level Lead classification.
///
/// `actor_in_scope` must come from Core's own verification of the Lead actor and
/// its active delegation; it is never read from a request payload. The evidence
/// bundle, the exact identity and the mandate are all validated in this one
/// decision, and any incomplete or unverified input yields no dictum.
pub fn classify_with_evidence(
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
    if mandate.revoked {
        return Err(ClassificationRefusal::MandateRevoked);
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
            revoked: false,
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
        let mut revoked = mandate();
        revoked.revoked = true;
        assert_eq!(
            classify_with_evidence(
                &revoked,
                Some(&bundle(path, deriver, Some("MIT"), Some(false))),
                LeadClassificationDecision::Accept
            ),
            Err(ClassificationRefusal::MandateRevoked)
        );
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
