//! Deterministic pre-candidate failure classification.
//!
//! Classification never calls a model and never grants authority. It only
//! selects whether an existing bounded recovery policy may consider a retry.

use serde::{Deserialize, Serialize};

pub const MAX_REASON_LENGTH: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FailureClass {
    RecoverableImplementationFailure,
    RecoverableExecutionFailure,
    QualityRepairRequired,
    OracleDefectSuspected,
    PolicyOrSecurityBlocked,
    UnresolvedExternalRecovery,
    InfrastructureBlocked,
}

impl FailureClass {
    pub fn is_recoverable(self) -> bool {
        matches!(
            self,
            Self::RecoverableImplementationFailure | Self::RecoverableExecutionFailure
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Classification {
    pub class: FailureClass,
    pub reason: String,
}

pub struct RecoveryInput<'a> {
    pub run_error: &'a str,
    pub stage_detail: Option<&'a str>,
    pub unresolved_recovery: bool,
    pub unresolved_reason: &'a str,
    pub cleanup_unconfirmed: bool,
}

pub fn classify(input: &RecoveryInput<'_>) -> Classification {
    let mut failure = input.run_error.trim().to_owned();
    if failure.is_empty()
        && let Some(detail) = input.stage_detail
    {
        failure = format!("paseo-task {detail}");
    }
    let lower = failure.to_lowercase();
    if input.unresolved_recovery {
        return Classification {
            class: FailureClass::UnresolvedExternalRecovery,
            reason: bounded(&format!(
                "unresolved external recovery: {}",
                input.unresolved_reason
            )),
        };
    }
    if input.cleanup_unconfirmed {
        return Classification {
            class: FailureClass::UnresolvedExternalRecovery,
            reason: "external session cleanup is unconfirmed".into(),
        };
    }
    if contains_any(
        &lower,
        &[
            "browser-test-server",
            "capability:",
            "tool unavailable",
            "browser tooling is unavailable",
            "not configured or installed",
            "participant executable",
        ],
    ) {
        return Classification {
            class: FailureClass::InfrastructureBlocked,
            reason: bounded(&failure),
        };
    }
    if contains_any(
        &lower,
        &[
            "outside the authorized scope",
            "outside authorized scope",
            "protected acceptance oracle was modified",
            "cross-project",
            "forged",
            "identity mismatch",
            "identity differs",
            "unauthorized",
            "stale generation",
            "candidate rejected",
            "repository identity",
            "base revision",
        ],
    ) && contains_any(
        &lower,
        &[
            "outside the authorized scope",
            "outside authorized scope",
            "protected acceptance oracle was modified",
            "cross-project",
            "forged",
            "unauthorized",
            "identity",
            "repository identity",
            "base revision",
        ],
    ) {
        return Classification {
            class: FailureClass::PolicyOrSecurityBlocked,
            reason: bounded(&failure),
        };
    }
    if contains_any(
        &lower,
        &[
            "no-source-change",
            "no source diff",
            "left no source diff",
            "empty candidate",
            "no git diff",
            "tests failed",
            "formatter failed",
            "format failed",
            "syntax",
            "compil",
            "test failure",
            "incomplete change",
        ],
    ) {
        return Classification {
            class: FailureClass::RecoverableImplementationFailure,
            reason: bounded(&failure),
        };
    }
    if contains_any(
        &lower,
        &[
            "session-start",
            "workspace-create",
            "startup",
            "timeout",
            "timed out",
            "temporary",
            "connection reset",
        ],
    ) || (lower.contains("exit 1") && lower.contains("paseo"))
    {
        return Classification {
            class: FailureClass::RecoverableExecutionFailure,
            reason: bounded(&failure),
        };
    }
    if contains_any(&lower, &["budget", "lease", "not authorized", "missing"]) {
        return Classification {
            class: FailureClass::InfrastructureBlocked,
            reason: bounded(&failure),
        };
    }
    Classification {
        class: FailureClass::OracleDefectSuspected,
        reason: bounded(&format!(
            "ambiguous failure requires independent assessment: {failure}"
        )),
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn bounded(value: &str) -> String {
    value.trim().chars().take(MAX_REASON_LENGTH).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(error: &str) -> RecoveryInput<'_> {
        RecoveryInput {
            run_error: error,
            stage_detail: None,
            unresolved_recovery: false,
            unresolved_reason: "",
            cleanup_unconfirmed: false,
        }
    }

    #[test]
    fn classifies_recoverable_failures() {
        for failure in [
            "paseo-task verify: no-source-change: agent left no git diff",
            "candidate: implementer left no source diff; an empty candidate is not success",
            "paseo-task verify: tests failed: npm exited 1",
            "paseo-task verify: formatter failed: npm exited 1",
            "paseo-task session-start: paseo run failed (startup): exit 1",
        ] {
            let result = classify(&input(failure));
            assert!(
                result.class.is_recoverable(),
                "{failure:?} classified as {:?}",
                result.class
            );
        }
    }

    #[test]
    fn classifies_blocking_failures() {
        for failure in [
            "candidate: candidate rejected: candidate changed path \"tests/browser-check.mjs\" outside the authorized scope",
            "paseo-task verify: protected acceptance oracle was modified by the candidate: browser-check.mjs",
            "candidate: cross-project evidence mismatch",
            "candidate: repository identity differs from the frozen project binding",
            "capability: browser capability is required but Playwright CLI/Chromium unavailable",
            "browser-test-server: authorized endpoint is occupied by an unowned process",
        ] {
            let result = classify(&input(failure));
            assert!(
                matches!(
                    result.class,
                    FailureClass::PolicyOrSecurityBlocked | FailureClass::InfrastructureBlocked
                ),
                "{failure:?} classified as {:?}",
                result.class
            );
        }
    }

    #[test]
    fn classifies_unresolved_and_ambiguous_failures() {
        let result = classify(&RecoveryInput {
            run_error: "paseo-task observe: cancelled",
            stage_detail: None,
            unresolved_recovery: true,
            unresolved_reason: "paseo session binding active is not terminal",
            cleanup_unconfirmed: false,
        });
        assert_eq!(result.class, FailureClass::UnresolvedExternalRecovery);
        assert!(result.reason.starts_with("unresolved external recovery: "));

        let result = classify(&input("something completely unexpected"));
        assert_eq!(result.class, FailureClass::OracleDefectSuspected);

        let result = classify(&input("paseo-task verify: no-source-change"));
        assert_eq!(result.class, FailureClass::RecoverableImplementationFailure);
    }

    #[test]
    fn cleanup_uncertainty_is_never_recoverable() {
        let result = classify(&RecoveryInput {
            run_error: "timeout",
            stage_detail: None,
            unresolved_recovery: false,
            unresolved_reason: "",
            cleanup_unconfirmed: true,
        });
        assert_eq!(result.class, FailureClass::UnresolvedExternalRecovery);
        assert!(!result.class.is_recoverable());
    }

    #[test]
    fn stage_detail_is_used_when_the_error_is_empty() {
        let result = classify(&RecoveryInput {
            run_error: "  ",
            stage_detail: Some("verify: syntax error"),
            unresolved_recovery: false,
            unresolved_reason: "",
            cleanup_unconfirmed: false,
        });
        assert_eq!(result.class, FailureClass::RecoverableImplementationFailure);
    }

    #[test]
    fn reasons_are_bounded_and_named_for_serialization() {
        let long = "x".repeat(MAX_REASON_LENGTH + 50);
        let result = classify(&input(&long));
        assert_eq!(result.reason.chars().count(), MAX_REASON_LENGTH);
        assert_eq!(
            serde_json::to_string(&FailureClass::PolicyOrSecurityBlocked).unwrap(),
            "\"POLICY_OR_SECURITY_BLOCKED\""
        );
    }
}
