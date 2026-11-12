//! Pinned OpenSpec CLI validation.
//!
//! OpenSpec is a projection, not authority. Core renders a deterministic change
//! and asks the official pinned CLI to validate it. The CLI result is evidence
//! for plan review; it never grants acceptance or bypasses a reviewer finding.
//!
//! The CLI is intentionally not a host dependency. The deployment image is
//! built from `deploy/openspec/Dockerfile` with the exact npm version below.

use std::path::Path;
use std::process::Command;

pub const OPENSPEC_CLI_VERSION: &str = "1.16.1";
pub const MAX_CLI_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum OpenSpecCliError {
    #[error("OpenSpec CLI is not available: {0}")]
    Unavailable(String),
    #[error("OpenSpec CLI version mismatch: expected {expected}, found {found}")]
    VersionMismatch { expected: String, found: String },
    #[error("OpenSpec validation failed: {0}")]
    Invalid(String),
    #[error("OpenSpec CLI output exceeded its bound")]
    OutputTooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSpecValidation {
    pub version: String,
    pub change_id: String,
    pub stdout: String,
    pub stderr: String,
}

pub trait OpenSpecRunner {
    fn version(&self, program: &str) -> Result<String, OpenSpecCliError>;
    fn validate(
        &self,
        program: &str,
        workspace: &Path,
        change_id: &str,
    ) -> Result<(String, String), OpenSpecCliError>;
}

pub struct ProcessRunner;

impl OpenSpecRunner for ProcessRunner {
    fn version(&self, program: &str) -> Result<String, OpenSpecCliError> {
        let output = Command::new(program)
            .arg("--version")
            .env("OPENSPEC_NO_UPDATE_CHECK", "1")
            .env("NO_COLOR", "1")
            .output()
            .map_err(|error| OpenSpecCliError::Unavailable(error.to_string()))?;
        if !output.status.success() {
            return Err(OpenSpecCliError::Unavailable(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        if output.stdout.len() > MAX_CLI_OUTPUT_BYTES {
            return Err(OpenSpecCliError::OutputTooLarge);
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn validate(
        &self,
        program: &str,
        workspace: &Path,
        change_id: &str,
    ) -> Result<(String, String), OpenSpecCliError> {
        let output = Command::new(program)
            .args(["validate", change_id, "--strict"])
            .current_dir(workspace)
            .env("OPENSPEC_NO_UPDATE_CHECK", "1")
            .env("NO_COLOR", "1")
            .output()
            .map_err(|error| OpenSpecCliError::Unavailable(error.to_string()))?;
        if output.stdout.len() + output.stderr.len() > MAX_CLI_OUTPUT_BYTES {
            return Err(OpenSpecCliError::OutputTooLarge);
        }
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if !output.status.success() {
            return Err(OpenSpecCliError::Invalid(if stderr.is_empty() {
                stdout
            } else {
                format!("{stdout}\n{stderr}")
            }));
        }
        Ok((stdout, stderr))
    }
}

/// Validate one rendered change with the exact pinned CLI. `workspace` must
/// contain the `openspec/changes/<change_id>` projection.
pub fn validate_with_pinned_cli(
    workspace: &Path,
    change_id: &str,
    runner: &dyn OpenSpecRunner,
) -> Result<OpenSpecValidation, OpenSpecCliError> {
    if !workspace.is_dir() {
        return Err(OpenSpecCliError::Unavailable(
            "OpenSpec workspace is not a directory".into(),
        ));
    }
    let version = runner.version("openspec")?;
    let reported = version
        .split_whitespace()
        .last()
        .unwrap_or_default()
        .trim_start_matches('v');
    if reported != OPENSPEC_CLI_VERSION {
        return Err(OpenSpecCliError::VersionMismatch {
            expected: OPENSPEC_CLI_VERSION.into(),
            found: version,
        });
    }
    let (stdout, stderr) = runner.validate("openspec", workspace, change_id)?;
    Ok(OpenSpecValidation {
        version,
        change_id: change_id.into(),
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeRunner {
        version: String,
        fail: bool,
    }

    impl OpenSpecRunner for FakeRunner {
        fn version(&self, _program: &str) -> Result<String, OpenSpecCliError> {
            Ok(self.version.clone())
        }

        fn validate(
            &self,
            _program: &str,
            _workspace: &Path,
            change_id: &str,
        ) -> Result<(String, String), OpenSpecCliError> {
            if self.fail {
                Err(OpenSpecCliError::Invalid("missing scenario".into()))
            } else {
                Ok((format!("{change_id}: valid"), String::new()))
            }
        }
    }

    #[test]
    fn pinned_version_is_enforced_and_validation_is_bound_to_the_change() {
        let root = std::env::temp_dir().join(format!("factory-openspec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let runner = FakeRunner {
            version: format!("openspec {OPENSPEC_CLI_VERSION}"),
            fail: false,
        };
        let result = validate_with_pinned_cli(&root, "add-validator", &runner).unwrap();
        assert_eq!(result.change_id, "add-validator");
        assert!(result.stdout.contains("add-validator"));
        let wrong = FakeRunner {
            version: "openspec 1.15.0".into(),
            fail: false,
        };
        assert!(matches!(
            validate_with_pinned_cli(&root, "add-validator", &wrong),
            Err(OpenSpecCliError::VersionMismatch { .. })
        ));
        let newer = FakeRunner {
            version: "openspec 1.16.10".into(),
            fail: false,
        };
        assert!(matches!(
            validate_with_pinned_cli(&root, "add-validator", &newer),
            Err(OpenSpecCliError::VersionMismatch { .. })
        ));
        let failing = FakeRunner {
            version: format!("openspec {OPENSPEC_CLI_VERSION}"),
            fail: true,
        };
        assert!(validate_with_pinned_cli(&root, "add-validator", &failing).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
