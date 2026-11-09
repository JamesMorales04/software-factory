//! Deterministic projection of a frozen specification to GitHub issues.
//!
//! The frozen specification and WorkGraph in Core remain authoritative. Issue
//! bodies carry deterministic markers so publication is idempotent and
//! recoverable. Issue edits or comments are operator context; they never
//! mutate the frozen record.

use crate::agentic::{FrozenSpec, ValidatedWorkGraph};
use crate::db::{AgenticSpecIssueRecord, AgenticSpecRecord, Database, Error};
use crate::github::{GitHubClient, GitHubError, NewGitHubIssue};
use crate::github_sync::GitHubCredential;

pub const EPIC_UNIT_ID: &str = "epic";
pub const SPEC_LABEL: &str = "factory-spec";

#[derive(Debug, thiserror::Error)]
pub enum SpecPublishError {
    #[error("database error: {0}")]
    Database(#[from] Error),
    #[error("GitHub upstream error: {0}")]
    GitHub(#[from] GitHubError),
    #[error("invalid spec publication: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecIssueTarget {
    pub unit_id: String,
    pub marker: String,
    pub title: String,
    pub body: String,
}

fn marker(spec_id: &str, unit_id: &str) -> String {
    format!("<!-- factory-spec:{spec_id}:{unit_id} -->")
}

fn bounded_title(text: &str) -> String {
    text.chars().take(200).collect()
}

/// Build the exact issue projection. IDs, ordering and bodies are deterministic
/// for a frozen spec revision.
pub fn build_issue_targets(
    objective: &str,
    spec_record_id: &str,
    spec: &FrozenSpec,
    graph: &ValidatedWorkGraph,
) -> Vec<SpecIssueTarget> {
    let mut targets = Vec::with_capacity(graph.units.len() + 1);
    let mut epic = String::new();
    epic.push_str(&marker(spec_record_id, EPIC_UNIT_ID));
    epic.push('\n');
    epic.push_str(&format!(
        "# {} (revision {})\n\n",
        objective.trim(),
        spec.revision
    ));
    epic.push_str(&format!("{}\n\n", spec.summary.trim()));
    epic.push_str(&format!("- Spec SHA-256: `{}`\n", spec.spec_sha256));
    epic.push_str(&format!("- WorkGraph SHA-256: `{}`\n", graph.graph_sha256));
    epic.push_str(&format!(
        "- Graph order: {}\n\n",
        graph.topological_order.join(" → ")
    ));
    epic.push_str("## Work units\n\n");
    for unit in &graph.units {
        epic.push_str(&format!(
            "- `{}` — {} (depends on: {})\n",
            unit.id,
            unit.title.trim(),
            if unit.depends_on.is_empty() {
                "none".to_string()
            } else {
                unit.depends_on.join(", ")
            }
        ));
    }
    epic.push_str("\n## Requirement traceability\n\n");
    for requirement in &spec.requirements {
        epic.push_str(&format!(
            "- `{}` {} — {}\n",
            requirement.id,
            requirement.statement.trim(),
            requirement
                .assertions
                .iter()
                .map(|assertion| format!("`{}` → `{}`", assertion.id, assertion.check_id))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !spec.assumptions.is_empty() {
        epic.push_str("\n## Assumptions\n\n");
        for assumption in &spec.assumptions {
            epic.push_str(&format!("- {}\n", assumption.trim()));
        }
    }
    if !spec.risks.is_empty() {
        epic.push_str("\n## Risks\n\n");
        for risk in &spec.risks {
            epic.push_str(&format!("- {}\n", risk.trim()));
        }
    }
    epic.push_str(
        "\n---\nCore is authoritative. Edited issue content is operator context and cannot mutate the frozen specification.\n",
    );
    targets.push(SpecIssueTarget {
        unit_id: EPIC_UNIT_ID.into(),
        marker: marker(spec_record_id, EPIC_UNIT_ID),
        title: bounded_title(&format!(
            "[Factory spec] {objective} (rev {})",
            spec.revision
        )),
        body: epic,
    });
    for unit in &graph.units {
        let mut body = String::new();
        body.push_str(&marker(spec_record_id, &unit.id));
        body.push('\n');
        body.push_str(&format!("# {}\n\n", unit.title.trim()));
        body.push_str(&format!("{}\n\n", unit.objective.trim()));
        body.push_str(&format!(
            "- Spec: `{spec_record_id}` revision {}\n",
            spec.revision
        ));
        body.push_str(&format!("- Spec SHA-256: `{}`\n", spec.spec_sha256));
        body.push_str(&format!("- Work unit: `{}`\n", unit.id));
        body.push_str(&format!(
            "- Depends on: {}\n",
            if unit.depends_on.is_empty() {
                "none".to_string()
            } else {
                unit.depends_on.join(", ")
            }
        ));
        body.push_str("- Paths:\n");
        for path in &unit.paths {
            body.push_str(&format!("  - `{path}`\n"));
        }
        body.push_str("\n## Acceptance\n\n");
        for acceptance in &unit.acceptance {
            body.push_str(&format!("- {}\n", acceptance.trim()));
        }
        body.push_str(
            "\n---\nCore is authoritative. This issue is a projection; it cannot widen the frozen scope.\n",
        );
        targets.push(SpecIssueTarget {
            unit_id: unit.id.clone(),
            marker: marker(spec_record_id, &unit.id),
            title: bounded_title(&format!("[Factory spec] {}", unit.title.trim())),
            body,
        });
    }
    targets
}

/// Parse a frozen record's exact spec and graph. A missing or mismatched value
/// fails closed instead of publishing a partial projection.
pub fn parse_frozen(
    record: &AgenticSpecRecord,
) -> Result<(FrozenSpec, ValidatedWorkGraph), SpecPublishError> {
    if record.state != "FROZEN" {
        return Err(SpecPublishError::Invalid(
            "only a frozen specification can be published".into(),
        ));
    }
    let frozen_json = record
        .frozen_json
        .as_deref()
        .ok_or_else(|| SpecPublishError::Invalid("frozen specification is missing".into()))?;
    let graph_json = record
        .graph_json
        .as_deref()
        .ok_or_else(|| SpecPublishError::Invalid("frozen work graph is missing".into()))?;
    let spec: FrozenSpec = serde_json::from_str(frozen_json)
        .map_err(|_| SpecPublishError::Invalid("frozen specification is not decodable".into()))?;
    let graph: ValidatedWorkGraph = serde_json::from_str(graph_json)
        .map_err(|_| SpecPublishError::Invalid("frozen work graph is not decodable".into()))?;
    if Some(&spec.spec_sha256) != record.spec_sha256.as_ref()
        || Some(&graph.graph_sha256) != record.graph_sha256.as_ref()
    {
        return Err(SpecPublishError::Invalid(
            "frozen identity does not match the stored record".into(),
        ));
    }
    Ok((spec, graph))
}

/// Publish or update the exact issue projection. Existing refs are updated;
/// otherwise a bounded marker scan recovers a create whose local write failed.
pub async fn publish_spec(
    db: &Database,
    github: &GitHubClient,
    credential: &GitHubCredential,
    record: &AgenticSpecRecord,
    objective: &str,
    owner: &str,
    repo: &str,
) -> Result<Vec<AgenticSpecIssueRecord>, SpecPublishError> {
    let (spec, graph) = parse_frozen(record)?;
    let targets = build_issue_targets(objective, &record.id, &spec, &graph);
    let labels = [SPEC_LABEL.to_string()];
    let mut published = Vec::with_capacity(targets.len());
    for target in targets {
        let existing = db
            .agentic_spec_issue(&record.id, &target.unit_id)
            .await
            .ok();
        let issue = match existing.as_ref().and_then(|issue| issue.external_number) {
            Some(number) => {
                github
                    .update_issue(
                        credential.token(),
                        owner,
                        repo,
                        u64::try_from(number).map_err(|_| {
                            SpecPublishError::Invalid("issue number is invalid".into())
                        })?,
                        NewGitHubIssue {
                            title: &target.title,
                            body: &target.body,
                            labels: &labels,
                        },
                    )
                    .await?
            }
            None => {
                let recovered = github
                    .find_issue_by_marker(credential.token(), owner, repo, &target.marker)
                    .await?;
                match recovered {
                    Some(issue) => {
                        github
                            .update_issue(
                                credential.token(),
                                owner,
                                repo,
                                issue.number,
                                NewGitHubIssue {
                                    title: &target.title,
                                    body: &target.body,
                                    labels: &labels,
                                },
                            )
                            .await?
                    }
                    None => {
                        github
                            .create_issue(
                                credential.token(),
                                owner,
                                repo,
                                NewGitHubIssue {
                                    title: &target.title,
                                    body: &target.body,
                                    labels: &labels,
                                },
                            )
                            .await?
                    }
                }
            }
        };
        published.push(
            db.record_agentic_spec_issue(
                &record.id,
                &target.unit_id,
                &target.marker,
                i64::try_from(issue.number).ok(),
                Some(&issue.html_url),
                "PUBLISHED",
            )
            .await?,
        );
    }
    Ok(published)
}

/// Re-read the projected issues and report drift. Core never repairs drift
/// silently; the operator can republish the projection.
pub async fn refresh_publication(
    db: &Database,
    github: &GitHubClient,
    credential: &GitHubCredential,
    record: &AgenticSpecRecord,
    owner: &str,
    repo: &str,
) -> Result<bool, SpecPublishError> {
    let (spec, _) = parse_frozen(record)?;
    let issues = db.agentic_spec_issues(&record.id).await?;
    if issues.is_empty() {
        return Ok(false);
    }
    let mut in_sync = true;
    for issue in issues {
        let Some(number) = issue.external_number else {
            in_sync = false;
            continue;
        };
        let external = match github
            .get_issue(
                credential.token(),
                owner,
                repo,
                u64::try_from(number)
                    .map_err(|_| SpecPublishError::Invalid("issue number is invalid".into()))?,
            )
            .await
        {
            Ok(issue) => issue,
            Err(GitHubError::NotFound) => {
                in_sync = false;
                continue;
            }
            Err(error) => return Err(SpecPublishError::GitHub(error)),
        };
        if !external.body.contains(&issue.marker)
            || !external.body.contains(&spec.spec_sha256)
            || external.state != "open"
        {
            in_sync = false;
        }
    }
    Ok(in_sync)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentic::{
        PlannedWorkUnit, SpecDraftModel, SpecRequirementDraft, WorkGraphModel, compile_spec,
        validate_work_graph,
    };

    fn graph() -> ValidatedWorkGraph {
        validate_work_graph(
            &WorkGraphModel {
                units: vec![PlannedWorkUnit {
                    id: "api".into(),
                    title: "API".into(),
                    objective: "Expose the validator".into(),
                    depends_on: vec![],
                    paths: vec!["src/api.rs".into()],
                    acceptance: vec!["invalid input returns an error".into()],
                }],
                assumptions: vec![],
                risks: vec![],
            },
            &["src".into()],
        )
        .unwrap()
    }

    fn spec() -> FrozenSpec {
        compile_spec(
            &SpecDraftModel {
                summary: "Bounded validator".into(),
                requirements: vec![SpecRequirementDraft {
                    statement: "Invalid input is rejected.".into(),
                    acceptance: vec!["invalid input returns an error".into()],
                    negative: vec!["valid input is accepted".into()],
                }],
                assumptions: vec![],
                risks: vec![],
            },
            1,
        )
        .unwrap()
    }

    #[test]
    fn targets_are_deterministic_and_carry_traceability() {
        let spec = spec();
        let targets = build_issue_targets("Fix validation", "spec-1", &spec, &graph());
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].unit_id, EPIC_UNIT_ID);
        assert!(targets[0].body.contains(&spec.spec_sha256));
        assert!(targets[0].body.contains("CHK-001-ACC-01"));
        assert!(targets[1].body.contains("<!-- factory-spec:spec-1:api -->"));
        assert!(targets[1].body.contains("invalid input returns an error"));
        let again = build_issue_targets("Fix validation", "spec-1", &spec, &graph());
        assert_eq!(targets, again);
    }
}
