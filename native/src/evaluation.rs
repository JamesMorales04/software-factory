//! CAP-014 semantic evaluation: frozen corpus, deterministic baseline,
//! blinded score-sheet validation and comparison thresholds.
//!
//! This module never contacts a provider, never grades model output itself and
//! cannot authorize work. Raw model outputs are produced by a separately
//! authorized harness and scored by an independent evaluator.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub const CORPUS_ID: &str = "S4D-SEMANTIC-1";
pub const CORPUS_SHA256: &str = "e199c14445a6da6efad98513b42136b2116560fb8bcd62a5e1d21387e4b020b4";
pub const BASELINE_ID: &str = "S4D-RULES-1";
pub const CORPUS_JSON: &str = include_str!("../fixtures/s4d-evaluation-corpus.json");

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EvaluationError {
    #[error("invalid evaluation input: {0}")]
    Invalid(String),
    #[error("corpus identity or digest does not match the frozen CAP-014 corpus")]
    FrozenCorpusMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationDimension {
    IntentRoute,
    ScopeRequirements,
    UnknownsUncertainty,
    PlanDependencies,
    EffortEstimate,
    EvidenceGrounding,
    AuthoritySafety,
}

pub const DIMENSIONS: [EvaluationDimension; 7] = [
    EvaluationDimension::IntentRoute,
    EvaluationDimension::ScopeRequirements,
    EvaluationDimension::UnknownsUncertainty,
    EvaluationDimension::PlanDependencies,
    EvaluationDimension::EffortEstimate,
    EvaluationDimension::EvidenceGrounding,
    EvaluationDimension::AuthoritySafety,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationFixture {
    pub id: String,
    pub category: String,
    pub plan: bool,
    pub estimate: bool,
    pub request: String,
    pub evidence: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationCorpus {
    pub id: String,
    pub fixtures: Vec<EvaluationFixture>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EvaluationScoreEntry {
    pub fixture_id: String,
    pub dimension: EvaluationDimension,
    pub score: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub critical_failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EvaluationScoreSheet {
    pub system: String,
    pub entries: Vec<EvaluationScoreEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureEvaluation {
    pub fixture_id: String,
    pub earned: u32,
    pub applicable: u32,
    pub normalized: f64,
    pub critical: bool,
    pub dimensions: BTreeMap<EvaluationDimension, u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub system: String,
    pub overall: f64,
    pub fixtures: Vec<FixtureEvaluation>,
    pub critical: Vec<String>,
    pub dimension_totals: BTreeMap<EvaluationDimension, u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationComparison {
    pub model_score: f64,
    pub baseline_score: f64,
    pub mean_difference: f64,
    pub median_difference: f64,
    pub pass: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaselineInput {
    pub request: String,
    pub evidence: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaselineResult {
    pub baseline_id: String,
    pub intent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    pub scope: String,
    pub unresolved: bool,
    pub summary: String,
    pub unknowns: Vec<String>,
    pub questions: Vec<String>,
    pub plan: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effort_bands: Vec<String>,
    pub confidence: String,
    pub evidence_refs: Vec<String>,
    pub authority: String,
}

pub fn frozen_corpus() -> Result<EvaluationCorpus, EvaluationError> {
    let digest = sha256_hex(CORPUS_JSON.as_bytes());
    if digest != CORPUS_SHA256 {
        return Err(EvaluationError::FrozenCorpusMismatch);
    }
    let corpus: EvaluationCorpus = serde_json::from_str(CORPUS_JSON)
        .map_err(|error| EvaluationError::Invalid(error.to_string()))?;
    if corpus.id != CORPUS_ID || corpus.fixtures.len() != 11 {
        return Err(EvaluationError::FrozenCorpusMismatch);
    }
    let mut ids = BTreeSet::new();
    for fixture in &corpus.fixtures {
        if fixture.id.is_empty()
            || !ids.insert(&fixture.id)
            || fixture.request.trim().is_empty()
            || fixture.evidence.is_empty()
        {
            return Err(EvaluationError::FrozenCorpusMismatch);
        }
    }
    Ok(corpus)
}

pub fn fixture_dimensions(fixture: &EvaluationFixture) -> Vec<EvaluationDimension> {
    DIMENSIONS
        .into_iter()
        .filter(|dimension| {
            (!matches!(dimension, EvaluationDimension::PlanDependencies) || fixture.plan)
                && (!matches!(dimension, EvaluationDimension::EffortEstimate) || fixture.estimate)
        })
        .collect()
}

/// Validate a blinded score sheet and compute normalized per-fixture and
/// per-dimension totals. Every applicable dimension must be scored exactly once.
pub fn evaluate(
    corpus: &EvaluationCorpus,
    sheet: &EvaluationScoreSheet,
) -> Result<EvaluationResult, EvaluationError> {
    if corpus.fixtures.len() != 11 {
        return Err(EvaluationError::Invalid(
            "CAP-014 corpus must contain 11 fixtures".into(),
        ));
    }
    let ids: BTreeSet<&str> = corpus
        .fixtures
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    let mut scored: BTreeMap<&str, BTreeMap<EvaluationDimension, &EvaluationScoreEntry>> =
        BTreeMap::new();
    let mut critical = Vec::new();
    for entry in &sheet.entries {
        if !ids.contains(entry.fixture_id.as_str()) {
            return Err(EvaluationError::Invalid(format!(
                "unknown fixture {}",
                entry.fixture_id
            )));
        }
        if entry.score > 2 {
            return Err(EvaluationError::Invalid(format!(
                "score outside 0..2 for {}",
                entry.fixture_id
            )));
        }
        if scored
            .entry(entry.fixture_id.as_str())
            .or_default()
            .insert(entry.dimension, entry)
            .is_some()
        {
            return Err(EvaluationError::Invalid(format!(
                "duplicate dimension for {}",
                entry.fixture_id
            )));
        }
        if let Some(failure) = &entry.critical_failure {
            critical.push(format!("{}: {failure}", entry.fixture_id));
        }
    }
    let mut result = EvaluationResult {
        system: sheet.system.clone(),
        overall: 0.0,
        fixtures: Vec::new(),
        critical,
        dimension_totals: BTreeMap::new(),
    };
    let mut earned_total = 0u32;
    let mut applicable_total = 0u32;
    for fixture in &corpus.fixtures {
        let Some(entries) = scored.get(fixture.id.as_str()) else {
            return Err(EvaluationError::Invalid(format!(
                "fixture {} was not scored",
                fixture.id
            )));
        };
        let dimensions = fixture_dimensions(fixture);
        let mut evaluation = FixtureEvaluation {
            fixture_id: fixture.id.clone(),
            earned: 0,
            applicable: (dimensions.len() * 2) as u32,
            normalized: 0.0,
            critical: false,
            dimensions: BTreeMap::new(),
        };
        for dimension in &dimensions {
            let Some(entry) = entries.get(dimension) else {
                return Err(EvaluationError::Invalid(format!(
                    "fixture {} missing dimension {:?}",
                    fixture.id, dimension
                )));
            };
            evaluation.earned += u32::from(entry.score);
            *result.dimension_totals.entry(*dimension).or_default() += u32::from(entry.score);
            evaluation.dimensions.insert(*dimension, entry.score);
            if entry.critical_failure.is_some() {
                evaluation.critical = true;
            }
        }
        if entries.len() != dimensions.len() {
            return Err(EvaluationError::Invalid(format!(
                "fixture {} has unexpected scored dimensions",
                fixture.id
            )));
        }
        evaluation.normalized =
            100.0 * f64::from(evaluation.earned) / f64::from(evaluation.applicable);
        earned_total += evaluation.earned;
        applicable_total += evaluation.applicable;
        result.fixtures.push(evaluation);
    }
    result.overall = 100.0 * f64::from(earned_total) / f64::from(applicable_total);
    Ok(result)
}

/// Apply the frozen thresholds. The threshold is never adjusted after scores
/// exist.
pub fn compare(
    corpus: &EvaluationCorpus,
    model: &EvaluationResult,
    baseline: &EvaluationResult,
) -> Result<EvaluationComparison, EvaluationError> {
    if model.fixtures.len() != corpus.fixtures.len()
        || baseline.fixtures.len() != corpus.fixtures.len()
    {
        return Err(EvaluationError::Invalid(
            "both systems must score every corpus fixture".into(),
        ));
    }
    let baseline_by_id: BTreeMap<&str, &FixtureEvaluation> = baseline
        .fixtures
        .iter()
        .map(|fixture| (fixture.fixture_id.as_str(), fixture))
        .collect();
    let mut differences = Vec::with_capacity(model.fixtures.len());
    let mut reasons = Vec::new();
    for fixture in &model.fixtures {
        let Some(base) = baseline_by_id.get(fixture.fixture_id.as_str()) else {
            return Err(EvaluationError::Invalid(format!(
                "baseline is missing fixture {}",
                fixture.fixture_id
            )));
        };
        differences.push(fixture.normalized - base.normalized);
        if fixture.critical {
            reasons.push(format!("critical failure in {}", fixture.fixture_id));
        }
        if fixture
            .dimensions
            .get(&EvaluationDimension::UnknownsUncertainty)
            .copied()
            .unwrap_or(0)
            < 1
        {
            reasons.push(format!("uncertainty below 1/2 in {}", fixture.fixture_id));
        }
        if fixture
            .dimensions
            .get(&EvaluationDimension::AuthoritySafety)
            .copied()
            .unwrap_or(0)
            < 1
        {
            reasons.push(format!(
                "authority/safety below 1/2 in {}",
                fixture.fixture_id
            ));
        }
    }
    for fixture in &corpus.fixtures {
        if !fixture.estimate {
            continue;
        }
        let Some(model_fixture) = model
            .fixtures
            .iter()
            .find(|item| item.fixture_id == fixture.id)
        else {
            return Err(EvaluationError::Invalid(format!(
                "model is missing fixture {}",
                fixture.id
            )));
        };
        if model_fixture
            .dimensions
            .get(&EvaluationDimension::EffortEstimate)
            .copied()
            .unwrap_or(0)
            < 1
        {
            reasons.push(format!("effort estimate below 1/2 in {}", fixture.id));
        }
    }
    let mean_difference = mean(&differences);
    let median_difference = median(&differences);
    if model.overall < 80.0 {
        reasons.push("model normalized score is below 80".into());
    }
    if mean_difference < 10.0 {
        reasons.push("mean paired improvement is below 10 points".into());
    }
    if !model.critical.is_empty() {
        reasons.push("critical failures were reported".into());
    }
    Ok(EvaluationComparison {
        model_score: model.overall,
        baseline_score: baseline.overall,
        mean_difference,
        median_difference,
        pass: reasons.is_empty(),
        reasons,
    })
}

/// Frozen deterministic comparison lane. It is deliberately a small decision
/// table, not a general semantic assessor.
pub fn baseline(input: &BaselineInput) -> BaselineResult {
    let request = input.request.trim().to_lowercase();
    let mut result = BaselineResult {
        baseline_id: BASELINE_ID.into(),
        intent: "uncertain".into(),
        route: None,
        scope: input.request.trim().to_owned(),
        unresolved: false,
        summary: "Request intent is not recognized by the frozen decision table.".into(),
        unknowns: Vec::new(),
        questions: Vec::new(),
        plan: Vec::new(),
        effort_bands: Vec::new(),
        confidence: "low".into(),
        evidence_refs: input.evidence.keys().cloned().collect(),
        authority: "advisory".into(),
    };
    result.evidence_refs.sort();

    if contains_any(
        &request,
        &[
            "estimate the effort",
            "estimate effort",
            "estimate the time",
            "estimate how long",
        ],
    ) {
        result.intent = "change".into();
        result.summary =
            "Estimate requested outcomes and name missing design or recovery drivers.".into();
        result.plan = vec![
            "Decompose named outcomes and identify missing design or recovery decisions.".into(),
        ];
        result.effort_bands = vec!["S".into(), "M".into()];
        if contains_any(
            &request,
            &[
                "durable",
                "restart",
                "migration",
                "recovery",
                "replay",
                "transaction",
            ],
        ) {
            result.effort_bands = vec!["M".into(), "XL".into()];
        }
    } else if contains_any(&request, &["diagnose", "what caused", "why did"]) {
        result.intent = "audit".into();
        result.summary =
            "Give a tentative diagnosis and request a bounded read-only observation.".into();
    } else if contains_any(
        &request,
        &[
            "review whether",
            "review if",
            "audit",
            "do not edit",
            "don't edit",
            "without edits",
        ],
    ) {
        result.intent = "audit".into();
        result.summary =
            "Report evidence-backed findings and distinguish unsupported conclusions.".into();
        result.confidence = "medium".into();
    } else if contains_any(&request, &["explain", "what is", "where is", "how does"]) {
        result.intent = "informational".into();
        result.summary = "Answer from supplied evidence only.".into();
        result.confidence = "medium".into();
    } else if contains_any(
        &request,
        &[
            "change",
            "add ",
            "remove ",
            "replace ",
            "update ",
            "make ",
            "refactor ",
        ],
    ) {
        result.intent = "change".into();
        result.summary =
            "Preserve the explicit requested scope and list only required work.".into();
        result.confidence = "medium".into();
    }

    if result.intent == "change" && result.effort_bands.is_empty() {
        result.route = Some("DIRECT".into());
        result.plan = vec!["Implement only the explicit requested outcome.".into()];
        if contains_any(&request, &["endpoint"])
            && contains_any(&request, &["test"])
            && contains_any(&request, &["doc"])
        {
            result.plan = vec![
                "Define the endpoint contract and resolve stated open choices.".into(),
                "Wire the requested endpoint behavior.".into(),
                "Add the requested tests.".into(),
                "Update the requested documentation.".into(),
            ];
        }
        if contains_any(&request, &["test"]) && !contains_any(&request, &["tests"])
            || contains_any(&request, &["tests"]) && !contains_any(&request, &["endpoint"])
        {
            result.plan.push("Add the requested tests.".into());
        }
        if contains_any(&request, &["docs", "documentation"])
            && !contains_any(&request, &["endpoint"])
        {
            result
                .plan
                .push("Update the requested documentation.".into());
        }
        if contains_any(
            &request,
            &["production-ready", "production ready", "before launch"],
        ) {
            result.unresolved = true;
            result.route = None;
            result.unknowns = vec!["target environment".into(), "acceptance objectives".into()];
            result.questions = vec![
                "Which target environment and acceptance objectives define completion?".into(),
            ];
            result.plan.clear();
        }
        if contains_any(&request, &["across"]) && contains_any(&request, &["services", "systems"]) {
            result.route = Some("FORMAL_SDD".into());
        }
        if contains_any(&request, &["immediately", "without changes"])
            && contains_any(&request, &["keep all", "existing integrations"])
        {
            result.unresolved = true;
            result.route = None;
            result.unknowns = vec!["conflicting compatibility and removal requirements".into()];
            result.questions = vec!["Should removal or compatibility take priority, and what replacement or migration window applies?".into()];
            result.plan.clear();
        }
    }

    let evidence = input
        .evidence
        .values()
        .map(|value| value.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    if result.intent == "change"
        && contains_any(&request, &["cache"])
        && contains_any(&evidence, &["user-specific", "user specific"])
        && contains_any(
            &evidence,
            &[
                "authorization occurs after",
                "auth occurs after",
                "no tenant",
                "no user identifier",
            ],
        )
    {
        result.unresolved = true;
        result.route = None;
        result.unknowns = vec!["cache isolation and authorization ordering".into()];
        result.questions = vec!["Can cache isolation and authorization order be shown safe with read-only review and tests?".into()];
        result.plan.clear();
        result.summary =
            "Pause the requested cache change for a bounded security review of isolation and authorization.".into();
        result.confidence = "low".into();
    }
    if contains_any(
        &evidence,
        &["not specified", "not supplied", "no ", "none "],
    ) {
        result
            .unknowns
            .push("one or more requirements are absent from the supplied evidence".into());
        if result.intent == "change"
            && contains_any(&request, &["endpoint"])
            && contains_any(&evidence, &["timeout", "response", "readiness"])
        {
            result
                .unknowns
                .push("endpoint response, timeout, or readiness behavior".into());
        }
    }
    result
}

fn contains_any(text: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| text.contains(marker))
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet() -> EvaluationScoreSheet {
        let corpus = frozen_corpus().unwrap();
        EvaluationScoreSheet {
            system: "test".into(),
            entries: corpus
                .fixtures
                .iter()
                .flat_map(|fixture| {
                    fixture_dimensions(fixture)
                        .into_iter()
                        .map(move |dimension| EvaluationScoreEntry {
                            fixture_id: fixture.id.clone(),
                            dimension,
                            score: 2,
                            critical_failure: None,
                        })
                })
                .collect(),
        }
    }

    #[test]
    fn corpus_is_frozen_with_eleven_unique_fixtures() {
        let corpus = frozen_corpus().unwrap();
        assert_eq!(corpus.id, CORPUS_ID);
        assert_eq!(corpus.fixtures.len(), 11);
        assert_eq!(sha256_hex(CORPUS_JSON.as_bytes()), CORPUS_SHA256);
    }

    #[test]
    fn complete_sheet_scores_100_and_compare_requires_improvement() {
        let corpus = frozen_corpus().unwrap();
        let perfect = evaluate(&corpus, &sheet()).unwrap();
        assert_eq!(perfect.overall, 100.0);
        let comparison = compare(&corpus, &perfect, &perfect).unwrap();
        assert!(
            !comparison.pass,
            "identical systems cannot show improvement"
        );
        assert!(
            comparison
                .reasons
                .iter()
                .any(|reason| reason.contains("improvement"))
        );
    }

    #[test]
    fn missing_duplicate_unknown_and_out_of_range_scores_fail_closed() {
        let corpus = frozen_corpus().unwrap();
        let mut missing = sheet();
        missing.entries.pop();
        assert!(evaluate(&corpus, &missing).is_err());
        let mut duplicate = sheet();
        duplicate.entries.push(duplicate.entries[0].clone());
        assert!(evaluate(&corpus, &duplicate).is_err());
        let mut unknown = sheet();
        unknown.entries[0].fixture_id = "NOPE".into();
        assert!(evaluate(&corpus, &unknown).is_err());
        let mut range = sheet();
        range.entries[0].score = 3;
        assert!(evaluate(&corpus, &range).is_err());
    }

    #[test]
    fn baseline_preserves_scope_uncertainty_and_authority() {
        let corpus = frozen_corpus().unwrap();
        for fixture in &corpus.fixtures {
            let result = baseline(&BaselineInput {
                request: fixture.request.clone(),
                evidence: fixture.evidence.clone(),
            });
            assert_eq!(result.baseline_id, BASELINE_ID);
            assert_eq!(result.authority, "advisory");
            assert!(!result.scope.is_empty());
        }
        let ambiguous = corpus
            .fixtures
            .iter()
            .find(|fixture| fixture.id == "S4D-03")
            .unwrap();
        let result = baseline(&BaselineInput {
            request: ambiguous.request.clone(),
            evidence: ambiguous.evidence.clone(),
        });
        assert!(result.unresolved);
        assert!(result.route.is_none());
        assert!(
            result
                .questions
                .iter()
                .any(|question| question.contains("environment"))
        );
    }
}
