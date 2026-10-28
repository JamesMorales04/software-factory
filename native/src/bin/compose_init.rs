//! Idempotent first-run material generation for the Compose deployment.
//!
//! Role secrets are split across per-service configuration volumes. The
//! runtime-broker credential is intentionally shared by the execution roles
//! that call the broker (Worker, Supervisor, verifier and runtime); other role
//! credentials and the Core master key are not shared. Copies of shared secrets
//! are generated once and verified equal. Existing material is never
//! overwritten and no secret is printed. Files are owned by the unprivileged
//! service UID and restricted to mode 0640.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

const DEFAULT_UID: u32 = 10001;

const CORE_SECRETS: &[&str] = &[
    "operator.token",
    "worker-bootstrap.token",
    "verifier.token",
    "reviewer.token",
    "attempt-grant.key",
    "master.key",
];
const WORKER_SECRETS: &[&str] = &[
    "worker-bootstrap.token",
    "worker-registration.key",
    "supervisor.token",
    "runtime-broker.token",
];
const SUPERVISOR_SECRETS: &[&str] = &[
    "supervisor.token",
    "attempt-grant.key",
    "supervisor-capability.key",
    "runtime-broker.token",
];
const VERIFIER_SECRETS: &[&str] = &["verifier.token", "runtime-broker.token"];
const REVIEWER_SECRETS: &[&str] = &["reviewer.token"];
const RUNTIME_SECRETS: &[&str] = &["runtime-broker.token"];

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
            directory: config_dir("FACTORY_CONFIG_REVIEWER_DIR", "/config-reviewer"),
            secrets: REVIEWER_SECRETS,
        },
        Placement {
            directory: config_dir("FACTORY_CONFIG_RUNTIME_DIR", "/config-runtime"),
            secrets: RUNTIME_SECRETS,
        },
    ];
    for placement in &placements {
        prepare_directory(&placement.directory, uid)?;
        let _ = fs::remove_file(placement.directory.join(".ready"));
    }
    for directory in [&data, &projects, &provider_sessions] {
        prepare_directory(directory, uid)?;
    }

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
