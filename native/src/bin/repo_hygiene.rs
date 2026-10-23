use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Audit,
    Strict,
}

#[derive(Debug, Eq, PartialEq)]
struct Finding {
    path: String,
    rule: &'static str,
}

fn main() -> ExitCode {
    match run(env::args().skip(1)) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("repo-hygiene: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(args: impl IntoIterator<Item = String>) -> Result<ExitCode, String> {
    let mut mode = None;
    let mut root = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => {
                let value = args.next().ok_or("--mode requires audit or strict")?;
                mode = Some(match value.as_str() {
                    "audit" => Mode::Audit,
                    "strict" => Mode::Strict,
                    _ => return Err("--mode requires audit or strict".into()),
                });
            }
            "--root" => {
                root = Some(PathBuf::from(args.next().ok_or("--root requires a path")?));
            }
            "-h" | "--help" => {
                println!("usage: factory-repo-hygiene --mode <audit|strict> [--root PATH]");
                return Ok(ExitCode::SUCCESS);
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    let mode = mode.ok_or("--mode is required")?;
    let root = root.unwrap_or(env::current_dir().map_err(|e| e.to_string())?);
    let root = root
        .canonicalize()
        .map_err(|e| format!("cannot resolve repository root: {e}"))?;
    let findings = scan(&root)?;
    if findings.is_empty() {
        println!("repository hygiene: PASS (0 findings)");
        return Ok(ExitCode::SUCCESS);
    }

    for finding in &findings {
        println!("{}: {}", finding.path, finding.rule);
    }
    println!("repository hygiene: {} finding(s)", findings.len());
    Ok(exit_status(mode, findings.len()))
}

fn exit_status(mode: Mode, finding_count: usize) -> ExitCode {
    if finding_count > 0 && mode == Mode::Strict {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn scan(root: &Path) -> Result<Vec<Finding>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .map_err(|e| format!("cannot run git ls-files: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let mut findings = Vec::new();
    for entry in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|s| !s.is_empty())
    {
        let relative = path_from_git_bytes(entry);
        let path = relative.to_string_lossy().replace('\\', "/");
        let absolute = root.join(&relative);
        let Some(name) = relative.file_name().and_then(OsStr::to_str) else {
            continue;
        };

        if is_go_path(name, &relative) {
            findings.push(Finding {
                path: path.clone(),
                rule: "Go source, module, or lint configuration",
            });
        }

        if is_runtime_path(&relative) {
            findings.push(Finding {
                path: path.clone(),
                rule: "former runtime adapter or runtime-named active path",
            });
        }

        let bytes = fs::read(&absolute)
            .map_err(|error| format!("cannot read tracked file {path}: {error}"))?;
        let text = String::from_utf8_lossy(&bytes);
        // This exact file contains diagnostic vocabulary and fixtures; its path remains checked.
        if !is_hygiene_scanner(&relative)
            && !is_immutable_reference(&relative)
            && contains_go_invocation(&text)
        {
            findings.push(Finding {
                path: path.clone(),
                rule: "Go toolchain invocation or dependency",
            });
        }

        if is_workflow(&relative) && contains_go_workflow_step(&text) {
            findings.push(Finding {
                path: path.clone(),
                rule: "GitHub workflow depends on Go",
            });
        }

        // Exempt only immutable provenance artifacts; all other active Markdown is scanned.
        if !is_hygiene_scanner(&relative)
            && !is_immutable_reference(&relative)
            && (is_active_implementation(&relative) || is_markdown(&relative))
            && contains_former_runtime_name(&text)
        {
            findings.push(Finding {
                path,
                rule: "former runtime name or integration in active code or guidance",
            });
        }
    }
    findings.sort_by(|a, b| (&a.path, a.rule).cmp(&(&b.path, b.rule)));
    findings.dedup();
    Ok(findings)
}

fn is_go_path(name: &str, path: &Path) -> bool {
    name.ends_with(".go")
        || matches!(name, "go.mod" | "go.sum" | "go.work" | "go.work.sum")
        || name.starts_with(".golangci")
        || name.starts_with("golangci")
        || path.extension().is_some_and(|ext| ext == "go")
}

fn is_runtime_path(path: &Path) -> bool {
    path.components().any(|component| {
        let segment = component.as_os_str().to_string_lossy().to_ascii_lowercase();
        segment.contains("paseo") || segment == "aeh" || segment.contains("agent-engine")
    })
}

fn is_workflow(path: &Path) -> bool {
    path.starts_with(".github/workflows")
}

fn is_hygiene_scanner(path: &Path) -> bool {
    path == Path::new("native/src/bin/repo_hygiene.rs")
}

fn is_markdown(path: &Path) -> bool {
    matches!(path.extension().and_then(OsStr::to_str), Some("md" | "mdx"))
}

fn is_immutable_reference(path: &Path) -> bool {
    let value = path.to_string_lossy();
    path.starts_with("docs/evidence")
        || matches!(
            value.as_ref(),
            "docs/implementation/native-rust-handoff.md"
                | "docs/implementation/native-rust-migration-matrix.md"
        )
        || is_accepted_historical_adr(path)
}

fn is_accepted_historical_adr(path: &Path) -> bool {
    if !path.starts_with("docs/architecture/decisions") {
        return false;
    }
    let Some(name) = path.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    const ACCEPTED_IDS: &[&str] = &[
        "ADR-0001", "ADR-0002", "ADR-0003", "ADR-0004", "ADR-0005", "ADR-0006", "ADR-0007",
        "ADR-0008", "ADR-0009", "ADR-0010", "ADR-0011", "ADR-0012", "ADR-0013", "ADR-0014",
        "ADR-0015", "ADR-0016", "ADR-0017", "ADR-0018",
    ];
    ACCEPTED_IDS
        .iter()
        .any(|id| name.starts_with(&format!("{id}-")) && name.ends_with(".md"))
}

fn is_active_implementation(path: &Path) -> bool {
    let extension = path.extension().and_then(OsStr::to_str).unwrap_or("");
    matches!(
        extension,
        "rs" | "go"
            | "ts"
            | "tsx"
            | "js"
            | "mjs"
            | "cjs"
            | "py"
            | "sh"
            | "yml"
            | "yaml"
            | "toml"
            | "json"
    )
}

fn contains_go_invocation(contents: &str) -> bool {
    contents.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        [
            "go build",
            "go test",
            "go run",
            "go vet",
            "go tool",
            "go install",
            "go get",
            "go mod",
            "gofmt -",
            "golangci-lint",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
    })
}

fn contains_go_workflow_step(contents: &str) -> bool {
    contains_go_invocation(contents)
        || contents.lines().any(|line| {
            let lower = line.to_ascii_lowercase();
            lower.contains("actions/setup-go") || lower.contains("go-version")
        })
}

fn contains_former_runtime_name(contents: &str) -> bool {
    contents.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.contains("paseo")
            || lower.contains("agent-engineering-harness")
            || lower.contains("agent engineering harness")
            || lower
                .split(|character: char| !character.is_ascii_alphanumeric())
                .any(|word| word == "aeh")
    })
}

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

#[cfg(unix)]
fn path_from_git_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn path_from_git_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Repo(PathBuf);

    impl Repo {
        fn new() -> Self {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!("factory-hygiene-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            let result = Command::new("git")
                .arg("init")
                .arg("--quiet")
                .arg(&path)
                .status()
                .unwrap();
            assert!(result.success());
            Self(path)
        }

        fn add(&self, path: &str, contents: &str) {
            let target = self.0.join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, contents).unwrap();
            let result = Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(["add", "--", path])
                .status()
                .unwrap();
            assert!(result.success());
        }

        fn scan(&self) -> Vec<Finding> {
            super::scan(&self.0).unwrap()
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn strict_inventory_finds_sources_modules_invocations_adapters_and_workflows() {
        let repo = Repo::new();
        repo.add("core/main.go", "package main\n");
        repo.add("core/go.mod", "module example.invalid/old\n");
        repo.add("scripts/build.sh", "go build ./...\n");
        repo.add("core/internal/paseo/adapter.go", "package legacy\n");
        repo.add(
            ".github/workflows/legacy.yml",
            "- uses: actions/setup-go@v5\n",
        );

        let findings = repo.scan();
        let paths: Vec<&str> = findings
            .iter()
            .map(|finding| finding.path.as_str())
            .collect();
        assert!(paths.contains(&"core/main.go"));
        assert!(paths.contains(&"core/go.mod"));
        assert!(paths.contains(&"scripts/build.sh"));
        assert!(paths.contains(&"core/internal/paseo/adapter.go"));
        assert!(paths.contains(&".github/workflows/legacy.yml"));
    }

    #[test]
    fn audit_mode_can_report_legacy_tree_without_failing_and_strict_mode_fails() {
        let repo = Repo::new();
        repo.add("worker.go", "package worker\n");
        let findings = repo.scan();
        assert!(!findings.is_empty());
        let audit = run([
            "--mode".into(),
            "audit".into(),
            "--root".into(),
            repo.0.to_string_lossy().into_owned(),
        ])
        .unwrap();
        let strict = run([
            "--mode".into(),
            "strict".into(),
            "--root".into(),
            repo.0.to_string_lossy().into_owned(),
        ])
        .unwrap();
        assert_eq!(audit, ExitCode::SUCCESS);
        assert_eq!(strict, ExitCode::FAILURE);
    }

    #[test]
    fn ignores_untracked_files_and_accepts_clean_tracked_tree() {
        let repo = Repo::new();
        repo.add("src/lib.rs", "pub fn answer() -> u8 { 42 }\n");
        fs::write(repo.0.join("untracked.go"), "package old\n").unwrap();
        assert!(repo.scan().is_empty());
    }

    #[test]
    fn active_guidance_is_scanned_but_immutable_provenance_is_preserved() {
        let repo = Repo::new();
        repo.add("README.md", "Do not invoke Paseo from the application.\n");
        repo.add("docs/evidence/run.json", "{\"runtime\":\"paseo\"}");
        repo.add(
            "docs/implementation/native-rust-handoff.md",
            "Historical discussion of Paseo.\n",
        );
        repo.add(
            "docs/implementation/native-rust-migration-matrix.md",
            "Migration of Paseo behavior.\n",
        );
        repo.add(
            "docs/architecture/decisions/ADR-0005-go-core-worker-language.md",
            "The old Go decision is retained.\n",
        );
        repo.add(
            "docs/architecture/decisions/ADR-0019-future.md",
            "This current decision discusses Paseo.\n",
        );
        let findings = repo.scan();
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].path, "README.md");
        assert_eq!(
            findings[1].path,
            "docs/architecture/decisions/ADR-0019-future.md"
        );
    }

    #[test]
    fn former_runtime_name_in_active_code_is_reported() {
        let repo = Repo::new();
        repo.add("native/src/lib.rs", "const RUNTIME: &str = \"paseo\";\n");
        repo.add("native/src/provider.rs", "// agent-engineering-harness\n");
        assert!(
            repo.scan()
                .iter()
                .any(|finding| finding.path == "native/src/lib.rs")
        );
        assert!(
            repo.scan()
                .iter()
                .any(|finding| finding.path == "native/src/provider.rs")
        );
    }

    #[test]
    fn fails_closed_when_tracked_file_cannot_be_read() {
        let repo = Repo::new();
        repo.add("src/file.rs", "safe\n");
        fs::remove_file(repo.0.join("src/file.rs")).unwrap();
        let error = super::scan(&repo.0).unwrap_err();
        assert!(error.contains("src/file.rs"));
    }

    #[test]
    fn staged_deletion_is_absent_from_tracked_inventory() {
        let repo = Repo::new();
        repo.add("src/file.rs", "safe\n");
        let result = Command::new("git")
            .arg("-C")
            .arg(&repo.0)
            .args(["rm", "-f", "--", "src/file.rs"])
            .status()
            .unwrap();
        assert!(result.success());
        assert!(repo.scan().is_empty());
    }
}
