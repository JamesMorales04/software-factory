//! Identity-facts manifest for the environment broker (ADR-0045).
//!
//! This module defines the *complete* facts manifest the broker must sign and
//! Core must verify: every fact carries the exact store path, derivation, NAR
//! hash and output paths. The manifest is never paginated; a closure larger than
//! the documented technical bound is refused instead of truncated.
//!
//! The broker produces the manifest from its own fixed Nix queries over the
//! realised store and authenticates it with a context-derived MAC of the
//! existing scoped environment bearer. The builder never sees that key and
//! never signs anything, and a model or builder-supplied manifest is rejected
//! by construction because only the broker-fixed query produces this envelope.

use ring::hmac;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::environment::{EnvironmentBuildReceipt, EnvironmentBuilderStatus, EnvironmentSpec};
use super::lead_classification::is_sha256;

/// Separate HMAC context; identity facts are not helper facts.
pub const IDENTITY_FACTS_CONTEXT: &str = "factory.environment.identity-facts.v1";
pub const IDENTITY_FACTS_SCHEMA_VERSION: u32 = 1;
/// Bound for Core-issued exact questions (`QUESTIONS` mode).
pub const MAX_IDENTITY_QUESTIONS: usize = 64;
/// Bound for the declared output paths of one derivation.
pub const MAX_IDENTITY_OUTPUTS: usize = 16;
/// Documented technical bound. It must comfortably exceed a real closure (73 in
/// Run10); exceeding it is a refusal, never a truncated manifest.
pub const MAX_IDENTITY_FACTS: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IdentityFactsMode {
    /// Discover the closure of the broker's realised root. `questions` is empty.
    Enumerate,
    /// Answer exactly the Core-issued questions.
    Questions,
}

/// Core-issued root selector: the broker resolves it against its own operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OperationRoot {
    Operation,
    Plan,
    Runtime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IdentityQuestion {
    pub store_path: String,
    #[serde(default)]
    pub deriver: Option<String>,
    pub nar_hash: String,
    pub outputs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IdentityFact {
    pub store_path: String,
    /// `None` is the explicit representation of a Nix path with no derivation;
    /// it is never omitted and never invented.
    #[serde(default)]
    pub deriver: Option<String>,
    pub nar_hash: String,
    pub outputs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IdentityFactsManifest {
    /// The root the broker actually observed, echoed and signed.
    pub observed_root: String,
    pub facts: Vec<IdentityFact>,
    pub fact_count: usize,
    pub closure_manifest_sha256: String,
    /// Always false. A truncated manifest is refused, so this can only be set by
    /// a producer that would then be rejected by Core.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IdentityFactsChallenge {
    pub operation_id: String,
    pub runtime_id: String,
    pub nonce: String,
    pub pin: String,
    pub plan_sha256: String,
    pub spec_sha256: String,
    pub mode: IdentityFactsMode,
    pub root: OperationRoot,
    pub questions: Vec<IdentityQuestion>,
}

impl IdentityFactsChallenge {
    /// Validate the Core-issued challenge before it is dispatched. Bounds are
    /// applied to every field so a broker never runs a fixed query on an
    /// oversized or incomplete binding.
    pub fn validate_shape(&self) -> Result<(), IdentityFactsError> {
        let bounded = |value: &str, max: usize, allow_controls: bool| {
            !value.trim().is_empty()
                && value.len() <= max
                && (allow_controls || !value.chars().any(char::is_control))
        };
        if !bounded(&self.operation_id, 200, false)
            || !bounded(&self.runtime_id, 200, false)
            || !bounded(&self.nonce, 200, false)
            || !bounded(&self.pin, 300, false)
            || !is_sha256(&self.plan_sha256)
            || !is_sha256(&self.spec_sha256)
            || self.questions.len() > MAX_IDENTITY_QUESTIONS
        {
            return Err(IdentityFactsError::InvalidChallenge);
        }
        let mut paths = std::collections::BTreeSet::new();
        for question in &self.questions {
            if !is_output_store_path(&question.store_path)
                || !paths.insert(question.store_path.as_str())
                || !is_sha256(&question.nar_hash)
                || question
                    .deriver
                    .as_deref()
                    .is_some_and(|deriver| !is_store_path(deriver) || !deriver.ends_with(".drv"))
                || question.outputs.is_empty()
                || question.outputs.len() > MAX_IDENTITY_OUTPUTS
                || question
                    .outputs
                    .iter()
                    .any(|output| !is_output_store_path(output))
                || question.outputs.len()
                    != question
                        .outputs
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                || !question.outputs.contains(&question.store_path)
                || (question.deriver.is_none()
                    && question.outputs != vec![question.store_path.clone()])
            {
                return Err(IdentityFactsError::InvalidChallenge);
            }
        }
        // The producer resolves the realised store of its own operation. It does
        // not implement a plan- or runtime-rooted resolution, so those selectors
        // are refused instead of being silently treated as the operation root.
        if self.root != OperationRoot::Operation {
            return Err(IdentityFactsError::InvalidChallenge);
        }
        match self.mode {
            IdentityFactsMode::Enumerate if !self.questions.is_empty() => {
                Err(IdentityFactsError::InvalidChallenge)
            }
            IdentityFactsMode::Questions if self.questions.is_empty() => {
                Err(IdentityFactsError::InvalidChallenge)
            }
            _ => Ok(()),
        }
    }
}

/// The signed envelope the broker returns to Core. It echoes the complete
/// Core-issued challenge and the complete manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IdentityFactsEnvelope {
    pub schema_version: u32,
    pub context: String,
    pub challenge: IdentityFactsChallenge,
    pub manifest: IdentityFactsManifest,
}

/// Envelope plus its MAC. The signature is outside the signed bytes, so a
/// receipt never signs its own tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct IdentityFactsReceipt {
    pub envelope: IdentityFactsEnvelope,
    pub signature: String,
}

/// Receipt that passed MAC and binding verification.
///
/// The field is private and only [`verify`] (and the feature-gated fixture) can
/// construct this type. The durable consumer accepts only this type, so a
/// structurally plausible forged envelope cannot be persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedIdentityFacts(IdentityFactsReceipt);

impl VerifiedIdentityFacts {
    pub fn receipt(&self) -> &IdentityFactsReceipt {
        &self.0
    }
}

/// How Core bound the observed root to the operation outcome.
///
/// A successful build proves the exact realised `storePath`, so Core requires
/// equality. A refused build has no successful receipt to compare; its identity
/// receipt is retained as an operation observation and is never converted into
/// an accepted build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IdentityFactsRootBinding {
    BuildReceipt,
    /// Broker observation of the exact spec-keyed cache, used when no
    /// successful build receipt exists (for example a refused licence gate).
    /// It proves what the broker observed, not which attempt wrote the root.
    SpecCacheObservation,
}

impl IdentityFactsRootBinding {
    pub const fn as_db_value(self) -> &'static str {
        match self {
            Self::BuildReceipt => "BUILD_RECEIPT",
            Self::SpecCacheObservation => "SPEC_CACHE_OBSERVATION",
        }
    }
}

/// Receipt plus a root binding proved by the producer of this value.
///
/// `bind_build_receipt` is the only constructor that can produce
/// `BUILD_RECEIPT`: it requires a successful build receipt whose exact store
/// path equals the signed observed root. `spec_cache_observation` is the
/// explicit weaker binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundIdentityFacts {
    verified: VerifiedIdentityFacts,
    binding: IdentityFactsRootBinding,
}

impl BoundIdentityFacts {
    pub fn bind_build_receipt(
        verified: VerifiedIdentityFacts,
        receipt: &EnvironmentBuildReceipt,
        spec: &EnvironmentSpec,
    ) -> Result<Self, IdentityFactsError> {
        receipt
            .validate_for(spec)
            .map_err(|error| IdentityFactsError::BuildReceiptInvalid(error.message))?;
        if receipt.spec_sha256 != verified.0.envelope.challenge.spec_sha256 {
            return Err(IdentityFactsError::RootBindingMismatch);
        }
        if receipt.store_path.is_empty()
            || !is_output_store_path(&receipt.store_path)
            || receipt.store_path != verified.0.envelope.manifest.observed_root
        {
            return Err(IdentityFactsError::RootBindingMismatch);
        }
        Ok(Self {
            verified,
            binding: IdentityFactsRootBinding::BuildReceipt,
        })
    }

    pub fn spec_cache_observation(verified: VerifiedIdentityFacts) -> Self {
        Self {
            verified,
            binding: IdentityFactsRootBinding::SpecCacheObservation,
        }
    }

    pub fn verified(&self) -> &VerifiedIdentityFacts {
        &self.verified
    }

    pub fn binding(&self) -> IdentityFactsRootBinding {
        self.binding
    }

    pub fn receipt(&self) -> &IdentityFactsReceipt {
        self.verified.receipt()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityFactsRefusal {
    EmptyManifest,
    TooManyFacts,
    Truncated,
    CountMismatch,
    DuplicatePath,
    IncompleteFact,
    RootNotObserved,
    DigestMismatch,
    QuestionMismatch,
}

impl std::fmt::Display for IdentityFactsRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::EmptyManifest => "the identity-facts manifest is empty",
            Self::TooManyFacts => "the identity-facts manifest exceeds its technical bound",
            Self::Truncated => "a truncated identity-facts manifest is never accepted",
            Self::CountMismatch => "the manifest fact count does not match its facts",
            Self::DuplicatePath => "the manifest repeats a store path",
            Self::IncompleteFact => "a fact is missing its derivation, hash or outputs",
            Self::RootNotObserved => "the observed root is not part of the closure",
            Self::DigestMismatch => "the closure manifest digest does not match",
            Self::QuestionMismatch => "the manifest does not answer the issued questions",
        })
    }
}

impl std::error::Error for IdentityFactsRefusal {}

/// Typed failure of the authenticated identity-facts channel. `EvidenceAbsent`
/// is not a silent success: Core must record an explicit blocker and keep the
/// environment blocked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityFactsError {
    InvalidChallenge,
    InvalidEnvelope,
    ContextMismatch,
    BindingMismatch,
    SignatureMismatch,
    RootBindingMismatch,
    BuildReceiptInvalid(String),
    EvidenceAbsent,
    Refused(IdentityFactsRefusal),
}

impl std::fmt::Display for IdentityFactsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidChallenge => {
                formatter.write_str("the identity-facts challenge is invalid")
            }
            Self::InvalidEnvelope => formatter.write_str("the identity-facts envelope is invalid"),
            Self::ContextMismatch => {
                formatter.write_str("the identity-facts context does not match")
            }
            Self::BindingMismatch => {
                formatter.write_str("the identity facts are bound to another challenge")
            }
            Self::SignatureMismatch => {
                formatter.write_str("the identity-facts signature does not verify")
            }
            Self::RootBindingMismatch => formatter
                .write_str("the identity-facts root does not bind the successful build receipt"),
            Self::BuildReceiptInvalid(message) => {
                write!(formatter, "the build receipt is invalid: {message}")
            }
            Self::EvidenceAbsent => {
                formatter.write_str("the broker returned no identity-facts evidence")
            }
            Self::Refused(refusal) => write!(formatter, "{refusal}"),
        }
    }
}

impl std::error::Error for IdentityFactsError {}

impl From<IdentityFactsRefusal> for IdentityFactsError {
    fn from(refusal: IdentityFactsRefusal) -> Self {
        Self::Refused(refusal)
    }
}

fn is_output_store_path(value: &str) -> bool {
    is_store_path(value) && !value.ends_with(".drv")
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

/// Canonical digest over the manifest facts, in the order they are listed.
pub fn closure_manifest_sha256(facts: &[IdentityFact]) -> String {
    // Serialization of a plain struct of strings cannot fail; if it ever does we
    // must not silently hash an empty buffer.
    let canonical = serde_json::to_vec(facts).expect("identity facts serialise");
    let digest = Sha256::digest(&canonical);
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

/// Derive the context-separated MAC key from the shared environment bearer.
/// The bearer is the HMAC key and the context is the signed message, so the
/// raw bearer is never used directly as a MAC key.
pub fn derive_key(bearer: &[u8]) -> [u8; 32] {
    let key = hmac::Key::new(hmac::HMAC_SHA256, bearer);
    let tag = hmac::sign(&key, IDENTITY_FACTS_CONTEXT.as_bytes());
    let mut derived = [0u8; 32];
    derived.copy_from_slice(&tag.as_ref()[..32]);
    derived
}

fn canonical_envelope_bytes(
    envelope: &IdentityFactsEnvelope,
) -> Result<Vec<u8>, IdentityFactsError> {
    serde_json::to_vec(envelope).map_err(|_| IdentityFactsError::InvalidEnvelope)
}

fn decode_signature(signature: &str) -> Result<[u8; 32], IdentityFactsError> {
    if signature.len() != 64 || !signature.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(IdentityFactsError::SignatureMismatch);
    }
    let mut tag = [0u8; 32];
    for (index, chunk) in signature.as_bytes().chunks(2).enumerate() {
        let text = std::str::from_utf8(chunk).map_err(|_| IdentityFactsError::SignatureMismatch)?;
        tag[index] =
            u8::from_str_radix(text, 16).map_err(|_| IdentityFactsError::SignatureMismatch)?;
    }
    Ok(tag)
}

/// Sign one envelope (broker side). The manifest is validated against the
/// echoed challenge first, so a producer can never sign an incomplete or
/// oversized closure.
pub fn sign(envelope: &IdentityFactsEnvelope, bearer: &[u8]) -> Result<String, IdentityFactsError> {
    validate_envelope(envelope)?;
    let bytes = canonical_envelope_bytes(envelope)?;
    let key = hmac::Key::new(hmac::HMAC_SHA256, &derive_key(bearer));
    let tag = hmac::sign(&key, &bytes);
    let mut output = String::with_capacity(64);
    for byte in tag.as_ref() {
        output.push_str(&format!("{byte:02x}"));
    }
    Ok(output)
}

/// Verify a receipt (Core side): envelope shape, context, the exact echoed
/// challenge, the manifest rules and the MAC.
pub fn verify(
    receipt: &IdentityFactsReceipt,
    bearer: &[u8],
    expected: &IdentityFactsChallenge,
) -> Result<VerifiedIdentityFacts, IdentityFactsError> {
    validate_envelope(&receipt.envelope)?;
    if receipt.envelope.context != IDENTITY_FACTS_CONTEXT {
        return Err(IdentityFactsError::ContextMismatch);
    }
    if &receipt.envelope.challenge != expected {
        return Err(IdentityFactsError::BindingMismatch);
    }
    validate_manifest(expected, &receipt.envelope.manifest)?;
    let tag = decode_signature(&receipt.signature)?;
    let bytes = canonical_envelope_bytes(&receipt.envelope)?;
    let key = hmac::Key::new(hmac::HMAC_SHA256, &derive_key(bearer));
    hmac::verify(&key, &bytes, &tag).map_err(|_| IdentityFactsError::SignatureMismatch)?;
    Ok(VerifiedIdentityFacts(receipt.clone()))
}

fn validate_envelope(envelope: &IdentityFactsEnvelope) -> Result<(), IdentityFactsError> {
    if envelope.schema_version != IDENTITY_FACTS_SCHEMA_VERSION {
        return Err(IdentityFactsError::InvalidEnvelope);
    }
    envelope.challenge.validate_shape()?;
    Ok(())
}

/// Validate a manifest against its challenge.
///
/// Enumerate mode requires at least one fact (an empty discovery is refused, so
/// an "empty questions" request can never produce an empty receipt). Questions
/// mode requires exactly the issued store paths.
pub fn validate_manifest(
    challenge: &IdentityFactsChallenge,
    manifest: &IdentityFactsManifest,
) -> Result<(), IdentityFactsRefusal> {
    if manifest.truncated {
        return Err(IdentityFactsRefusal::Truncated);
    }
    if manifest.facts.is_empty() {
        return Err(IdentityFactsRefusal::EmptyManifest);
    }
    if manifest.facts.len() > MAX_IDENTITY_FACTS {
        return Err(IdentityFactsRefusal::TooManyFacts);
    }
    if manifest.fact_count != manifest.facts.len() {
        return Err(IdentityFactsRefusal::CountMismatch);
    }
    if challenge.questions.len() > MAX_IDENTITY_QUESTIONS
        || challenge
            .questions
            .iter()
            .map(|question| question.store_path.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != challenge.questions.len()
    {
        return Err(IdentityFactsRefusal::QuestionMismatch);
    }
    if !is_output_store_path(&manifest.observed_root) {
        return Err(IdentityFactsRefusal::IncompleteFact);
    }
    let mut seen = std::collections::BTreeSet::new();
    for fact in &manifest.facts {
        let deriver_ok = match &fact.deriver {
            Some(deriver) => is_store_path(deriver) && deriver.ends_with(".drv"),
            // A deriver-less path is explicit and still needs its outputs.
            None => true,
        };
        if !is_output_store_path(&fact.store_path)
            || !deriver_ok
            || !is_sha256(&fact.nar_hash)
            || fact.outputs.is_empty()
            || fact.outputs.len() > MAX_IDENTITY_OUTPUTS
            || fact
                .outputs
                .iter()
                .any(|output| !is_output_store_path(output))
            || !fact.outputs.contains(&fact.store_path)
            || fact.outputs.len()
                != fact
                    .outputs
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
        {
            return Err(IdentityFactsRefusal::IncompleteFact);
        }
        if fact.deriver.is_none() && fact.outputs != vec![fact.store_path.clone()] {
            return Err(IdentityFactsRefusal::IncompleteFact);
        }
        if !seen.insert(fact.store_path.as_str()) {
            return Err(IdentityFactsRefusal::DuplicatePath);
        }
    }
    if closure_manifest_sha256(&manifest.facts) != manifest.closure_manifest_sha256 {
        return Err(IdentityFactsRefusal::DigestMismatch);
    }
    match challenge.mode {
        IdentityFactsMode::Enumerate => {
            if !challenge.questions.is_empty() {
                return Err(IdentityFactsRefusal::QuestionMismatch);
            }
            // Discovery starts at the observed root, so the root must be one of
            // the facts. QUESTIONS answers a subset and is exempt.
            if !manifest
                .facts
                .iter()
                .any(|fact| fact.store_path == manifest.observed_root)
            {
                return Err(IdentityFactsRefusal::RootNotObserved);
            }
            Ok(())
        }
        IdentityFactsMode::Questions => {
            if challenge.questions.is_empty() {
                return Err(IdentityFactsRefusal::QuestionMismatch);
            }
            if challenge.questions.len() != manifest.facts.len() {
                return Err(IdentityFactsRefusal::QuestionMismatch);
            }
            // Questions are canonical: the answer must be in exactly the issued
            // order, with every field equal. Reordering is a refusal.
            for (question, fact) in challenge.questions.iter().zip(&manifest.facts) {
                if fact.store_path != question.store_path
                    || fact.deriver != question.deriver
                    || fact.nar_hash != question.nar_hash
                    || fact.outputs != question.outputs
                {
                    return Err(IdentityFactsRefusal::QuestionMismatch);
                }
            }
            Ok(())
        }
    }
}

/// Deterministic store path for feature-gated test builders.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub fn fixture_store_path(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    let hash: String = digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("/nix/store/{hash}-fixture-environment")
}

/// Deterministic receipt for feature-gated test builders. It is never
/// compiled into a production binary and is not accepted as real evidence.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub fn fixture_receipt(challenge: &IdentityFactsChallenge, seed: &str) -> IdentityFactsReceipt {
    let path = fixture_store_path(seed);
    let facts = vec![IdentityFact {
        store_path: path.clone(),
        deriver: None,
        nar_hash: "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
        outputs: vec![path.clone()],
    }];
    IdentityFactsReceipt {
        envelope: IdentityFactsEnvelope {
            schema_version: IDENTITY_FACTS_SCHEMA_VERSION,
            context: IDENTITY_FACTS_CONTEXT.into(),
            challenge: challenge.clone(),
            manifest: IdentityFactsManifest {
                observed_root: path,
                fact_count: facts.len(),
                closure_manifest_sha256: closure_manifest_sha256(&facts),
                truncated: false,
                facts,
            },
        },
        signature: "0".repeat(64),
    }
}

/// Feature-gated verified receipt for deterministic test builders.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub fn fixture_verified_receipt(
    challenge: &IdentityFactsChallenge,
    seed: &str,
) -> VerifiedIdentityFacts {
    VerifiedIdentityFacts(fixture_receipt(challenge, seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BEARER: &[u8] = b"scoped-environment-bearer-0123456789abcdef";

    fn fact(index: usize) -> IdentityFact {
        let hash = format!("{index:032x}");
        let path = format!("/nix/store/{hash}-pkg-{index}");
        IdentityFact {
            store_path: path.clone(),
            deriver: Some(format!("/nix/store/{hash}-pkg-{index}.drv")),
            nar_hash: "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            outputs: vec![path],
        }
    }

    fn make_manifest(facts: Vec<IdentityFact>) -> IdentityFactsManifest {
        let observed_root = facts
            .first()
            .map(|fact| fact.store_path.clone())
            .unwrap_or_else(|| format!("/nix/store/{}-profile", "a".repeat(32)));
        IdentityFactsManifest {
            observed_root,
            fact_count: facts.len(),
            closure_manifest_sha256: closure_manifest_sha256(&facts),
            truncated: false,
            facts,
        }
    }

    fn challenge(
        mode: IdentityFactsMode,
        questions: Vec<IdentityQuestion>,
    ) -> IdentityFactsChallenge {
        IdentityFactsChallenge {
            operation_id: "op-1".into(),
            runtime_id: "runtime-1".into(),
            nonce: "nonce-1".into(),
            pin: "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d".into(),
            plan_sha256: "a".repeat(64),
            spec_sha256: "c".repeat(64),
            mode,
            root: OperationRoot::Operation,
            questions,
        }
    }

    #[test]
    fn deriver_less_paths_are_explicit_and_accepted() {
        let mut facts: Vec<IdentityFact> = (0..73).map(fact).collect();
        for item in facts.iter_mut().take(14) {
            item.deriver = None;
        }
        let manifest = make_manifest(facts);
        validate_manifest(&challenge(IdentityFactsMode::Enumerate, vec![]), &manifest)
            .expect("deriver-less paths are explicit and accepted");
    }

    #[test]
    fn questions_compare_every_field_not_only_the_path() {
        let facts = vec![fact(0)];
        let manifest = make_manifest(facts.clone());
        let mut question = IdentityQuestion {
            store_path: facts[0].store_path.clone(),
            deriver: facts[0].deriver.clone(),
            nar_hash: facts[0].nar_hash.clone(),
            outputs: facts[0].outputs.clone(),
        };
        question.nar_hash = "sha256-other".into();
        assert_eq!(
            validate_manifest(
                &challenge(IdentityFactsMode::Questions, vec![question]),
                &manifest
            ),
            Err(IdentityFactsRefusal::QuestionMismatch)
        );
    }

    #[test]
    fn a_closure_larger_than_run10_is_accepted() {
        let facts: Vec<IdentityFact> = (0..73).map(fact).collect();
        let manifest = make_manifest(facts);
        validate_manifest(&challenge(IdentityFactsMode::Enumerate, vec![]), &manifest)
            .expect("73 facts are accepted");
    }

    #[test]
    fn empty_truncated_or_overflowing_manifests_are_refused() {
        let empty = make_manifest(vec![]);
        assert_eq!(
            validate_manifest(&challenge(IdentityFactsMode::Enumerate, vec![]), &empty),
            Err(IdentityFactsRefusal::EmptyManifest)
        );
        let mut truncated = make_manifest(vec![fact(0)]);
        truncated.truncated = true;
        assert_eq!(
            validate_manifest(&challenge(IdentityFactsMode::Enumerate, vec![]), &truncated),
            Err(IdentityFactsRefusal::Truncated)
        );
        let oversized: Vec<IdentityFact> = (0..=MAX_IDENTITY_FACTS).map(fact).collect();
        let mut too_many = make_manifest(oversized);
        too_many.fact_count = too_many.facts.len();
        assert_eq!(
            validate_manifest(&challenge(IdentityFactsMode::Enumerate, vec![]), &too_many),
            Err(IdentityFactsRefusal::TooManyFacts)
        );
    }

    #[test]
    fn duplicate_or_incomplete_facts_and_digest_mismatch_are_refused() {
        let duplicate = make_manifest(vec![fact(0), fact(0)]);
        assert_eq!(
            validate_manifest(&challenge(IdentityFactsMode::Enumerate, vec![]), &duplicate),
            Err(IdentityFactsRefusal::DuplicatePath)
        );
        let mut incomplete = make_manifest(vec![fact(0)]);
        incomplete.facts[0].outputs.clear();
        incomplete.closure_manifest_sha256 = closure_manifest_sha256(&incomplete.facts);
        assert_eq!(
            validate_manifest(
                &challenge(IdentityFactsMode::Enumerate, vec![]),
                &incomplete
            ),
            Err(IdentityFactsRefusal::IncompleteFact)
        );
        let mut tampered = make_manifest(vec![fact(0)]);
        tampered.closure_manifest_sha256 = "0".repeat(64);
        assert_eq!(
            validate_manifest(&challenge(IdentityFactsMode::Enumerate, vec![]), &tampered),
            Err(IdentityFactsRefusal::DigestMismatch)
        );
    }

    fn signed_receipt(
        challenge: &IdentityFactsChallenge,
        facts: Vec<IdentityFact>,
        bearer: &[u8],
    ) -> IdentityFactsReceipt {
        let manifest = make_manifest(facts);
        let envelope = IdentityFactsEnvelope {
            schema_version: IDENTITY_FACTS_SCHEMA_VERSION,
            context: IDENTITY_FACTS_CONTEXT.into(),
            challenge: challenge.clone(),
            manifest,
        };
        let signature = sign(&envelope, bearer).expect("the fixture envelope signs");
        IdentityFactsReceipt {
            envelope,
            signature,
        }
    }

    #[test]
    fn the_broker_fixture_manifest_is_structurally_valid() {
        let root = format!("/nix/store/{}-fixture-root", "c".repeat(32));
        let drv = format!("/nix/store/{}-fixture.drv", "d".repeat(32));
        let helper = format!("/nix/store/{}-helper.sh", "e".repeat(32));
        let facts = vec![
            IdentityFact {
                store_path: root.clone(),
                deriver: Some(drv.clone()),
                nar_hash: "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
                outputs: vec![root.clone()],
            },
            IdentityFact {
                store_path: helper.clone(),
                deriver: None,
                nar_hash: "sha256-AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=".into(),
                outputs: vec![helper.clone()],
            },
        ];
        let manifest = make_manifest(facts);
        validate_manifest(&challenge(IdentityFactsMode::Enumerate, vec![]), &manifest)
            .expect("the broker fixture manifest must validate");
    }

    #[test]
    fn a_broker_receipt_verifies_for_its_exact_challenge() {
        let challenge = challenge(IdentityFactsMode::Enumerate, vec![]);
        let receipt = signed_receipt(&challenge, vec![fact(0)], BEARER);
        verify(&receipt, BEARER, &challenge).expect("the exact challenge verifies");
    }

    #[test]
    fn tampered_manifest_context_or_signature_is_refused() {
        let challenge = challenge(IdentityFactsMode::Enumerate, vec![]);
        let receipt = signed_receipt(&challenge, vec![fact(0)], BEARER);

        let mut tampered = receipt.clone();
        tampered.envelope.manifest.facts[0]
            .outputs
            .push(format!("/nix/store/{}-other-output", "b".repeat(32)));
        tampered.envelope.manifest.closure_manifest_sha256 =
            closure_manifest_sha256(&tampered.envelope.manifest.facts);
        assert_eq!(
            verify(&tampered, BEARER, &challenge),
            Err(IdentityFactsError::SignatureMismatch)
        );

        let mut wrong_context = receipt.clone();
        wrong_context.envelope.context = "factory.environment.helper-facts.v1".into();
        assert_eq!(
            verify(&wrong_context, BEARER, &challenge),
            Err(IdentityFactsError::ContextMismatch)
        );

        assert_eq!(
            verify(&receipt, b"another-bearer-0123456789abcdefghij", &challenge),
            Err(IdentityFactsError::SignatureMismatch)
        );
    }

    #[test]
    fn another_operation_nonce_plan_runtime_or_pin_is_refused() {
        let challenge = challenge(IdentityFactsMode::Enumerate, vec![]);
        let receipt = signed_receipt(&challenge, vec![fact(0)], BEARER);
        let mut other = challenge.clone();
        other.nonce = "nonce-2".into();
        assert_eq!(
            verify(&receipt, BEARER, &other),
            Err(IdentityFactsError::BindingMismatch)
        );
        let mut other = challenge.clone();
        other.operation_id = "op-2".into();
        assert_eq!(
            verify(&receipt, BEARER, &other),
            Err(IdentityFactsError::BindingMismatch)
        );
        let mut other = challenge.clone();
        other.plan_sha256 = "b".repeat(64);
        assert_eq!(
            verify(&receipt, BEARER, &other),
            Err(IdentityFactsError::BindingMismatch)
        );
        let mut other = challenge.clone();
        other.runtime_id = "runtime-2".into();
        assert_eq!(
            verify(&receipt, BEARER, &other),
            Err(IdentityFactsError::BindingMismatch)
        );
        let mut other = challenge.clone();
        other.pin = "github:NixOS/nixpkgs/other".into();
        assert_eq!(
            verify(&receipt, BEARER, &other),
            Err(IdentityFactsError::BindingMismatch)
        );
    }

    #[test]
    fn a_malformed_challenge_is_refused_before_dispatch() {
        let mut empty_questions = challenge(IdentityFactsMode::Questions, vec![]);
        assert_eq!(
            empty_questions.validate_shape(),
            Err(IdentityFactsError::InvalidChallenge)
        );
        empty_questions.mode = IdentityFactsMode::Enumerate;
        empty_questions
            .validate_shape()
            .expect("enumerate is discovery");
        let mut discovery_with_questions =
            challenge(IdentityFactsMode::Enumerate, vec![question(0)]);
        assert_eq!(
            discovery_with_questions.validate_shape(),
            Err(IdentityFactsError::InvalidChallenge)
        );
        discovery_with_questions.mode = IdentityFactsMode::Questions;
        discovery_with_questions
            .validate_shape()
            .expect("questions mode accepts exact questions");
        let mut bad_plan = challenge(IdentityFactsMode::Enumerate, vec![]);
        bad_plan.plan_sha256 = "not-a-hash".into();
        assert_eq!(
            bad_plan.validate_shape(),
            Err(IdentityFactsError::InvalidChallenge)
        );
        let mut deriverless_extra = question(0);
        deriverless_extra.deriver = None;
        deriverless_extra
            .outputs
            .push(format!("/nix/store/{}-extra", "f".repeat(32)));
        assert_eq!(
            challenge(IdentityFactsMode::Questions, vec![deriverless_extra]).validate_shape(),
            Err(IdentityFactsError::InvalidChallenge)
        );
        let mut missing_self = question(0);
        missing_self.outputs = vec![format!("/nix/store/{}-other", "f".repeat(32))];
        assert_eq!(
            challenge(IdentityFactsMode::Questions, vec![missing_self]).validate_shape(),
            Err(IdentityFactsError::InvalidChallenge)
        );
    }

    fn question(index: usize) -> IdentityQuestion {
        let item = fact(index);
        IdentityQuestion {
            store_path: item.store_path,
            deriver: item.deriver,
            nar_hash: item.nar_hash,
            outputs: item.outputs,
        }
    }

    #[test]
    fn inconsistent_observed_facts_and_roots_are_refused() {
        let challenge = challenge(IdentityFactsMode::Enumerate, vec![]);
        let mut missing_self = make_manifest(vec![fact(0)]);
        missing_self.facts[0].outputs.clear();
        missing_self.facts[0]
            .outputs
            .push(format!("/nix/store/{}-other", "f".repeat(32)));
        missing_self.closure_manifest_sha256 = closure_manifest_sha256(&missing_self.facts);
        assert_eq!(
            validate_manifest(&challenge, &missing_self),
            Err(IdentityFactsRefusal::IncompleteFact)
        );
        let mut deriverless_extra = make_manifest(vec![fact(0)]);
        deriverless_extra.facts[0].deriver = None;
        deriverless_extra.facts[0]
            .outputs
            .push(format!("/nix/store/{}-extra", "f".repeat(32)));
        deriverless_extra.closure_manifest_sha256 =
            closure_manifest_sha256(&deriverless_extra.facts);
        assert_eq!(
            validate_manifest(&challenge, &deriverless_extra),
            Err(IdentityFactsRefusal::IncompleteFact)
        );
        let mut derivation_path = make_manifest(vec![fact(0)]);
        derivation_path.facts[0].store_path = format!("/nix/store/{}-pkg.drv", "f".repeat(32));
        derivation_path.closure_manifest_sha256 = closure_manifest_sha256(&derivation_path.facts);
        assert_eq!(
            validate_manifest(&challenge, &derivation_path),
            Err(IdentityFactsRefusal::IncompleteFact)
        );
        let mut orphan_root = make_manifest(vec![fact(0)]);
        orphan_root.observed_root = format!("/nix/store/{}-unobserved", "f".repeat(32));
        assert_eq!(
            validate_manifest(&challenge, &orphan_root),
            Err(IdentityFactsRefusal::RootNotObserved)
        );
    }

    #[test]
    fn questions_mode_requires_exactly_the_issued_paths() {
        let facts = vec![fact(0), fact(1)];
        let manifest = make_manifest(facts.clone());
        let questions = facts
            .iter()
            .map(|item| IdentityQuestion {
                store_path: item.store_path.clone(),
                deriver: item.deriver.clone(),
                nar_hash: item.nar_hash.clone(),
                outputs: item.outputs.clone(),
            })
            .collect();
        validate_manifest(
            &challenge(IdentityFactsMode::Questions, questions),
            &manifest,
        )
        .expect("exact answers are accepted");
        let partial = make_manifest(vec![fact(0)]);
        let wanted = vec![
            IdentityQuestion {
                store_path: fact(0).store_path,
                deriver: fact(0).deriver,
                nar_hash: fact(0).nar_hash,
                outputs: fact(0).outputs,
            },
            IdentityQuestion {
                store_path: fact(1).store_path,
                deriver: fact(1).deriver,
                nar_hash: fact(1).nar_hash,
                outputs: fact(1).outputs,
            },
        ];
        assert_eq!(
            validate_manifest(
                &challenge(IdentityFactsMode::Questions, wanted.clone()),
                &partial
            ),
            Err(IdentityFactsRefusal::QuestionMismatch)
        );
        // The answer order is canonical: reordering the issued questions is
        // refused even when every expected identity is present.
        let mut reordered = wanted.clone();
        reordered.reverse();
        assert_eq!(
            validate_manifest(
                &challenge(IdentityFactsMode::Questions, reordered),
                &manifest
            ),
            Err(IdentityFactsRefusal::QuestionMismatch)
        );
    }
}
