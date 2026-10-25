//! Deterministic final-diff review.
//!
//! The reviewer verifies the exact candidate identity and scope, then applies
//! closed static rules to the real diff. It never edits the candidate, never
//! executes candidate code, and never grants authority. Model-based review is
//! not implemented.

use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::workspace::{self, WorkspaceError};

pub const MAX_REVIEW_PATHS: usize = 50;
pub const MAX_FINDINGS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Approve,
    RequestChanges,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FindingSeverity {
    Blocking,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FindingCategory {
    Functional,
    Architecture,
    Security,
    Testing,
    Scope,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReviewFinding {
    #[schemars(length(min = 1, max = 64))]
    pub id: String,
    pub severity: FindingSeverity,
    pub category: FindingCategory,
    #[schemars(length(min = 1, max = 2000))]
    pub message: String,
    #[schemars(length(min = 1, max = 16))]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewEvidence {
    pub commit_sha: String,
    pub tree_sha: String,
    pub changed_paths: Vec<String>,
    pub diff_stat: String,
    pub diff_bytes: usize,
    pub findings: Vec<ReviewFinding>,
    pub verdict: ReviewVerdict,
}

#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    #[error("candidate facts could not be read: {0}")]
    Candidate(#[from] WorkspaceError),
}

pub struct ReviewRequest<'a> {
    pub repository: &'a Path,
    pub base_commit: &'a str,
    pub commit_sha: &'a str,
    pub tree_sha: &'a str,
    pub allowed_paths: &'a [String],
    pub integration_lineage: Option<&'a [String]>,
}

pub fn review_candidate(request: &ReviewRequest<'_>) -> Result<ReviewEvidence, ReviewError> {
    let (commit_sha, tree_sha) = match request.integration_lineage {
        Some(lineage) => {
            let expected: Vec<String> = lineage
                .iter()
                .filter(|parent| !parent.eq_ignore_ascii_case(request.base_commit))
                .cloned()
                .collect();
            let verified = workspace::verify_integrated_candidate(
                request.repository,
                request.base_commit,
                request.commit_sha,
                request.tree_sha,
                &expected,
            )?;
            (verified.commit_sha, verified.tree_sha)
        }
        None => {
            let verified = workspace::verify_candidate_commit(
                request.repository,
                request.base_commit,
                request.commit_sha,
                request.tree_sha,
            )?;
            (verified.commit_sha, verified.tree_sha)
        }
    };
    let changed_paths =
        workspace::changed_paths(request.repository, request.base_commit, &commit_sha)?;
    let (diff, diff_bytes) = workspace::diff(request.repository, request.base_commit, &commit_sha)?;
    let diff_stat = workspace::diff_stat(request.repository, request.base_commit, &commit_sha)
        .unwrap_or_default();
    let evidence = evaluate_diff(
        &changed_paths,
        request.allowed_paths,
        &diff,
        diff_bytes,
        diff_stat,
        commit_sha,
        tree_sha,
    );
    Ok(evidence)
}

fn evaluate_diff(
    changed_paths: &[String],
    allowed_paths: &[String],
    diff: &str,
    diff_bytes: usize,
    diff_stat: String,
    commit_sha: String,
    tree_sha: String,
) -> ReviewEvidence {
    let mut findings = Vec::new();
    if changed_paths.len() > MAX_REVIEW_PATHS {
        add_finding(
            &mut findings,
            FindingSeverity::Blocking,
            FindingCategory::Scope,
            "final diff changes too many paths for this bounded task",
            &[],
        );
    }
    if !allowed_paths.is_empty()
        && let Some(path) = changed_paths
            .iter()
            .find(|path| !workspace::path_covered(path, allowed_paths))
    {
        add_finding(
            &mut findings,
            FindingSeverity::Blocking,
            FindingCategory::Scope,
            &format!("candidate changed path {path:?} outside the frozen allowed paths"),
            std::slice::from_ref(path),
        );
    }
    if diff.contains("Binary files ") {
        add_finding(
            &mut findings,
            FindingSeverity::Warning,
            FindingCategory::Other,
            "final diff contains a binary file change",
            &[],
        );
    }
    let source_change = changed_paths.iter().any(|path| {
        !path.ends_with("_test.go") && !path.contains(".test.") && !path.starts_with("docs/")
    });
    if !source_change {
        add_finding(
            &mut findings,
            FindingSeverity::Blocking,
            FindingCategory::Scope,
            "candidate changes no production source file",
            &[],
        );
    }
    for line in workspace::added_lines(diff) {
        let lower = line.to_lowercase();
        if [
            "begin private key",
            "aws_secret",
            "ghp_",
            "api_key=",
            "password=",
            "secret=",
        ]
        .iter()
        .any(|pattern| lower.contains(pattern))
        {
            add_finding(
                &mut findings,
                FindingSeverity::Blocking,
                FindingCategory::Security,
                "added line contains a secret-shaped value",
                &[],
            );
        } else if lower.contains("child_process") || lower.contains("eval(") {
            add_finding(
                &mut findings,
                FindingSeverity::Warning,
                FindingCategory::Security,
                "added line introduces a high-risk dynamic execution primitive",
                &[],
            );
        }
    }
    let verdict = if findings
        .iter()
        .any(|finding| finding.severity == FindingSeverity::Blocking)
    {
        ReviewVerdict::RequestChanges
    } else {
        ReviewVerdict::Approve
    };
    ReviewEvidence {
        commit_sha,
        tree_sha,
        changed_paths: changed_paths.to_vec(),
        diff_stat,
        diff_bytes,
        findings,
        verdict,
    }
}

fn add_finding(
    findings: &mut Vec<ReviewFinding>,
    severity: FindingSeverity,
    category: FindingCategory,
    message: &str,
    evidence_refs: &[String],
) {
    if findings.len() < MAX_FINDINGS {
        findings.push(ReviewFinding {
            id: format!("review-{}", findings.len() + 1),
            severity,
            category,
            message: message.into(),
            evidence_refs: evidence_refs.to_vec(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn diff_with(line: &str) -> String {
        format!("diff --git a/src/file b/src/file\n--- a/src/file\n+++ b/src/file\n+{line}\n")
    }

    fn evaluate(diff: &str) -> ReviewEvidence {
        evaluate_diff(
            &["src/file".into()],
            &["src".into()],
            diff,
            diff.len(),
            "1 file changed".into(),
            "a".repeat(40),
            "b".repeat(40),
        )
    }

    #[test]
    fn accepts_production_source_with_a_dynamic_execution_warning() {
        let evidence = evaluate(&diff_with("const run = eval(input);"));
        assert_eq!(evidence.verdict, ReviewVerdict::Approve);
        assert_eq!(evidence.findings.len(), 1);
        assert_eq!(evidence.findings[0].severity, FindingSeverity::Warning);
    }

    #[test]
    fn requests_changes_for_secret_shaped_lines_without_echoing_the_value() {
        let evidence = evaluate(&diff_with("password=hunter2"));
        assert_eq!(evidence.verdict, ReviewVerdict::RequestChanges);
        assert_eq!(evidence.findings[0].severity, FindingSeverity::Blocking);
        assert!(!format!("{evidence:?}").contains("hunter2"));
    }

    #[test]
    fn rejects_out_of_scope_and_test_only_changes() {
        let out_of_scope = evaluate_diff(
            &["tests/journey.test.mjs".into()],
            &["src".into()],
            &diff_with("test"),
            diff_with("test").len(),
            String::new(),
            "a".repeat(40),
            "b".repeat(40),
        );
        assert_eq!(out_of_scope.verdict, ReviewVerdict::RequestChanges);
        assert_eq!(
            out_of_scope
                .findings
                .iter()
                .filter(|finding| finding.severity == FindingSeverity::Blocking)
                .count(),
            2
        );
    }

    #[test]
    fn rejects_too_many_paths() {
        let paths: Vec<String> = (0..=MAX_REVIEW_PATHS)
            .map(|index| format!("src/file-{index}"))
            .collect();
        let evidence = evaluate_diff(
            &paths,
            &["src".into()],
            &diff_with("change"),
            diff_with("change").len(),
            String::new(),
            "a".repeat(40),
            "b".repeat(40),
        );
        assert_eq!(evidence.verdict, ReviewVerdict::RequestChanges);
        assert!(
            evidence
                .findings
                .iter()
                .any(|finding| finding.message.contains("too many paths"))
        );
    }

    struct TempRepo(PathBuf);

    impl TempRepo {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "factory-review-test-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            git(&path, &["init", "-q"]);
            git(&path, &["config", "user.email", "review@example.invalid"]);
            git(&path, &["config", "user.name", "Review Test"]);
            fs::write(path.join("seed.txt"), "base\n").unwrap();
            git(&path, &["add", "seed.txt"]);
            git(&path, &["commit", "-qm", "base"]);
            Self(path)
        }

        fn head(&self) -> String {
            String::from_utf8(
                Command::new("git")
                    .args(["-C", self.0.to_str().unwrap(), "rev-parse", "HEAD"])
                    .output()
                    .unwrap()
                    .stdout,
            )
            .unwrap()
            .trim()
            .into()
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn verifies_real_candidate_identity_and_diff_facts() {
        let repo = TempRepo::new();
        let base = repo.head();
        fs::write(repo.0.join("seed.txt"), "candidate\n").unwrap();
        git(&repo.0, &["add", "seed.txt"]);
        git(&repo.0, &["commit", "-qm", "candidate"]);
        let commit = repo.head();
        let tree = String::from_utf8(
            Command::new("git")
                .args([
                    "-C",
                    repo.0.to_str().unwrap(),
                    "show",
                    "-s",
                    "--format=%T",
                    &commit,
                ])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        let evidence = review_candidate(&ReviewRequest {
            repository: &repo.0,
            base_commit: &base,
            commit_sha: &commit,
            tree_sha: &tree,
            allowed_paths: &["seed.txt".into()],
            integration_lineage: None,
        })
        .unwrap();
        assert_eq!(evidence.commit_sha, commit);
        assert_eq!(evidence.tree_sha, tree);
        assert_eq!(evidence.changed_paths, vec!["seed.txt"]);
        assert_eq!(evidence.verdict, ReviewVerdict::Approve);
        assert!(evidence.diff_bytes > 0);
    }

    #[test]
    fn rejects_a_claim_with_the_wrong_tree() {
        let repo = TempRepo::new();
        let base = repo.head();
        fs::write(repo.0.join("seed.txt"), "candidate\n").unwrap();
        git(&repo.0, &["add", "seed.txt"]);
        git(&repo.0, &["commit", "-qm", "candidate"]);
        let commit = repo.head();
        assert!(
            review_candidate(&ReviewRequest {
                repository: &repo.0,
                base_commit: &base,
                commit_sha: &commit,
                tree_sha: &"0".repeat(40),
                allowed_paths: &[],
                integration_lineage: None,
            })
            .is_err()
        );
    }
}
