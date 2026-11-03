//! Disposable engineering workspace used by the deterministic journey.
//!
//! Boundaries are deliberately narrow:
//!
//! * Only the fixed fixture tasks below can write files or run commands.
//! * Paths are relative, validated, and contained in the workspace root.
//! * Commands are fixed vectors run with a cleared environment, a fixed
//!   `PATH`, a timeout and bounded output. No network, GitHub credential or
//!   host-project path is made available.
//! * The reviewer runs in a separate copy of the candidate.
//!
//! This is a process-level boundary, not a certified sandbox. The module also
//! exposes a Podman command adapter for environments where an approved image
//! is available; the deterministic tests report which mode actually ran.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::process::Command as TokioCommand;

use crate::model::JourneyPlan;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const OUTPUT_LIMIT: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum EngineeringError {
    #[error("filesystem: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid task or path: {0}")]
    Invalid(String),
    #[error("command failed: {0}")]
    Command(String),
    #[error("test failure: {0}")]
    TestFailure(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandEvidence {
    pub command: Vec<String>,
    pub exit_code: i32,
    pub output_sha256: String,
    pub output: String,
    pub timed_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkerArtifact {
    pub marker_path: String,
    pub marker_sha256: String,
    pub result: serde_json::Value,
    pub candidate: Option<CandidateEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateEvidence {
    pub candidate_sha256: String,
    pub git_revision: String,
    pub manifest: serde_json::Value,
    pub review: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum SandboxMode {
    /// Fixed command allowlist in a disposable directory on the host. Not a
    /// security sandbox; used for fast deterministic tests.
    LocalProcess,
    /// Fixed command allowlist executed in a rootless Podman container with no
    /// network, dropped capabilities, read-only rootfs and a workspace mount.
    Podman { image: String },
}

#[derive(Debug, Clone)]
pub struct EngineeringWorkspace {
    root: PathBuf,
    base: PathBuf,
    agents: PathBuf,
    candidate: PathBuf,
    reviews: PathBuf,
    markers: PathBuf,
    candidate_lock: Arc<tokio::sync::Mutex<()>>,
    sandbox: SandboxMode,
}

impl EngineeringWorkspace {
    pub fn new(root: impl Into<PathBuf>, _plan: Arc<JourneyPlan>, sandbox: SandboxMode) -> Self {
        let root = root.into();
        Self {
            base: root.join("base"),
            agents: root.join("agents"),
            candidate: root.join("candidate"),
            reviews: root.join("reviews"),
            markers: root.join("markers"),
            root,
            candidate_lock: Default::default(),
            sandbox,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Append a durable record of an actual external application. If recovery
    /// ever re-executes rather than reconciles an effect, the duplicate line is
    /// visible evidence.
    pub fn record_application(&self, effect_key: &str) -> Result<(), EngineeringError> {
        use std::io::Write;
        let path = self.root.join("applied-effects.log");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(file, "{effect_key}")?;
        file.sync_all()?;
        Ok(())
    }

    pub async fn initialize(&self) -> Result<(), EngineeringError> {
        std::fs::create_dir_all(&self.agents)?;
        std::fs::create_dir_all(&self.reviews)?;
        std::fs::create_dir_all(&self.markers)?;
        if !self.base.join(".git").exists() {
            std::fs::create_dir_all(&self.base)?;
            self.git(&self.base, &["init", "-q"])?;
            self.git(&self.base, &["config", "user.name", "Factory Fixture"])?;
            self.git(
                &self.base,
                &["config", "user.email", "fixture@example.invalid"],
            )?;
            atomic_write(&self.base.join("calculator.py"), CALCULATOR_STUB.as_bytes())?;
            atomic_write(&self.base.join("test_calculator.py"), BASE_TESTS.as_bytes())?;
            atomic_write(
                &self.base.join("README.md"),
                b"Disposable factory fixture.\n",
            )?;
            self.git(&self.base, &["add", "."])?;
            self.git_commit(&self.base, "fixture: seed calculator")?;
        }
        Ok(())
    }

    pub async fn perform(&self, task_id: &str) -> Result<MarkerArtifact, EngineeringError> {
        match task_id {
            "implement-multiply" => self.implement_multiply().await,
            "implement-tests" => self.implement_tests().await,
            "review" => self.review().await,
            other => Err(EngineeringError::Invalid(format!(
                "task '{other}' is not in the fixed fixture allowlist"
            ))),
        }
    }

    async fn implement_multiply(&self) -> Result<MarkerArtifact, EngineeringError> {
        let workspace = self.agent_workspace("implement-multiply")?;
        atomic_write(
            &workspace.join("calculator.py"),
            CALCULATOR_IMPLEMENTED.as_bytes(),
        )?;
        let test = run_command(
            &workspace,
            &["python3", "-m", "unittest", "-q"],
            &self.sandbox,
        )
        .await?;
        if test.exit_code != 0 || test.timed_out {
            return Err(EngineeringError::TestFailure(format!(
                "implement-multiply baseline tests failed: {}",
                test.output
            )));
        }
        let files = file_hashes(&workspace, &["calculator.py"])?;
        self.write_marker(
            "implement-multiply",
            serde_json::json!({
                "task_id": "implement-multiply",
                "status": "PASS",
                "workspace": workspace.display().to_string(),
                "files": files,
                "tests": test,
                "confinement": self.confinement_label()
            }),
            None,
        )
    }

    async fn implement_tests(&self) -> Result<MarkerArtifact, EngineeringError> {
        let workspace = self.agent_workspace("implement-tests")?;
        atomic_write(
            &workspace.join("test_calculator.py"),
            MULTIPLY_TESTS.as_bytes(),
        )?;
        let syntax = run_command(
            &workspace,
            &[
                "python3",
                "-c",
                "import ast; ast.parse(open('test_calculator.py').read())",
            ],
            &self.sandbox,
        )
        .await?;
        if syntax.exit_code != 0 || syntax.timed_out {
            return Err(EngineeringError::TestFailure(format!(
                "test fixture did not compile: {}",
                syntax.output
            )));
        }
        let baseline = run_command(
            &workspace,
            &[
                "python3",
                "-m",
                "unittest",
                "test_calculator.TestCalculator.test_add",
                "-q",
            ],
            &self.sandbox,
        )
        .await?;
        if baseline.exit_code != 0 || baseline.timed_out {
            return Err(EngineeringError::TestFailure(format!(
                "baseline add test failed: {}",
                baseline.output
            )));
        }
        let files = file_hashes(&workspace, &["test_calculator.py"])?;
        self.write_marker(
            "implement-tests",
            serde_json::json!({
                "task_id": "implement-tests",
                "status": "PASS",
                "workspace": workspace.display().to_string(),
                "files": files,
                "checks": [syntax, baseline],
                "full_suite": "DEFERRED_TO_INTEGRATION",
                "confinement": self.confinement_label()
            }),
            None,
        )
    }

    async fn review(&self) -> Result<MarkerArtifact, EngineeringError> {
        let _guard = self.candidate_lock.lock().await;
        let multiply = self.agent_workspace("implement-multiply")?;
        let tests = self.agent_workspace("implement-tests")?;
        if !self.markers.join("implement-multiply.json").exists()
            || !self.markers.join("implement-tests.json").exists()
        {
            return Err(EngineeringError::Invalid(
                "review requested before both implementer markers exist".to_owned(),
            ));
        }

        if self.candidate.exists() {
            std::fs::remove_dir_all(&self.candidate)?;
        }
        copy_dir(&self.base, &self.candidate)?;
        copy_one(
            &multiply.join("calculator.py"),
            &self.candidate.join("calculator.py"),
        )?;
        copy_one(
            &tests.join("test_calculator.py"),
            &self.candidate.join("test_calculator.py"),
        )?;
        self.git(&self.candidate, &["add", "."])?;
        self.git_commit(&self.candidate, "fixture: integrate calculator changes")?;
        let git_revision = self.git_output(&self.candidate, &["rev-parse", "HEAD"])?;

        let tests_run = run_command(
            &self.candidate,
            &["python3", "-m", "unittest", "-q"],
            &self.sandbox,
        )
        .await?;
        if tests_run.exit_code != 0 || tests_run.timed_out {
            let marker = self.write_marker(
                "review",
                serde_json::json!({
                    "task_id": "review",
                    "status": "FAIL",
                    "git_revision": git_revision,
                    "tests": tests_run,
                    "review": { "verdict": "FAIL", "reason": "candidate tests failed" }
                }),
                None,
            )?;
            return Ok(marker);
        }

        let manifest = candidate_manifest(&self.candidate, &git_revision, &tests_run)?;
        let candidate_sha256 = manifest
            .get("candidate_sha256")
            .and_then(|value| value.as_str())
            .expect("bug: manifest candidate hash")
            .to_owned();

        // Independent review: fresh copy, fresh process, recompute everything.
        let review_dir = self.reviews.join("independent");
        if review_dir.exists() {
            std::fs::remove_dir_all(&review_dir)?;
        }
        copy_dir(&self.candidate, &review_dir)?;
        let review_tests = run_command(
            &review_dir,
            &["python3", "-m", "unittest", "-q"],
            &self.sandbox,
        )
        .await?;
        let review_manifest = candidate_manifest(&review_dir, &git_revision, &review_tests)?;
        let review_sha256 = review_manifest
            .get("candidate_sha256")
            .and_then(|value| value.as_str())
            .expect("bug: review manifest candidate hash")
            .to_owned();
        let verdict = if review_tests.exit_code == 0
            && !review_tests.timed_out
            && review_sha256 == candidate_sha256
        {
            "PASS"
        } else {
            "FAIL"
        };
        let review = serde_json::json!({
            "verdict": verdict,
            "reviewer": "independent-fresh-copy",
            "candidate_sha256": candidate_sha256,
            "recomputed_sha256": review_sha256,
            "git_revision": git_revision,
            "tests": review_tests,
            "original_manifest": manifest,
            "confinement": self.confinement_label()
        });
        let candidate = CandidateEvidence {
            candidate_sha256: candidate_sha256.clone(),
            git_revision: git_revision.clone(),
            manifest: manifest.clone(),
            review: review.clone(),
        };
        self.write_marker(
            "review",
            serde_json::json!({
                "task_id": "review",
                "status": verdict,
                "candidate_sha256": candidate_sha256,
                "git_revision": git_revision,
                "manifest": manifest,
                "review": review
            }),
            Some(candidate),
        )
    }

    fn confinement_label(&self) -> &'static str {
        match self.sandbox {
            SandboxMode::LocalProcess => "LOCAL_PROCESS_FIXED_ALLOWLIST_NOT_CERTIFIED",
            SandboxMode::Podman { .. } => "ROOTLESS_PODMAN_UNCERTIFIED",
        }
    }

    fn agent_workspace(&self, task_id: &str) -> Result<PathBuf, EngineeringError> {
        let workspace = self.agents.join(task_id);
        if !workspace.exists() {
            copy_dir(&self.base, &workspace)?;
        }
        Ok(workspace)
    }

    fn write_marker(
        &self,
        task_id: &str,
        result: serde_json::Value,
        candidate: Option<CandidateEvidence>,
    ) -> Result<MarkerArtifact, EngineeringError> {
        let marker_path = self.markers.join(format!("{task_id}.json"));
        let payload = serde_json::json!({
            "task_id": task_id,
            "result": result,
            "candidate": candidate,
        });
        let bytes = serde_json::to_vec_pretty(&payload)?;
        atomic_write(&marker_path, &bytes)?;
        let marker_sha256 = sha256_bytes(&bytes);
        Ok(MarkerArtifact {
            marker_path: marker_path.display().to_string(),
            marker_sha256,
            result,
            candidate,
        })
    }

    fn git(&self, cwd: &Path, arguments: &[&str]) -> Result<(), EngineeringError> {
        let output = git_command(cwd, arguments)
            .output()
            .map_err(|error| EngineeringError::Command(error.to_string()))?;
        if !output.status.success() {
            return Err(EngineeringError::Command(format!(
                "git {:?}: {}",
                arguments,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(())
    }

    fn git_commit(&self, cwd: &Path, message: &str) -> Result<(), EngineeringError> {
        self.git(cwd, &["commit", "-qm", message])
    }

    fn git_output(&self, cwd: &Path, arguments: &[&str]) -> Result<String, EngineeringError> {
        let output = git_command(cwd, arguments)
            .output()
            .map_err(|error| EngineeringError::Command(error.to_string()))?;
        if !output.status.success() {
            return Err(EngineeringError::Command(format!(
                "git {:?}: {}",
                arguments,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
}

fn git_command(cwd: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(cwd)
        .args(arguments)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_AUTHOR_NAME", "Factory Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_NAME", "Factory Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z");
    command
}

async fn run_command(
    cwd: &Path,
    argv: &[&str],
    sandbox: &SandboxMode,
) -> Result<CommandEvidence, EngineeringError> {
    let mut command = match sandbox {
        SandboxMode::LocalProcess => {
            let mut command = TokioCommand::new(argv[0]);
            command
                .args(&argv[1..])
                .current_dir(cwd)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("PYTHONDONTWRITEBYTECODE", "1")
                .env("HOME", cwd);
            command
        }
        SandboxMode::Podman { image } => {
            let mut command = TokioCommand::new("podman");
            command
                .args([
                    "run",
                    "--rm",
                    "--network=none",
                    "--read-only",
                    "--cap-drop=ALL",
                    "--security-opt=no-new-privileges",
                    "--pids-limit=128",
                    "--memory=512m",
                    "--cpus=1",
                    "--userns=keep-id",
                    "--tmpfs",
                    "/tmp:rw,noexec,nosuid,size=64m",
                    "-e",
                    "HOME=/tmp",
                    "-e",
                    "PYTHONDONTWRITEBYTECODE=1",
                    "--volume",
                ])
                .arg(format!("{}:/workspace:Z", cwd.display()))
                .args(["--workdir", "/workspace", "--entrypoint", argv[0], image])
                .args(&argv[1..])
                .env("PATH", "/usr/bin:/bin");
            command
        }
    };
    command.kill_on_drop(true);
    let result = tokio::time::timeout(COMMAND_TIMEOUT, command.output()).await;
    let (timed_out, output) = match result {
        Ok(Ok(output)) => (false, output),
        Ok(Err(error)) => {
            return Err(EngineeringError::Command(format!("{argv:?}: {error}")));
        }
        Err(_) => {
            return Ok(CommandEvidence {
                command: argv.iter().map(|value| (*value).to_owned()).collect(),
                exit_code: -1,
                output_sha256: sha256_bytes(b"timeout"),
                output: "command timed out".to_owned(),
                timed_out: true,
            });
        }
    };
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        text.push_str("\n[stderr]\n");
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    let text = normalize_output(&text);
    let text = truncate(&text, OUTPUT_LIMIT);
    Ok(CommandEvidence {
        command: argv.iter().map(|value| (*value).to_owned()).collect(),
        exit_code: output.status.code().unwrap_or(-1),
        output_sha256: sha256_bytes(text.as_bytes()),
        output: text,
        timed_out,
    })
}

fn normalize_output(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.starts_with("Ran ") && line.contains(" in ") {
                let count = line.split_whitespace().nth(1).unwrap_or("?");
                format!("Ran {count} tests")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...[truncated]", &text[..end])
}

fn candidate_manifest(
    workspace: &Path,
    git_revision: &str,
    tests: &CommandEvidence,
) -> Result<serde_json::Value, EngineeringError> {
    let mut files = Vec::new();
    for entry in walk_files(workspace)? {
        if entry.starts_with(".git") {
            continue;
        }
        let bytes = std::fs::read(workspace.join(&entry))?;
        files.push(serde_json::json!({
            "path": entry,
            "sha256": sha256_bytes(&bytes),
            "bytes": bytes.len(),
        }));
    }
    files.sort_by(|left, right| {
        left.get("path")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .cmp(
                right
                    .get("path")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default(),
            )
    });
    let mut manifest = serde_json::json!({
        "git_revision": git_revision,
        "files": files,
        "tests": tests,
    });
    let digest = sha256_bytes(&serde_json::to_vec(&manifest)?);
    manifest
        .as_object_mut()
        .expect("bug: manifest object")
        .insert(
            "candidate_sha256".to_owned(),
            serde_json::Value::String(digest),
        );
    Ok(manifest)
}

fn file_hashes(
    workspace: &Path,
    paths: &[&str],
) -> Result<Vec<serde_json::Value>, EngineeringError> {
    paths
        .iter()
        .map(|path| {
            let bytes = std::fs::read(workspace.join(path))?;
            Ok(serde_json::json!({
                "path": path,
                "sha256": sha256_bytes(&bytes),
                "bytes": bytes.len(),
            }))
        })
        .collect()
}

fn walk_files(root: &Path) -> Result<Vec<String>, EngineeringError> {
    fn visit(root: &Path, current: &Path, files: &mut Vec<String>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("bug: walked outside root")
                .to_string_lossy()
                .replace('\\', "/");
            if relative == ".git" || relative.starts_with(".git/") {
                continue;
            }
            if entry.file_type()?.is_dir() {
                visit(root, &path, files)?;
            } else {
                files.push(relative);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn copy_one(source: &Path, target: &Path) -> Result<(), EngineeringError> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(source, target)?;
    Ok(())
}

fn copy_dir(source: &Path, target: &Path) -> Result<(), EngineeringError> {
    fn visit(source: &Path, target: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(target)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            let from = entry.path();
            let to = target.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                visit(&from, &to)?;
            } else {
                std::fs::copy(&from, &to)?;
            }
        }
        Ok(())
    }
    visit(source, target)?;
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), EngineeringError> {
    let parent = path
        .parent()
        .ok_or_else(|| EngineeringError::Invalid("path has no parent".to_owned()))?;
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("write"),
        uuid::Uuid::new_v4()
    ));
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&temp, path)?;
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn read_marker_result(marker_path: &str) -> Result<serde_json::Value, EngineeringError> {
    let bytes = std::fs::read(marker_path)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    Ok(value)
}

const CALCULATOR_STUB: &str = r#"def add(a, b):
    return a + b


def multiply(a, b):
    raise NotImplementedError("multiply is not implemented")
"#;

const CALCULATOR_IMPLEMENTED: &str = r#"def add(a, b):
    return a + b


def multiply(a, b):
    return a * b
"#;

const BASE_TESTS: &str = r#"import unittest

from calculator import add


class TestCalculator(unittest.TestCase):
    def test_add(self):
        self.assertEqual(add(2, 3), 5)


if __name__ == "__main__":
    unittest.main()
"#;

const MULTIPLY_TESTS: &str = r#"import unittest

from calculator import add, multiply


class TestCalculator(unittest.TestCase):
    def test_add(self):
        self.assertEqual(add(2, 3), 5)

    def test_multiply(self):
        self.assertEqual(multiply(3, 4), 12)

    def test_multiply_zero(self):
        self.assertEqual(multiply(0, 99), 0)


if __name__ == "__main__":
    unittest.main()
"#;
