//! Identity-facts manifest for the environment broker (ADR-0045).
//!
//! This module defines the *complete* facts manifest the broker must sign and
//! Core must verify: every fact carries the exact store path, derivation, NAR
//! hash and output paths. The manifest is never paginated; a closure larger than
//! the documented technical bound is refused instead of truncated.
//!
//! It is pure and unactivated: no producer or consumer calls it yet.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::lead_classification::is_sha256;

/// Separate HMAC context; identity facts are not helper facts.
pub const IDENTITY_FACTS_CONTEXT: &str = "factory.environment.identity-facts.v1";
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
    pub mode: IdentityFactsMode,
    pub root: OperationRoot,
    pub questions: Vec<IdentityQuestion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityFactsRefusal {
    EmptyManifest,
    TooManyFacts,
    Truncated,
    CountMismatch,
    DuplicatePath,
    IncompleteFact,
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
            Self::DigestMismatch => "the closure manifest digest does not match",
            Self::QuestionMismatch => "the manifest does not answer the issued questions",
        })
    }
}

impl std::error::Error for IdentityFactsRefusal {}

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
    if challenge.questions.len() > 64
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
    if !is_store_path(&manifest.observed_root) {
        return Err(IdentityFactsRefusal::IncompleteFact);
    }
    let mut seen = std::collections::BTreeSet::new();
    for fact in &manifest.facts {
        let deriver_ok = match &fact.deriver {
            Some(deriver) => deriver.ends_with(".drv"),
            // A deriver-less path is explicit and still needs its outputs.
            None => true,
        };
        if !is_store_path(&fact.store_path)
            || !deriver_ok
            || !is_sha256(&fact.nar_hash)
            || fact.outputs.is_empty()
            || fact.outputs.iter().any(|output| !is_store_path(output))
            || fact.outputs.len()
                != fact
                    .outputs
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
        {
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
            Ok(())
        }
        IdentityFactsMode::Questions => {
            if challenge.questions.is_empty() {
                return Err(IdentityFactsRefusal::QuestionMismatch);
            }
            if challenge.questions.len() != manifest.facts.len() {
                return Err(IdentityFactsRefusal::QuestionMismatch);
            }
            for question in &challenge.questions {
                let Some(fact) = manifest
                    .facts
                    .iter()
                    .find(|fact| fact.store_path == question.store_path)
                else {
                    return Err(IdentityFactsRefusal::QuestionMismatch);
                };
                // Compare the complete expectation, not only the path.
                if fact.deriver != question.deriver
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

#[cfg(test)]
mod tests {
    use super::*;

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
        IdentityFactsManifest {
            observed_root: format!("/nix/store/{}-profile", "a".repeat(32)),
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
            validate_manifest(&challenge(IdentityFactsMode::Questions, wanted), &partial),
            Err(IdentityFactsRefusal::QuestionMismatch)
        );
    }
}
