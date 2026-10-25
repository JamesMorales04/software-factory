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

/// A bounded, Core-authorized CAP-014 model evaluation run. Raw model outputs
/// are evidence for an independent scorer; they are never an acceptance claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelEvaluationFixture {
    pub fixture_id: String,
    pub status: String,
    pub reservation_id: String,
    pub provider: String,
    pub model: String,
    pub content: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelEvaluationEvidence {
    pub schema: String,
    pub corpus_id: String,
    pub corpus_sha256: String,
    pub evaluation_id: String,
    pub recorded_at: String,
    pub status: String,
    pub provider: String,
    pub model: String,
    pub fixtures: Vec<ModelEvaluationFixture>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct ModelEvaluationConfig {
    pub core_url: String,
    pub operator_token: String,
    pub evaluation_id: String,
    pub timeout: std::time::Duration,
}

pub async fn execute_model_evaluation(
    corpus: &EvaluationCorpus,
    config: &ModelEvaluationConfig,
) -> Result<ModelEvaluationEvidence, EvaluationError> {
    let url = reqwest::Url::parse(&config.core_url)
        .map_err(|_| EvaluationError::Invalid("core URL is invalid".into()))?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"))
        || url.port().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(EvaluationError::Invalid(
            "core URL must be a loopback HTTP origin with an explicit port".into(),
        ));
    }
    if config.operator_token.len() < 16 || config.evaluation_id.trim().is_empty() {
        return Err(EvaluationError::Invalid(
            "operator token or evaluation identity is invalid".into(),
        ));
    }
    let client = reqwest::Client::builder()
        .timeout(config.timeout)
        .no_proxy()
        .build()
        .map_err(|_| EvaluationError::Invalid("evaluation client".into()))?;
    let mut evidence = ModelEvaluationEvidence {
        schema: "factory.cap014-model-evaluation.v1".into(),
        corpus_id: corpus.id.clone(),
        corpus_sha256: CORPUS_SHA256.into(),
        evaluation_id: config.evaluation_id.clone(),
        recorded_at: chrono::Utc::now().to_rfc3339(),
        status: "COMPLETE".into(),
        provider: String::new(),
        model: String::new(),
        fixtures: Vec::new(),
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        cost_micros: 0,
        error: String::new(),
    };
    for fixture in &corpus.fixtures {
        let (system_prompt, user_prompt) = evaluation_prompt(fixture);
        let response = client
            .post(
                url.join("model/calls")
                    .map_err(|_| EvaluationError::Invalid("evaluation core route".into()))?,
            )
            .bearer_auth(&config.operator_token)
            .json(&serde_json::json!({
                "role": "evaluation",
                "principalId": "s4d-evaluator",
                "projectId": "evaluation",
                "objectiveId": config.evaluation_id,
                "scopeId": config.evaluation_id,
                "purpose": "cap014_evaluation",
                "systemPrompt": system_prompt,
                "userPrompt": user_prompt,
                "maxOutputTokens": 1024
            }))
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                evidence.status = "FAILED".into();
                evidence.error = format!("evaluation request failed: {error}");
                evidence
                    .fixtures
                    .push(failed_evaluation_fixture(fixture, evidence.error.clone()));
                return Ok(evidence);
            }
        };
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            evidence.status = "FAILED".into();
            evidence.error = format!(
                "evaluation model call failed with HTTP {status}: {}",
                bound_text(&body, 512)
            );
            evidence
                .fixtures
                .push(failed_evaluation_fixture(fixture, evidence.error.clone()));
            return Ok(evidence);
        }
        let value: serde_json::Value = match response.json().await {
            Ok(value) => value,
            Err(_) => {
                evidence.status = "FAILED".into();
                evidence.error = "evaluation response is not JSON".into();
                evidence
                    .fixtures
                    .push(failed_evaluation_fixture(fixture, evidence.error.clone()));
                return Ok(evidence);
            }
        };
        let Some(content) = value.get("content").and_then(serde_json::Value::as_str) else {
            evidence.status = "FAILED".into();
            evidence.error = "evaluation response has no content".into();
            evidence
                .fixtures
                .push(failed_evaluation_fixture(fixture, evidence.error.clone()));
            return Ok(evidence);
        };
        let provider = value
            .get("provider")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let model = value
            .get("model")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if evidence.provider.is_empty() {
            evidence.provider = provider.into();
            evidence.model = model.into();
        } else if evidence.provider != provider || evidence.model != model {
            evidence.status = "FAILED".into();
            evidence.error = "evaluation changed provider or model mid-run".into();
            evidence
                .fixtures
                .push(failed_evaluation_fixture(fixture, evidence.error.clone()));
            return Ok(evidence);
        }
        let input = value
            .get("inputTokens")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default();
        let output = value
            .get("outputTokens")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default();
        let total = value
            .get("totalTokens")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default();
        let cost = value
            .get("costMicros")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default();
        let usage_known = value
            .get("usageKnown")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if !usage_known || content.trim().is_empty() {
            evidence.status = "FAILED".into();
            evidence.error = "evaluation returned unknown usage or empty content".into();
            evidence
                .fixtures
                .push(failed_evaluation_fixture(fixture, evidence.error.clone()));
            return Ok(evidence);
        }
        evidence.input_tokens = evidence.input_tokens.saturating_add(input);
        evidence.output_tokens = evidence.output_tokens.saturating_add(output);
        evidence.total_tokens = evidence.total_tokens.saturating_add(total);
        evidence.cost_micros = evidence.cost_micros.saturating_add(cost);
        evidence.fixtures.push(ModelEvaluationFixture {
            fixture_id: fixture.id.clone(),
            status: "PASS".into(),
            reservation_id: value
                .get("reservationId")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .into(),
            provider: provider.into(),
            model: model.into(),
            content: bound_text(content, 64 * 1024),
            input_tokens: input,
            output_tokens: output,
            total_tokens: total,
            cost_micros: cost,
            usage_known,
            error: String::new(),
        });
    }
    Ok(evidence)
}

fn evaluation_prompt(fixture: &EvaluationFixture) -> (String, String) {
    let evidence = serde_json::to_string(&fixture.evidence).unwrap_or_default();
    (
        "You are the CAP-014 semantic evaluator. Return one strict JSON object with keys \
intent, route, scope, unresolved, unknowns, questions, plan, effortBands, confidence, \
evidenceRefs, authority. Do not add prose or markdown. Model output is advisory."
            .into(),
        format!(
            "Request: {}\nEvidence JSON: {}\nReturn the assessment only.",
            fixture.request, evidence
        ),
    )
}

fn failed_evaluation_fixture(fixture: &EvaluationFixture, error: String) -> ModelEvaluationFixture {
    ModelEvaluationFixture {
        fixture_id: fixture.id.clone(),
        status: "FAILED".into(),
        reservation_id: String::new(),
        provider: String::new(),
        model: String::new(),
        content: String::new(),
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        cost_micros: 0,
        usage_known: false,
        error,
    }
}

fn bound_text(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod execution_tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    async fn mock_core(
        axum::extract::State(state): axum::extract::State<(Arc<AtomicUsize>, usize)>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        let (calls, fail_at) = state;
        let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
        if fail_at != 0 && call == fail_at {
            return (
                axum::http::StatusCode::FORBIDDEN,
                axum::Json(serde_json::json!({"error":"budget exhausted"})),
            )
                .into_response();
        }
        (
            axum::http::StatusCode::OK,
            axum::Json(serde_json::json!({
                "reservationId": format!("reservation-{call}"),
                "provider": "opencode-go",
                "model": "deepseek-v4.1-flash",
                "content": "{\"intent\":\"change\"}",
                "inputTokens": 10,
                "outputTokens": 20,
                "totalTokens": 30,
                "costMicros": 5,
                "usageKnown": true
            })),
        )
            .into_response()
    }

    async fn start(
        calls: Arc<AtomicUsize>,
        fail_at: usize,
    ) -> (String, tokio::task::JoinHandle<()>) {
        use axum::{Router, routing::post};
        let app = Router::new()
            .route("/model/calls", post(mock_core))
            .with_state((calls, fail_at));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (url, server)
    }

    #[tokio::test]
    async fn evaluation_records_every_fixture_and_totals() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (url, server) = start(calls.clone(), 0).await;
        let corpus = frozen_corpus().unwrap();
        let evidence = execute_model_evaluation(
            &corpus,
            &ModelEvaluationConfig {
                core_url: url,
                operator_token: "operator-token-1234567890".into(),
                evaluation_id: "evaluation-1".into(),
                timeout: std::time::Duration::from_secs(10),
            },
        )
        .await
        .unwrap();
        assert_eq!(evidence.status, "COMPLETE");
        assert_eq!(evidence.fixtures.len(), 11);
        assert_eq!(calls.load(Ordering::SeqCst), 11);
        assert_eq!(evidence.input_tokens, 110);
        assert_eq!(evidence.output_tokens, 220);
        assert_eq!(evidence.cost_micros, 55);
        assert!(evidence.fixtures.iter().all(|f| f.status == "PASS"));
        server.abort();
    }

    #[tokio::test]
    async fn evaluation_stops_on_denied_budget_and_never_claims_complete() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (url, server) = start(calls.clone(), 6).await;
        let corpus = frozen_corpus().unwrap();
        let evidence = execute_model_evaluation(
            &corpus,
            &ModelEvaluationConfig {
                core_url: url,
                operator_token: "operator-token-1234567890".into(),
                evaluation_id: "evaluation-2".into(),
                timeout: std::time::Duration::from_secs(10),
            },
        )
        .await
        .unwrap();
        assert_eq!(evidence.status, "FAILED");
        assert_eq!(evidence.fixtures.len(), 6);
        assert!(evidence.error.contains("HTTP 403"));
        assert_eq!(calls.load(Ordering::SeqCst), 6);
        server.abort();
    }
}
