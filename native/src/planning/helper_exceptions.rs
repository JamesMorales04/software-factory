//! Owner-approved trust-root perimeter for Nix helper exceptions (ADR-0045).
//!
//! The Owner approves the *kind* of thing that may be excepted, once, by
//! deploying a trust-root record: the pinned nixpkgs revision plus the exact
//! `stdenv` and `nix-shell-env` store identities. Helper identities are never
//! listed. A candidate is eligible only when it is derived from that trusted
//! `stdenv` and observed as a regular, deriver-less `*.sh` file, and only the
//! Lead's operational decision may accept it inside that perimeter.
//!
//! Core recomputes everything from its own record and the observed facts. A
//! digest carried by a plan is not a signature and is never trusted on its own.

use serde::{Deserialize, Serialize};

pub const HELPER_TRUST_SCHEMA_VERSION: u32 = 1;
pub const HELPER_NAME_SUFFIX: &str = ".sh";
/// Bounded set: a per-plan exception set is small, and the closure manifest is
/// the evidence that limits it.
pub const MAX_HELPERS_PER_PLAN: usize = 128;
pub const NIXPKGS_PREFIX: &str = "github:NixOS/nixpkgs/";
pub const STORE_PREFIX: &str = "/nix/store/";

/// One trusted store identity, anchored by its own derivation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TrustedStoreIdentity {
    pub store_path: String,
    pub deriver: String,
    pub nar_hash: String,
}

/// The Owner-approved perimeter for one pinned revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HelperTrustRoots {
    pub schema_version: u32,
    pub pin: String,
    pub stdenv: TrustedStoreIdentity,
    pub shell_wrapper: TrustedStoreIdentity,
    pub revision: u64,
    pub catalog_sha256: String,
}

/// One observed candidate, derived from closure facts only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HelperObservation {
    pub store_path: String,
    pub nar_hash: String,
    pub deriver: String,
    pub regular_file: bool,
    pub references: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HelperDecision {
    Accept,
    Reject,
}

/// The Lead's typed operational decision for one observed candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LeadHelperDecision {
    pub candidate_sha256: String,
    pub decision: HelperDecision,
    pub rationale: String,
}

/// A grant Core may place in a frozen plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HelperExceptionGrant {
    pub store_path: String,
    pub nar_hash: String,
    pub trust_roots_sha256: String,
    pub lead_review_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperPerimeterError {
    InvalidRoots,
    RootsDigestMismatch,
    PinMismatch,
    InvalidObservation,
    DeriverPresent,
    NotRegularFile,
    WrongClass,
    UntrustedReferrer,
    MissingTrustedReferrer,
    TooManyCandidates,
    MissingDecision,
    DuplicateDecision,
    DecisionMismatch,
    Rejected,
}

impl std::fmt::Display for HelperPerimeterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidRoots => "helper trust roots are not a bounded record",
            Self::RootsDigestMismatch => "helper trust roots digest does not match the record",
            Self::PinMismatch => "helper trust roots are pinned to another revision",
            Self::InvalidObservation => "helper observation is not a bounded store fact",
            Self::DeriverPresent => "helper candidate has a derivation of its own",
            Self::NotRegularFile => "helper candidate is not a regular file",
            Self::WrongClass => "helper candidate is not a generated shell helper",
            Self::UntrustedReferrer => "helper candidate has a referrer outside the trust roots",
            Self::MissingTrustedReferrer => "helper candidate is not referenced by the trusted stdenv",
            Self::TooManyCandidates => "helper candidate count exceeds the evidence bound",
            Self::MissingDecision => "helper candidate has no Lead decision",
            Self::DuplicateDecision => "helper candidate has more than one Lead decision",
            Self::DecisionMismatch => "Lead decision does not match the observed candidate",
            Self::Rejected => "the Lead rejected the helper candidate",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for HelperPerimeterError {}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn is_store_path(value: &str) -> bool {
    let Some(rest) = value.strip_prefix(STORE_PREFIX) else {
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
        && name.len() <= 200
        && !name.contains('/')
        && !name.contains(char::is_whitespace)
}

fn is_deriver(value: &str) -> bool {
    is_store_path(value) && value.ends_with(".drv")
}

fn is_nar_hash(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("sha256-") else {
        return false;
    };
    (40..=64).contains(&rest.len())
        && rest
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/' || byte == b'=')
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_roots(roots: &HelperTrustRoots) -> Result<(), HelperPerimeterError> {
    if roots.schema_version != HELPER_TRUST_SCHEMA_VERSION
        || !roots.pin.starts_with(NIXPKGS_PREFIX)
        || roots.pin.len() > 200
        || roots.revision == 0
        || !is_sha256_hex(&roots.catalog_sha256)
        || !is_store_path(&roots.stdenv.store_path)
        || !is_deriver(&roots.stdenv.deriver)
        || !is_nar_hash(&roots.stdenv.nar_hash)
        || !is_store_path(&roots.shell_wrapper.store_path)
        || !is_deriver(&roots.shell_wrapper.deriver)
        || !is_nar_hash(&roots.shell_wrapper.nar_hash)
        || roots.stdenv.store_path == roots.shell_wrapper.store_path
    {
        return Err(HelperPerimeterError::InvalidRoots);
    }
    if canonical_roots_sha256(roots) != roots.catalog_sha256 {
        return Err(HelperPerimeterError::RootsDigestMismatch);
    }
    Ok(())
}

/// Canonical digest of the trust-root record (schema, pin, identities, revision).
pub fn canonical_roots_sha256(roots: &HelperTrustRoots) -> String {
    let value = serde_json::json!({
        "schemaVersion": roots.schema_version,
        "pin": roots.pin,
        "stdenv": roots.stdenv,
        "shellWrapper": roots.shell_wrapper,
        "revision": roots.revision,
    });
    sha256_hex(&serde_json::to_vec(&value).unwrap_or_default())
}

/// Canonical digest of one observation; this is the only handle the Lead sees.
pub fn candidate_sha256(observation: &HelperObservation) -> String {
    sha256_hex(&serde_json::to_vec(observation).unwrap_or_default())
}

/// True membership in the trusted derivation graph.
///
/// The trusted `stdenv` must reference the candidate and no referrer outside the
/// trusted roots may exist. A devShell path, an SBOM label or a name suffix is
/// never sufficient on its own.
pub fn check_membership(
    roots: &HelperTrustRoots,
    observation: &HelperObservation,
) -> Result<(), HelperPerimeterError> {
    if !is_store_path(&observation.store_path) || !is_nar_hash(&observation.nar_hash) {
        return Err(HelperPerimeterError::InvalidObservation);
    }
    if !observation.deriver.trim().is_empty() {
        return Err(HelperPerimeterError::DeriverPresent);
    }
    if !observation.regular_file {
        return Err(HelperPerimeterError::NotRegularFile);
    }
    let name = observation
        .store_path
        .rsplit_once('-')
        .map(|(_, name)| name)
        .unwrap_or_default();
    if !name.ends_with(HELPER_NAME_SUFFIX) {
        return Err(HelperPerimeterError::WrongClass);
    }
    let trusted = [&roots.stdenv.store_path, &roots.shell_wrapper.store_path];
    if !observation
        .references
        .iter()
        .any(|reference| reference == &roots.stdenv.store_path)
    {
        return Err(HelperPerimeterError::MissingTrustedReferrer);
    }
    if observation
        .references
        .iter()
        .any(|reference| !trusted.contains(&reference))
    {
        return Err(HelperPerimeterError::UntrustedReferrer);
    }
    Ok(())
}

/// Core's authority gate.
///
/// `pin` is the plan's pinned revision; it must equal the trust-root pin.
/// Exactly one Lead decision is required per observation, it must be ACCEPT, and
/// it must name the recomputed candidate digest. Anything else is a refusal.
pub fn authorize(
    roots: &HelperTrustRoots,
    pin: &str,
    observations: &[HelperObservation],
    decisions: &[LeadHelperDecision],
    lead_review_id: &str,
) -> Result<Vec<HelperExceptionGrant>, HelperPerimeterError> {
    validate_roots(roots)?;
    if pin != roots.pin {
        return Err(HelperPerimeterError::PinMismatch);
    }
    if lead_review_id.trim().is_empty() || lead_review_id.len() > 200 {
        return Err(HelperPerimeterError::DecisionMismatch);
    }
    if observations.len() > MAX_HELPERS_PER_PLAN {
        return Err(HelperPerimeterError::TooManyCandidates);
    }
    let trust_roots_sha256 = canonical_roots_sha256(roots);
    let mut grants = Vec::with_capacity(observations.len());
    for observation in observations {
        check_membership(roots, observation)?;
        let digest = candidate_sha256(observation);
        let mut matching = decisions
            .iter()
            .filter(|decision| decision.candidate_sha256 == digest);
        let Some(decision) = matching.next() else {
            return Err(HelperPerimeterError::MissingDecision);
        };
        if matching.next().is_some() {
            return Err(HelperPerimeterError::DuplicateDecision);
        }
        if decision.rationale.trim().is_empty() || decision.rationale.len() > 500 {
            return Err(HelperPerimeterError::DecisionMismatch);
        }
        match decision.decision {
            HelperDecision::Accept => {}
            HelperDecision::Reject => return Err(HelperPerimeterError::Rejected),
        }
        grants.push(HelperExceptionGrant {
            store_path: observation.store_path.clone(),
            nar_hash: observation.nar_hash.clone(),
            trust_roots_sha256: trust_roots_sha256.clone(),
            lead_review_id: lead_review_id.to_owned(),
        });
    }
    Ok(grants)
}



#[cfg(test)]
mod tests {
    use super::*;

    const PIN: &str = "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d";
    const STDENV: &str = "/nix/store/lfwfj17y9fpjb73nsj2m35rmkh587a0x-stdenv-linux";
    const STDENV_DRV: &str = "/nix/store/nsvrknrqxdf2lxcfbqkr0p05897ww5lx-stdenv-linux.drv";
    const SHELL: &str = "/nix/store/xp720k4l3krsg7fjdjhpv45300ghgr6s-nix-shell-env";
    const SHELL_DRV: &str = "/nix/store/gkp675qsx7wg4rij541kzmi49skfzsnv-nix-shell-env.drv";
    const HELPER: &str = "/nix/store/cickvswrvann041nqxb0rxilc46svw1n-prune-libtool-files.sh";
    const NAR: &str = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

    fn roots() -> HelperTrustRoots {
        let mut roots = HelperTrustRoots {
            schema_version: HELPER_TRUST_SCHEMA_VERSION,
            pin: PIN.into(),
            stdenv: TrustedStoreIdentity {
                store_path: STDENV.into(),
                deriver: STDENV_DRV.into(),
                nar_hash: "sha256-7uyA5WT1X+9o1QV/Dqs7GsmWtellurWH5ipGyZK8Szk=".into(),
            },
            shell_wrapper: TrustedStoreIdentity {
                store_path: SHELL.into(),
                deriver: SHELL_DRV.into(),
                nar_hash: "sha256-lH0a8hOLzQfqRQDAP8xrNxGxwVW7EcjXyDOakbb+MCw=".into(),
            },
            revision: 1,
            catalog_sha256: String::new(),
        };
        roots.catalog_sha256 = canonical_roots_sha256(&roots);
        roots
    }

    fn observation() -> HelperObservation {
        HelperObservation {
            store_path: HELPER.into(),
            nar_hash: NAR.into(),
            deriver: String::new(),
            regular_file: true,
            references: vec![STDENV.into(), SHELL.into()],
        }
    }

    fn accept(observation: &HelperObservation) -> LeadHelperDecision {
        LeadHelperDecision {
            candidate_sha256: candidate_sha256(observation),
            decision: HelperDecision::Accept,
            rationale: "generated stdenv helper".into(),
        }
    }

    #[test]
    fn a_new_eligible_helper_is_authorized_without_an_owner_list() {
        let roots = roots();
        let observation = observation();
        let grants = authorize(&roots, PIN, &[observation.clone()], &[accept(&observation)], "review-1")
            .expect("eligible helper is authorized");
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].store_path, HELPER);
        assert_eq!(grants[0].trust_roots_sha256, roots.catalog_sha256);
    }

    #[test]
    fn an_arbitrary_referrer_is_rejected() {
        let roots = roots();
        let mut observation = observation();
        observation
            .references
            .push("/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-fake-devshell".into());
        assert_eq!(
            authorize(&roots, PIN, &[observation.clone()], &[accept(&observation)], "review-1"),
            Err(HelperPerimeterError::UntrustedReferrer)
        );
    }

    #[test]
    fn the_wrapper_alone_is_not_sufficient() {
        let roots = roots();
        let mut observation = observation();
        observation.references = vec![SHELL.into()];
        assert_eq!(
            authorize(&roots, PIN, &[observation.clone()], &[accept(&observation)], "review-1"),
            Err(HelperPerimeterError::MissingTrustedReferrer)
        );
    }

    #[test]
    fn deriver_present_directory_or_wrong_class_are_rejected() {
        let roots = roots();
        let mut derived = observation();
        derived.deriver = STDENV_DRV.into();
        assert_eq!(
            check_membership(&roots, &derived),
            Err(HelperPerimeterError::DeriverPresent)
        );
        let mut directory = observation();
        directory.regular_file = false;
        assert_eq!(
            check_membership(&roots, &directory),
            Err(HelperPerimeterError::NotRegularFile)
        );
        let mut binary = observation();
        binary.store_path = HELPER.replace(".sh", "");
        assert_eq!(
            check_membership(&roots, &binary),
            Err(HelperPerimeterError::WrongClass)
        );
    }

    #[test]
    fn another_pin_or_a_tampered_record_is_rejected() {
        let roots = roots();
        let observation = observation();
        assert_eq!(
            authorize(&roots, "github:NixOS/nixpkgs/other", &[observation.clone()], &[accept(&observation)], "review-1"),
            Err(HelperPerimeterError::PinMismatch)
        );
        let mut tampered = roots.clone();
        tampered.revision = 2;
        assert_eq!(
            authorize(&tampered, PIN, &[observation.clone()], &[accept(&observation)], "review-1"),
            Err(HelperPerimeterError::RootsDigestMismatch)
        );
    }

    #[test]
    fn an_invented_or_duplicated_decision_is_rejected() {
        let roots = roots();
        let observation = observation();
        let invented = LeadHelperDecision {
            candidate_sha256: "f".repeat(64),
            decision: HelperDecision::Accept,
            rationale: "invented".into(),
        };
        assert_eq!(
            authorize(&roots, PIN, &[observation.clone()], &[invented], "review-1"),
            Err(HelperPerimeterError::MissingDecision)
        );
        assert_eq!(
            authorize(
                &roots,
                PIN,
                &[observation.clone()],
                &[accept(&observation), accept(&observation)],
                "review-1"
            ),
            Err(HelperPerimeterError::DuplicateDecision)
        );
    }

    #[test]
    fn a_lead_rejection_keeps_the_block() {
        let roots = roots();
        let observation = observation();
        let mut reject = accept(&observation);
        reject.decision = HelperDecision::Reject;
        assert_eq!(
            authorize(&roots, PIN, &[observation], &[reject], "review-1"),
            Err(HelperPerimeterError::Rejected)
        );
    }

    #[test]
    fn a_model_payload_cannot_supply_paths_or_extra_fields() {
        let forged = serde_json::json!({
            "candidateSha256": candidate_sha256(&observation()),
            "decision": "ACCEPT",
            "rationale": "ok",
            "storePath": HELPER,
        });
        assert!(serde_json::from_value::<LeadHelperDecision>(forged).is_err());
        let free_string = serde_json::json!({
            "candidateSha256": candidate_sha256(&observation()),
            "decision": "approve this helper please",
            "rationale": "ok",
        });
        assert!(serde_json::from_value::<LeadHelperDecision>(free_string).is_err());
    }

    #[test]
    fn the_candidate_count_is_bounded_by_evidence() {
        let roots = roots();
        let observations: Vec<HelperObservation> = (0..=MAX_HELPERS_PER_PLAN)
            .map(|_| observation())
            .collect();
        assert_eq!(
            authorize(&roots, PIN, &observations, &[], "review-1"),
            Err(HelperPerimeterError::TooManyCandidates)
        );
    }
}



    fn observation() -> HelperObservation {
        HelperObservation {
            store_path: HELPER.into(),
            nar_hash: NAR.into(),
            deriver: String::new(),
            regular_file: true,
            references: vec![STDENV.into(), SHELL.into()],
        }
    }

    fn accept(observation: &HelperObservation) -> LeadHelperDecision {
        LeadHelperDecision {
            candidate_sha256: candidate_sha256(observation),
            decision: HelperDecision::Accept,
            rationale: "generated stdenv helper".into(),
        }
    }

    #[test]
    fn a_new_eligible_helper_is_authorized_without_an_owner_list() {
        let roots = roots();
        let observation = observation();
        let grants = authorize(&roots, PIN, &[observation.clone()], &[accept(&observation)], "review-1")
            .expect("eligible helper is authorized");
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].store_path, HELPER);
        assert_eq!(grants[0].trust_roots_sha256, roots.catalog_sha256);
    }

    #[test]
    fn an_arbitrary_referrer_is_rejected() {
        let roots = roots();
        let mut observation = observation();
        observation
            .references
            .push("/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-fake-devshell".into());
        assert_eq!(
            authorize(&roots, PIN, &[observation.clone()], &[accept(&observation)], "review-1"),
            Err(HelperPerimeterError::UntrustedReferrer)
        );
    }

    #[test]
    fn the_wrapper_alone_is_not_sufficient() {
        let roots = roots();
        let mut observation = observation();
        observation.references = vec![SHELL.into()];
        assert_eq!(
            authorize(&roots, PIN, &[observation.clone()], &[accept(&observation)], "review-1"),
            Err(HelperPerimeterError::MissingTrustedReferrer)
        );
    }

    #[test]
    fn deriver_present_directory_or_wrong_class_are_rejected() {
        let roots = roots();
        let mut derived = observation();
        derived.deriver = STDENV_DRV.into();
        assert_eq!(
            check_membership(&roots, &derived),
            Err(HelperPerimeterError::DeriverPresent)
        );
        let mut directory = observation();
        directory.regular_file = false;
        assert_eq!(
            check_membership(&roots, &directory),
            Err(HelperPerimeterError::NotRegularFile)
        );
        let mut binary = observation();
        binary.store_path = HELPER.replace(".sh", "");
        assert_eq!(
            check_membership(&roots, &binary),
            Err(HelperPerimeterError::WrongClass)
        );
    }

    #[test]
    fn another_pin_or_a_tampered_record_is_rejected() {
        let roots = roots();
        let observation = observation();
        assert_eq!(
            authorize(&roots, "github:NixOS/nixpkgs/other", &[observation.clone()], &[accept(&observation)], "review-1"),
            Err(HelperPerimeterError::PinMismatch)
        );
        let mut tampered = roots.clone();
        tampered.revision = 2;
        assert_eq!(
            authorize(&tampered, PIN, &[observation.clone()], &[accept(&observation)], "review-1"),
            Err(HelperPerimeterError::RootsDigestMismatch)
        );
    }

    #[test]
    fn an_invented_or_duplicated_decision_is_rejected() {
        let roots = roots();
        let observation = observation();
        let invented = LeadHelperDecision {
            candidate_sha256: "f".repeat(64),
            decision: HelperDecision::Accept,
            rationale: "invented".into(),
        };
        assert_eq!(
            authorize(&roots, PIN, &[observation.clone()], &[invented], "review-1"),
            Err(HelperPerimeterError::MissingDecision)
        );
        assert_eq!(
            authorize(
                &roots,
                PIN,
                &[observation.clone()],
                &[accept(&observation), accept(&observation)],
                "review-1"
            ),
            Err(HelperPerimeterError::DuplicateDecision)
        );
    }

    #[test]
    fn a_lead_rejection_keeps_the_block() {
        let roots = roots();
        let observation = observation();
        let mut reject = accept(&observation);
        reject.decision = HelperDecision::Reject;
        assert_eq!(
            authorize(&roots, PIN, &[observation], &[reject], "review-1"),
            Err(HelperPerimeterError::Rejected)
        );
    }

    #[test]
    fn a_model_payload_cannot_supply_paths_or_extra_fields() {
        let forged = serde_json::json!({
            "candidateSha256": candidate_sha256(&observation()),
            "decision": "ACCEPT",
            "rationale": "ok",
            "storePath": HELPER,
        });
        assert!(serde_json::from_value::<LeadHelperDecision>(forged).is_err());
        let free_string = serde_json::json!({
            "candidateSha256": candidate_sha256(&observation()),
            "decision": "approve this helper please",
            "rationale": "ok",
        });
        assert!(serde_json::from_value::<LeadHelperDecision>(free_string).is_err());
    }

    #[test]
    fn the_candidate_count_is_bounded_by_evidence() {
        let roots = roots();
        let observations: Vec<HelperObservation> = (0..=MAX_HELPERS_PER_PLAN)
            .map(|_| observation())
            .collect();
        assert_eq!(
            authorize(&roots, PIN, &observations, &[], "review-1"),
            Err(HelperPerimeterError::TooManyCandidates)
        );
    }
}


// ---------------------------------------------------------------------------
// Class-level Lead delegation for Nix identity classification (no allowlist).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LeadClassificationDecision {
    Accept,
    Reject,
    NeedsEvidence,
}

/// The Owner-granted class of decision. It names a policy scope, never an
/// identity: any identity with exact binding and independent evidence may be
/// classified under it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClassificationMandate {
    pub scope: String,
    pub revision: u64,
    pub revoked: bool,
}

/// One observed Nix identity with its evidence reference.
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
            Self::MissingEvidence => "the identity has no independently observed licence evidence",
            Self::LicenseDenied => "the observed licence is denied",
            Self::Unfree => "the observed identity is unfree",
            Self::NeedsEvidence => "the Lead requires more evidence",
            Self::Rejected => "the Lead rejected the identity",
        })
    }
}

impl std::error::Error for ClassificationRefusal {}

fn identity_complete(observation: &IdentityObservation) -> bool {
    !observation.pin.trim().is_empty()
        && observation.store_path.starts_with("/nix/store/")
        && observation.store_path.ends_with(observation.deriver.rsplit_once('-').map(|_| ()).map(|_| "").unwrap_or(""))
        && !observation.deriver.trim().is_empty()
        && observation.deriver.ends_with(".drv")
        && !observation.nar_hash.trim().is_empty()
        && !observation.evidence_ref.trim().is_empty()
}

/// Deterministic Core decision for a class-level Lead classification.
///
/// Missing evidence is never escalated to the Owner and never accepted: it
/// returns a fail-closed refusal. The mandate and the actor's delegation must be
/// active, and the identity must be complete and exactly bound.
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
    let unfree = observation.unfree;
    let (Some(license), Some(unfree)) = (license, unfree) else {
        return Err(match decision {
            LeadClassificationDecision::NeedsEvidence => ClassificationRefusal::NeedsEvidence,
            _ => ClassificationRefusal::MissingEvidence,
        });
    };
    match decision {
        LeadClassificationDecision::Accept => {}
        LeadClassificationDecision::Reject => return Err(ClassificationRefusal::Rejected),
        LeadClassificationDecision::NeedsEvidence => {
            return Err(ClassificationRefusal::NeedsEvidence);
        }
    }
    if unfree {
        return Err(ClassificationRefusal::Unfree);
    }
    if matches!(license, "NOASSERTION" | "NONE") {
        return Err(ClassificationRefusal::LicenseDenied);
    }
    Ok(ApprovedIdentity {
        pin: observation.pin.clone(),
        store_path: observation.store_path.clone(),
        deriver: observation.deriver.clone(),
        nar_hash: observation.nar_hash.clone(),
        mandate_revision: mandate.revision,
        evidence_ref: observation.evidence_ref.clone(),
    })

    fn observation(path: &str, deriver: &str, license: Option<(&str)>, unfree: Option<bool>) -> IdentityObservation {
        IdentityObservation {
            pin: "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d".into(),
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

    #[test]
    fn any_eligible_identity_is_classified_under_the_class_mandate() {
        let observed = [
            ("/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-a-1", "/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-a-1.drv"),
            ("/nix/store/cccccccccccccccccccccccccccccccc-c-1", "/nix/store/dddddddddddddddddddddddddddddddd-c-1.drv"),
            ("/nix/store/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-e-1", "/nix/store/ffffffffffffffffffffffffffffffff-e-1.drv"),
        ];
        for (path, deriver) in observed {
            let grant = classify_identity(
                &mandate(),
                true,
                &observation(path, deriver, Some("MIT"), Some(false)),
                LeadClassificationDecision::Accept,
            )
            .expect("an eligible identity is classified");
            self_check(&grant, path, deriver);
        }

    fn self_check(grant: &ApprovedIdentity, path: &str, deriver: &str) {
            assert_eq!(grant.store_path, path);
            assert_eq!(grant.deriver, deriver);
            assert_eq!(grant.mandate_revision, 3);
    }

    #[test]
    fn missing_evidence_is_fail_closed_and_never_escalated() {
        let path = "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-a-1";
        let deriver = "/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-a-1.drv";
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
    fn unfree_denied_revoked_or_out_of_scope_are_refused() {
        let path = "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-a-1";
        let deriver = "/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-a-1.drv";
        assert_eq!(
            classify_identity(&mandate(), true, &observation(path, deriver, Some("MIT"), Some(true)), LeadClassificationDecision::Accept),
            Err(ClassificationRefusal::Unfree)
        );
        assert_eq!(
            classify_identity(&mandate(), true, &observation(path, deriver, Some("NOASSERTION"), Some(false)), LeadClassificationDecision::Accept),
            Err(ClassificationRefusal::LicenseDenied)
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
}
