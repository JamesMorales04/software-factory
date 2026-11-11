//! Idempotent first-run material generation for the Compose deployment.
//!
//! Role secrets are split across per-service configuration volumes. Each
//! runtime-broker caller receives its own owner token: Supervisor, verifier and
//! Worker/provider. The runtime broker receives all three owner tokens so it can
//! derive the caller identity from the authenticated credential. Other role
//! credentials and the Core master key are not shared. Existing material is
//! never overwritten and no secret is printed. Files are owned by the
//! unprivileged service UID and restricted to mode 0640.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

const DEFAULT_UID: u32 = 10001;

/// One independent reviewer process identity per required role. Tokens are
/// generated per identity and never shared across roles.
const REVIEWER_IDENTITIES: &[(&str, &str, &str, &str)] = &[
    (
        "reviewer-functional",
        "reviewer-functional.token",
        "reviewer-functional-process",
        "FUNCTIONAL",
    ),
    (
        "reviewer-architecture",
        "reviewer-architecture.token",
        "reviewer-architecture-process",
        "ARCHITECTURE",
    ),
    (
        "reviewer-code-quality",
        "reviewer-code-quality.token",
        "reviewer-code-quality-process",
        "CODE_QUALITY",
    ),
    (
        "reviewer-security",
        "reviewer-security.token",
        "reviewer-security-process",
        "SECURITY",
    ),
    (
        "reviewer-testing",
        "reviewer-testing.token",
        "reviewer-testing-process",
        "TESTING",
    ),
    (
        "reviewer-operability",
        "reviewer-operability.token",
        "reviewer-operability-process",
        "OPERABILITY",
    ),
];

const CORE_SECRETS: &[&str] = &[
    "operator.token",
    "worker-bootstrap.token",
    "verifier.token",
    "reviewer-functional.token",
    "reviewer-architecture.token",
    "reviewer-code-quality.token",
    "reviewer-security.token",
    "reviewer-testing.token",
    "reviewer-operability.token",
    "attempt-grant.key",
    "master.key",
    "runtime-broker-environment.token",
];
const WORKER_SECRETS: &[&str] = &[
    "worker-bootstrap.token",
    "worker-registration.key",
    "supervisor.token",
    "runtime-broker-provider.token",
];
const SUPERVISOR_SECRETS: &[&str] = &[
    "supervisor.token",
    "attempt-grant.key",
    "supervisor-capability.key",
    "runtime-broker-supervisor.token",
];
const VERIFIER_SECRETS: &[&str] = &["verifier.token", "runtime-broker-verifier.token"];
const REVIEWER_FUNCTIONAL_SECRETS: &[&str] = &["reviewer-functional.token"];
const REVIEWER_ARCHITECTURE_SECRETS: &[&str] = &["reviewer-architecture.token"];
const REVIEWER_CODE_QUALITY_SECRETS: &[&str] = &["reviewer-code-quality.token"];
const REVIEWER_SECURITY_SECRETS: &[&str] = &["reviewer-security.token"];
const REVIEWER_TESTING_SECRETS: &[&str] = &["reviewer-testing.token"];
const REVIEWER_OPERABILITY_SECRETS: &[&str] = &["reviewer-operability.token"];
const RUNTIME_SECRETS: &[&str] = &[
    "runtime-broker-supervisor.token",
    "runtime-broker-verifier.token",
    "runtime-broker-provider.token",
    "runtime-broker-environment.token",
];
// The profile-gated OpenSpec validation job reads the latest durable
// projection through the operator-authenticated Core endpoint. It receives only
// the operator token, not Core's master key or the other role credentials.
const OPENSPEC_SECRETS: &[&str] = &["operator.token"];

struct Placement {
    directory: PathBuf,
    secrets: &'static [&'static str],
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data = PathBuf::from(std::env::var("FACTORY_DATA_DIR").unwrap_or_else(|_| "/data".into()));
    let projects =
        PathBuf::from(std::env::var("FACTORY_PROJECTS_DIR").unwrap_or_else(|_| "/projects".into()));
    let provider_sessions = PathBuf::from(
        std::env::var("FACTORY_PROVIDER_SESSIONS_DIR")
            .unwrap_or_else(|_| "/provider-sessions".into()),
    );
    let codex_home = PathBuf::from(
        std::env::var("FACTORY_CODEX_HOME_DIR").unwrap_or_else(|_| "/codex-home".into()),
    );
    let office_socket = PathBuf::from(
        std::env::var("FACTORY_OFFICE_SOCKET_DIR").unwrap_or_else(|_| "/office-socket".into()),
    );
    let uid = std::env::var("FACTORY_CONFIG_UID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(DEFAULT_UID);
    let placements = vec![
        Placement {
            directory: config_dir("FACTORY_CONFIG_CORE_DIR", "/config-core"),
            secrets: CORE_SECRETS,
        },
        Placement {
            directory: config_dir("FACTORY_CONFIG_WORKER_DIR", "/config-worker"),
            secrets: WORKER_SECRETS,
        },
        Placement {
            directory: config_dir("FACTORY_CONFIG_SUPERVISOR_DIR", "/config-supervisor"),
            secrets: SUPERVISOR_SECRETS,
        },
        Placement {
            directory: config_dir("FACTORY_CONFIG_VERIFIER_DIR", "/config-verifier"),
            secrets: VERIFIER_SECRETS,
        },
        Placement {
            directory: config_dir(
                "FACTORY_CONFIG_REVIEWER_FUNCTIONAL_DIR",
                "/config-reviewer-functional",
            ),
            secrets: REVIEWER_FUNCTIONAL_SECRETS,
        },
        Placement {
            directory: config_dir(
                "FACTORY_CONFIG_REVIEWER_ARCHITECTURE_DIR",
                "/config-reviewer-architecture",
            ),
            secrets: REVIEWER_ARCHITECTURE_SECRETS,
        },
        Placement {
            directory: config_dir(
                "FACTORY_CONFIG_REVIEWER_CODE_QUALITY_DIR",
                "/config-reviewer-code-quality",
            ),
            secrets: REVIEWER_CODE_QUALITY_SECRETS,
        },
        Placement {
            directory: config_dir(
                "FACTORY_CONFIG_REVIEWER_SECURITY_DIR",
                "/config-reviewer-security",
            ),
            secrets: REVIEWER_SECURITY_SECRETS,
        },
        Placement {
            directory: config_dir(
                "FACTORY_CONFIG_REVIEWER_TESTING_DIR",
                "/config-reviewer-testing",
            ),
            secrets: REVIEWER_TESTING_SECRETS,
        },
        Placement {
            directory: config_dir(
                "FACTORY_CONFIG_REVIEWER_OPERABILITY_DIR",
                "/config-reviewer-operability",
            ),
            secrets: REVIEWER_OPERABILITY_SECRETS,
        },
        Placement {
            directory: config_dir("FACTORY_CONFIG_RUNTIME_DIR", "/config-runtime"),
            secrets: RUNTIME_SECRETS,
        },
        Placement {
            directory: config_dir("FACTORY_CONFIG_OPENSPEC_DIR", "/config-openspec"),
            secrets: OPENSPEC_SECRETS,
        },
    ];
    for placement in &placements {
        prepare_directory(&placement.directory, uid)?;
        let _ = fs::remove_file(placement.directory.join(".ready"));
        // Remove the retired shared credential; callers now use owner tokens.
        let _ = fs::remove_file(placement.directory.join("runtime-broker.token"));
    }
    for directory in [&data, &projects, &provider_sessions, &office_socket] {
        prepare_directory(directory, uid)?;
    }
    prepare_directory(&codex_home, uid)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&codex_home, fs::Permissions::from_mode(0o700))?;
    }
    // The dedicated Office socket volume is shared with the Tailscale
    // container, whose root is mapped outside Core's UID in rootless Podman.
    fs::set_permissions(&office_socket, fs::Permissions::from_mode(0o755))?;

    let mut names = BTreeSet::new();
    for placement in &placements {
        names.extend(placement.secrets.iter().copied());
    }
    let mut values: BTreeMap<&'static str, String> = BTreeMap::new();
    for name in names {
        let existing = existing_value(&placements, name)?;
        let value = match existing {
            Some(value) => normalize_existing(name, value)?,
            None => random_secret(name)?,
        };
        for placement in &placements {
            if !placement.secrets.contains(&name) {
                continue;
            }
            let path = placement.directory.join(name);
            if path.exists() {
                let current = read_secret(&path)?;
                if current != value {
                    return Err(format!(
                        "existing secret copies for {name} disagree; refusing to continue"
                    )
                    .into());
                }
                ensure_mode(&path)?;
            } else {
                write_secret(&path, &value, uid)?;
            }
        }
        values.insert(name, value);
    }

    let core_config_dir = PathBuf::from(
        std::env::var("FACTORY_CORE_CONFIG_DIR").unwrap_or_else(|_| "/config".into()),
    );
    if !core_config_dir.is_absolute() {
        return Err("FACTORY_CORE_CONFIG_DIR must be absolute".into());
    }
    let identities = REVIEWER_IDENTITIES
        .iter()
        .map(|(_, token, principal, role)| {
            serde_json::json!({
                "principalId": principal,
                "role": role,
                "tokenFile": core_config_dir.join(token).to_string_lossy(),
            })
        })
        .collect::<Vec<_>>();
    let identities_json = serde_json::to_string_pretty(&identities)?;
    let identities_path =
        config_dir("FACTORY_CONFIG_CORE_DIR", "/config-core").join("reviewer-identities.json");
    write_config(&identities_path, &identities_json, uid)?;

    for placement in &placements {
        write_secret(&placement.directory.join(".ready"), "ready", uid)?;
    }
    println!("Factory role configuration material is ready; no secrets were printed.");
    Ok(())
}

fn config_dir(variable: &str, default: &str) -> PathBuf {
    PathBuf::from(std::env::var(variable).unwrap_or_else(|_| default.into()))
}

fn prepare_directory(path: &Path, uid: u32) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o750))?;
    std::os::unix::fs::chown(path, Some(uid), Some(uid))?;
    Ok(())
}

fn existing_value(
    placements: &[Placement],
    name: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let mut values = BTreeSet::new();
    for placement in placements {
        if !placement.secrets.contains(&name) {
            continue;
        }
        let path = placement.directory.join(name);
        if path.exists() {
            values.insert(read_secret(&path)?);
        }
    }
    if values.len() > 1 {
        return Err(format!("existing secret copies for {name} disagree").into());
    }
    Ok(values.into_iter().next())
}

fn normalize_existing(name: &str, value: String) -> Result<String, Box<dyn std::error::Error>> {
    let value = value.trim().to_owned();
    if value.is_empty() || value.len() > 4096 || value.contains('\0') {
        return Err(format!("existing {name} is invalid").into());
    }
    if name == "master.key" && value.len() == 128 && value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(value[..64].to_owned());
    }
    Ok(value)
}

fn random_secret(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let bytes = match name {
        "master.key" => 32,
        _ if name.ends_with(".key") => 64,
        _ => 48,
    };
    random_hex(bytes)
}

fn read_secret(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let value = fs::read_to_string(path)?;
    Ok(value.trim_end_matches(['\r', '\n']).to_owned())
}

fn ensure_mode(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 4096 {
        return Err(format!("existing {} is not a valid secret file", path.display()).into());
    }
    if metadata.permissions().mode() & 0o777 != 0o640 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o640))?;
    }
    Ok(())
}

fn write_config(path: &Path, value: &str, uid: u32) -> Result<(), Box<dyn std::error::Error>> {
    write_secret(path, value, uid)
}

fn write_secret(path: &Path, value: &str, uid: u32) -> Result<(), Box<dyn std::error::Error>> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o640)
        .open(&temporary)?;
    file.write_all(value.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    std::os::unix::fs::chown(&temporary, Some(uid), Some(uid))?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn random_hex(bytes: usize) -> Result<String, Box<dyn std::error::Error>> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut buffer = vec![0u8; bytes];
    SystemRandom::new()
        .fill(&mut buffer)
        .map_err(|_| "system randomness unavailable")?;
    Ok(buffer.iter().map(|byte| format!("{byte:02x}")).collect())
}
