//! Opt-in live registry acquisition test.
//!
//! This test performs a real crates.io fetch and is skipped unless
//! `FACTORY_LIVE_DEPENDENCY_FETCH=1` is set. It does not use paid inference.
//! A host/container Cargo toolchain and network access are required.

use std::{path::Path, process::Command};

use sha2::{Digest, Sha256};

fn hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn toolchain() -> Option<(String, String)> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let cargo = std::fs::canonicalize(&cargo).ok()?;
    if !cargo.is_file() {
        return None;
    }
    let rustup_home = std::env::var("RUSTUP_HOME").unwrap_or_else(|_| {
        format!(
            "{}/.rustup",
            std::env::var("HOME").unwrap_or_else(|_| "/root".into())
        )
    });
    if !Path::new(&rustup_home).is_dir() {
        return None;
    }
    Some((cargo.to_string_lossy().into_owned(), rustup_home))
}

#[test]
fn live_registry_acquisition_is_explicit_and_optional() {
    if std::env::var("FACTORY_LIVE_DEPENDENCY_FETCH").as_deref() != Ok("1") {
        eprintln!(
            "SKIPPED live registry acquisition: set FACTORY_LIVE_DEPENDENCY_FETCH=1 to opt in"
        );
        return;
    }
    let Some((cargo, rustup_home)) = toolchain() else {
        eprintln!("SKIPPED live registry acquisition: Cargo or RUSTUP_HOME is unavailable");
        return;
    };
    let root =
        std::env::temp_dir().join(format!("factory-live-dependency-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"factory-live-dependency-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nitoa = \"=1.0.17\"\n",
    )
    .unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    let generated = Command::new(&cargo)
        .args(["generate-lockfile"])
        .current_dir(&root)
        .output()
        .expect("cargo generate-lockfile");
    if !generated.status.success() {
        eprintln!(
            "SKIPPED live registry acquisition: cargo generate-lockfile failed: {}",
            String::from_utf8_lossy(&generated.stderr)
                .chars()
                .take(500)
                .collect::<String>()
        );
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    let lockfile = std::fs::read(root.join("Cargo.lock")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_factory-dependency-acquire"))
        .args([
            "--root",
            root.to_str().unwrap(),
            "--workdir",
            ".",
            "--lockfile",
            "Cargo.lock",
            "--lockfile-sha256",
            &hex(&lockfile),
            "--vendor",
            "vendor",
            "--registry",
            "sparse+https://index.crates.io/",
        ])
        .env("FACTORY_DEPENDENCY_CARGO", &cargo)
        .env("FACTORY_DEPENDENCY_RUSTUP_HOME", &rustup_home)
        .env(
            "FACTORY_DEPENDENCY_IMAGE",
            "localhost/software-factory-dependency:dev",
        )
        .output()
        .expect("dependency acquisition binary");
    assert!(
        output.status.success(),
        "live acquisition failed: {}",
        String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(2_000)
            .collect::<String>()
    );
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["lockfileSha256"], hex(&lockfile));
    assert!(receipt["vendorSha256"].as_str().unwrap().len() == 64);
    for file in ["sbom.cdx.json", "licenses.json", "advisories.json"] {
        assert!(
            root.join("vendor")
                .join(".factory-supply-chain")
                .join(file)
                .is_file(),
            "missing evidence {file}"
        );
    }
    assert!(
        std::fs::read_dir(root.join("vendor"))
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name == "itoa" || name.starts_with("itoa-")
            })
    );
    let _ = std::fs::remove_dir_all(root);
}
