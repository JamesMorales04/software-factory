use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

pub(crate) const CORE_MODEL_ENV_FILE_VARIABLE: &str = "FACTORY_CORE_MODEL_ENV_FILE";
pub(crate) const PROVIDER_ENV_FILE_VARIABLE: &str = "FACTORY_PROVIDER_ENV_FILE";

const CORE_MODEL_KEYS: &[&str] = &[
    "FACTORY_MODEL_URL",
    "FACTORY_MODEL_TOKEN",
    "FACTORY_MODEL_PROVIDER",
    "FACTORY_MODEL_NAME",
    "FACTORY_MODEL_TIMEOUT_SECS",
    "FACTORY_MODEL_INPUT_MICROS_PER_MILLION",
    "FACTORY_MODEL_OUTPUT_MICROS_PER_MILLION",
    "FACTORY_MODEL_GLOBAL_MAX_INPUT_TOKENS",
    "FACTORY_MODEL_GLOBAL_MAX_OUTPUT_TOKENS",
    "FACTORY_MODEL_GLOBAL_MAX_COST_MICROS",
    "FACTORY_LEAD_MODEL_MAX_INPUT_TOKENS",
    "FACTORY_LEAD_MODEL_MAX_OUTPUT_TOKENS",
    "FACTORY_LEAD_MODEL_MAX_COST_MICROS",
    "FACTORY_REVIEW_MODEL_MAX_INPUT_TOKENS",
    "FACTORY_REVIEW_MODEL_MAX_OUTPUT_TOKENS",
    "FACTORY_REVIEW_MODEL_MAX_COST_MICROS",
    "FACTORY_EVALUATION_MODEL_MAX_INPUT_TOKENS",
    "FACTORY_EVALUATION_MODEL_MAX_OUTPUT_TOKENS",
    "FACTORY_EVALUATION_MODEL_MAX_COST_MICROS",
    "FACTORY_ENABLE_AUTONOMOUS_START",
    "FACTORY_ALLOW_TEST_UPSTREAM",
];

const PROVIDER_KEYS: &[&str] = &[
    "FACTORY_ALLOW_TEST_UPSTREAM",
    "FACTORY_ALLOW_UNCONFINED_PROVIDER",
    "FACTORY_PROVIDER_ARGS",
    "FACTORY_PROVIDER_BASE_URL",
    "FACTORY_PROVIDER_BINARY",
    "FACTORY_PROVIDER_BRIDGE",
    "FACTORY_PROVIDER_CODEX_INPUT_COST_MICROS_PER_MILLION",
    "FACTORY_PROVIDER_CODEX_OUTPUT_COST_MICROS_PER_MILLION",
    "FACTORY_PROVIDER_EGRESS_BRIDGE",
    "FACTORY_PROVIDER_EGRESS_TOKEN",
    "FACTORY_PROVIDER_KEEP_SESSION",
    "FACTORY_PROVIDER_KIND",
    "FACTORY_PROVIDER_MEMORY_LIMIT_BYTES",
    "FACTORY_PROVIDER_MODEL_CONTEXT",
    "FACTORY_PROVIDER_MODEL_OUTPUT",
    "FACTORY_PROVIDER_PASSWORD",
    "FACTORY_PROVIDER_SANDBOX",
    "FACTORY_PROVIDER_SESSION_MAX_BYTES",
    "FACTORY_PROVIDER_SESSION_ROOT",
    "FACTORY_PROVIDER_TCP_PORT",
    "FACTORY_PROVIDER_UPSTREAM_TOKEN",
    "FACTORY_PROVIDER_UPSTREAM_URL",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct EnvFile {
    path: PathBuf,
    values: BTreeMap<String, String>,
    fingerprint: String,
}

impl EnvFile {
    fn value(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    fn required(&self, key: &str) -> Result<&str, String> {
        self.value(key)
            .ok_or_else(|| format!("{} is missing {key}", self.path.display()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoreModelEnv {
    file: EnvFile,
}

impl CoreModelEnv {
    pub(crate) fn path(&self) -> &Path {
        &self.file.path
    }

    pub(crate) fn fingerprint(&self) -> &str {
        &self.file.fingerprint
    }

    pub(crate) fn summary(&self) -> String {
        let provider = self
            .file
            .value("FACTORY_MODEL_PROVIDER")
            .unwrap_or("opencode-go");
        let model = self
            .file
            .value("FACTORY_MODEL_NAME")
            .unwrap_or("deepseek-v4.1-flash");
        let endpoint = self
            .file
            .value("FACTORY_MODEL_URL")
            .map(endpoint_origin)
            .unwrap_or_else(|| "set".to_owned());
        let autonomous = self
            .file
            .value("FACTORY_ENABLE_AUTONOMOUS_START")
            .is_some_and(|value| value == "1");
        format!(
            "provider={provider}, model={model}, endpoint={endpoint}, token=set, autonomous_start={autonomous}"
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderEnv {
    file: EnvFile,
}

impl ProviderEnv {
    pub(crate) fn path(&self) -> &Path {
        &self.file.path
    }

    pub(crate) fn summary(&self) -> String {
        let kind = self.file.value("FACTORY_PROVIDER_KIND").unwrap_or("unset");
        let sandbox = self
            .file
            .value("FACTORY_PROVIDER_SANDBOX")
            .unwrap_or("unset");
        let upstream = self
            .file
            .value("FACTORY_PROVIDER_UPSTREAM_URL")
            .map(endpoint_origin)
            .or_else(|| {
                self.file
                    .value("FACTORY_PROVIDER_BASE_URL")
                    .map(endpoint_origin)
            })
            .unwrap_or_else(|| "unset".to_owned());
        let binary = if self.file.value("FACTORY_PROVIDER_BINARY").is_some() {
            "configured"
        } else {
            "unset"
        };
        let session_root = if self.file.value("FACTORY_PROVIDER_SESSION_ROOT").is_some() {
            "configured"
        } else {
            "unset"
        };
        format!(
            "kind={kind}, sandbox={sandbox}, upstream={upstream}, binary={binary}, session_root={session_root}"
        )
    }

    pub(crate) fn environment(&self) -> Vec<(String, String)> {
        self.file
            .values
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }
}

#[cfg(test)]
pub(crate) fn provider_for_test(values: &[(&str, &str)]) -> ProviderEnv {
    let values = values
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect::<BTreeMap<_, _>>();
    let fingerprint = fingerprint(&values);
    ProviderEnv {
        file: EnvFile {
            path: PathBuf::from("/test/provider.env"),
            values,
            fingerprint,
        },
    }
}

pub(crate) fn load_core_model() -> Result<Option<CoreModelEnv>, String> {
    let Some(path) = path_from_environment(CORE_MODEL_ENV_FILE_VARIABLE)? else {
        return Ok(None);
    };
    load_core_model_at(&path).map(Some)
}

pub(crate) fn load_provider() -> Result<Option<ProviderEnv>, String> {
    let Some(path) = path_from_environment(PROVIDER_ENV_FILE_VARIABLE)? else {
        return Ok(None);
    };
    load_provider_at(&path).map(Some)
}

fn path_from_environment(variable: &str) -> Result<Option<PathBuf>, String> {
    path_from_value(variable, env::var_os(variable))
}

fn path_from_value(variable: &str, value: Option<OsString>) -> Result<Option<PathBuf>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value
        .into_string()
        .map_err(|_| format!("{variable} is not valid UTF-8"))?;
    if value.is_empty() {
        return Err(format!("{variable} is empty"));
    }
    if value.chars().any(|character| character.is_control()) {
        return Err(format!("{variable} must not contain control characters"));
    }
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(format!("{variable} must be an absolute path"));
    }
    Ok(Some(path))
}

fn load_core_model_at(path: &Path) -> Result<CoreModelEnv, String> {
    let file = load_file(CORE_MODEL_ENV_FILE_VARIABLE, path, CORE_MODEL_KEYS)?;
    validate_core_model(&file)?;
    Ok(CoreModelEnv { file })
}

fn load_provider_at(path: &Path) -> Result<ProviderEnv, String> {
    let file = load_file(PROVIDER_ENV_FILE_VARIABLE, path, PROVIDER_KEYS)?;
    validate_provider(&file)?;
    Ok(ProviderEnv { file })
}

fn load_file(variable: &str, path: &Path, allowlist: &[&str]) -> Result<EnvFile, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {variable} file {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "{variable} file {} must not be a symbolic link",
            path.display()
        ));
    }
    if !metadata.is_file() {
        return Err(format!(
            "{variable} file {} is not a regular file",
            path.display()
        ));
    }
    if metadata.len() > 64 * 1024 {
        return Err(format!(
            "{variable} file {} exceeds the 64 KiB configuration limit",
            path.display()
        ));
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err(format!(
            "{variable} file {} has mode {mode:o}; it must be a private regular file with mode 600",
            path.display()
        ));
    }
    let content = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {variable} file {}: {error}", path.display()))?;
    let values = parse_content(variable, path, &content, allowlist)?;
    let fingerprint = fingerprint(&values);
    Ok(EnvFile {
        path: path.to_path_buf(),
        values,
        fingerprint,
    })
}

fn parse_content(
    variable: &str,
    path: &Path,
    content: &str,
    allowlist: &[&str],
) -> Result<BTreeMap<String, String>, String> {
    let mut values = BTreeMap::new();
    for (index, raw_line) in content.split('\n').enumerate() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| {
            format!(
                "{variable} file {} line {} is not KEY=VALUE",
                path.display(),
                index.saturating_add(1)
            )
        })?;
        let key = key.trim();
        let value = value.trim();
        if !valid_key(key) {
            return Err(format!(
                "{variable} file {} line {} contains an invalid key",
                path.display(),
                index.saturating_add(1)
            ));
        }
        if !allowlist.contains(&key) {
            return Err(format!(
                "{variable} file {} contains unsupported key {key}",
                path.display()
            ));
        }
        if value.is_empty() {
            return Err(format!(
                "{variable} file {} contains an empty value for {key}",
                path.display()
            ));
        }
        if value.chars().any(char::is_control) {
            return Err(format!(
                "{variable} file {} contains a control character in {key}",
                path.display()
            ));
        }
        if value.contains(" #") || value.contains('\t') {
            return Err(format!(
                "{variable} file {} contains an inline comment or tab in {key}",
                path.display()
            ));
        }
        if values.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err(format!(
                "{variable} file {} contains duplicate key {key}",
                path.display()
            ));
        }
    }
    Ok(values)
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        && !key.as_bytes()[0].is_ascii_digit()
}

fn fingerprint(values: &BTreeMap<String, String>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"factory-managed-env-v1\0");
    for (key, value) in values {
        hasher.update(key.as_bytes());
        hasher.update(b"=");
        hasher.update(value.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

fn validate_core_model(file: &EnvFile) -> Result<(), String> {
    let url = file.required("FACTORY_MODEL_URL")?;
    let token = file.required("FACTORY_MODEL_TOKEN")?;
    let allow_test = boolean(file, "FACTORY_ALLOW_TEST_UPSTREAM", false)?;
    validate_upstream_url("FACTORY_MODEL_URL", url, allow_test)?;
    if token.len() < 16 {
        return Err("FACTORY_MODEL_TOKEN must be at least 16 bytes".to_owned());
    }
    validate_secret_value("FACTORY_MODEL_TOKEN", token)?;
    for key in ["FACTORY_MODEL_PROVIDER", "FACTORY_MODEL_NAME"] {
        if let Some(value) = file.value(key) {
            validate_secret_value(key, value)?;
        }
    }
    for key in [
        "FACTORY_MODEL_TIMEOUT_SECS",
        "FACTORY_MODEL_INPUT_MICROS_PER_MILLION",
        "FACTORY_MODEL_OUTPUT_MICROS_PER_MILLION",
        "FACTORY_MODEL_GLOBAL_MAX_INPUT_TOKENS",
        "FACTORY_MODEL_GLOBAL_MAX_OUTPUT_TOKENS",
        "FACTORY_MODEL_GLOBAL_MAX_COST_MICROS",
        "FACTORY_LEAD_MODEL_MAX_INPUT_TOKENS",
        "FACTORY_LEAD_MODEL_MAX_OUTPUT_TOKENS",
        "FACTORY_LEAD_MODEL_MAX_COST_MICROS",
        "FACTORY_REVIEW_MODEL_MAX_INPUT_TOKENS",
        "FACTORY_REVIEW_MODEL_MAX_OUTPUT_TOKENS",
        "FACTORY_REVIEW_MODEL_MAX_COST_MICROS",
        "FACTORY_EVALUATION_MODEL_MAX_INPUT_TOKENS",
        "FACTORY_EVALUATION_MODEL_MAX_OUTPUT_TOKENS",
        "FACTORY_EVALUATION_MODEL_MAX_COST_MICROS",
    ] {
        if let Some(value) = file.value(key) {
            parse_u64(key, value)?;
        }
    }
    boolean(file, "FACTORY_ENABLE_AUTONOMOUS_START", false)?;
    Ok(())
}

fn validate_provider(file: &EnvFile) -> Result<(), String> {
    let kind = file.required("FACTORY_PROVIDER_KIND")?;
    if !matches!(kind, "fixture" | "opencode" | "codex") {
        return Err("FACTORY_PROVIDER_KIND must be fixture, opencode or codex".to_owned());
    }
    let sandbox = file.required("FACTORY_PROVIDER_SANDBOX")?;
    let allow_test = boolean(file, "FACTORY_ALLOW_TEST_UPSTREAM", false)?;
    let allow_unconfined = boolean(file, "FACTORY_ALLOW_UNCONFINED_PROVIDER", false)?;
    boolean(file, "FACTORY_PROVIDER_KEEP_SESSION", false)?;
    match sandbox {
        "bwrap" => {
            let binary = file.required("FACTORY_PROVIDER_BINARY")?;
            validate_absolute_file("FACTORY_PROVIDER_BINARY", binary)?;
            if let Some(bridge) = file.value("FACTORY_PROVIDER_BRIDGE") {
                validate_absolute_file("FACTORY_PROVIDER_BRIDGE", bridge)?;
            }
        }
        "unconfined" => {
            if !allow_unconfined {
                return Err(
                    "FACTORY_PROVIDER_SANDBOX=unconfined requires FACTORY_ALLOW_UNCONFINED_PROVIDER=1"
                        .to_owned(),
                );
            }
            let base_url = file.required("FACTORY_PROVIDER_BASE_URL")?;
            validate_loopback_origin("FACTORY_PROVIDER_BASE_URL", base_url)?;
            let password = file.required("FACTORY_PROVIDER_PASSWORD")?;
            if password.len() < 16 {
                return Err("FACTORY_PROVIDER_PASSWORD must be at least 16 bytes".to_owned());
            }
            validate_secret_value("FACTORY_PROVIDER_PASSWORD", password)?;
        }
        _ => return Err("FACTORY_PROVIDER_SANDBOX must be bwrap or unconfined".to_owned()),
    }
    if let Some(upstream) = file.value("FACTORY_PROVIDER_UPSTREAM_URL") {
        validate_upstream_url("FACTORY_PROVIDER_UPSTREAM_URL", upstream, allow_test)?;
        let upstream_token = file.required("FACTORY_PROVIDER_UPSTREAM_TOKEN")?;
        if upstream_token.len() < 16 {
            return Err("FACTORY_PROVIDER_UPSTREAM_TOKEN must be at least 16 bytes".to_owned());
        }
        validate_secret_value("FACTORY_PROVIDER_UPSTREAM_TOKEN", upstream_token)?;
        let egress_bridge = file.required("FACTORY_PROVIDER_EGRESS_BRIDGE")?;
        validate_absolute_file("FACTORY_PROVIDER_EGRESS_BRIDGE", egress_bridge)?;
    } else {
        if matches!(kind, "opencode" | "codex") && sandbox == "bwrap" {
            return Err(format!(
                "FACTORY_PROVIDER_UPSTREAM_URL is required for the {kind} provider route"
            ));
        }
        if file.value("FACTORY_PROVIDER_UPSTREAM_TOKEN").is_some() {
            return Err(
                "FACTORY_PROVIDER_UPSTREAM_TOKEN requires FACTORY_PROVIDER_UPSTREAM_URL".to_owned(),
            );
        }
    }
    if let Some(root) = file.value("FACTORY_PROVIDER_SESSION_ROOT") {
        validate_absolute_path("FACTORY_PROVIDER_SESSION_ROOT", root)?;
    }
    if let Some(value) = file.value("FACTORY_PROVIDER_EGRESS_TOKEN") {
        validate_secret_value("FACTORY_PROVIDER_EGRESS_TOKEN", value)?;
    }
    if let Some(value) = file.value("FACTORY_PROVIDER_PASSWORD") {
        if value.len() < 16 {
            return Err("FACTORY_PROVIDER_PASSWORD must be at least 16 bytes".to_owned());
        }
        validate_secret_value("FACTORY_PROVIDER_PASSWORD", value)?;
    }
    for key in [
        "FACTORY_PROVIDER_MEMORY_LIMIT_BYTES",
        "FACTORY_PROVIDER_MODEL_CONTEXT",
        "FACTORY_PROVIDER_MODEL_OUTPUT",
        "FACTORY_PROVIDER_SESSION_MAX_BYTES",
        "FACTORY_PROVIDER_CODEX_INPUT_COST_MICROS_PER_MILLION",
        "FACTORY_PROVIDER_CODEX_OUTPUT_COST_MICROS_PER_MILLION",
    ] {
        if let Some(value) = file.value(key) {
            let parsed = parse_u64(key, value)?;
            if parsed == 0 {
                return Err(format!("{key} must be greater than zero"));
            }
        }
    }
    if kind == "codex" && sandbox == "bwrap" {
        for key in [
            "FACTORY_PROVIDER_CODEX_INPUT_COST_MICROS_PER_MILLION",
            "FACTORY_PROVIDER_CODEX_OUTPUT_COST_MICROS_PER_MILLION",
        ] {
            file.required(key)?;
        }
    }
    if let Some(value) = file.value("FACTORY_PROVIDER_TCP_PORT") {
        let port = parse_u64("FACTORY_PROVIDER_TCP_PORT", value)?;
        if port == 0 || port > u16::MAX as u64 || port == 18080 {
            return Err(
                "FACTORY_PROVIDER_TCP_PORT must be a non-zero port other than 18080".to_owned(),
            );
        }
    }
    if let Some(value) = file.value("FACTORY_PROVIDER_ARGS") {
        let args: serde_json::Value = serde_json::from_str(value)
            .map_err(|_| "FACTORY_PROVIDER_ARGS must be a JSON string array".to_owned())?;
        if !args
            .as_array()
            .is_some_and(|items| items.iter().all(serde_json::Value::is_string))
        {
            return Err("FACTORY_PROVIDER_ARGS must be a JSON string array".to_owned());
        }
    }
    Ok(())
}

fn validate_upstream_url(key: &str, value: &str, allow_loopback_http: bool) -> Result<(), String> {
    if value
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(format!(
            "{key} must not contain whitespace or control characters"
        ));
    }
    let (scheme, rest) = value.split_once("://").ok_or_else(|| {
        format!("{key} must be an absolute https URL (http is allowed only for loopback tests)")
    })?;
    if scheme != "https" && scheme != "http" {
        return Err(format!("{key} must use https"));
    }
    if rest.is_empty() || rest.starts_with('/') {
        return Err(format!("{key} has no host"));
    }
    if value.contains('?') || value.contains('#') {
        return Err(format!("{key} must not contain a query or fragment"));
    }
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        return Err(format!("{key} has no host"));
    }
    if authority.contains('@') {
        return Err(format!("{key} must not contain user information"));
    }
    let (host, port) = authority_parts(key, authority)?;
    if scheme == "http" {
        if !allow_loopback_http {
            return Err(format!(
                "{key} uses plain HTTP; set FACTORY_ALLOW_TEST_UPSTREAM=1 only for loopback tests"
            ));
        }
        if !is_loopback_host(host) {
            return Err(format!(
                "{key} allows plain HTTP only for a loopback test upstream"
            ));
        }
    }
    if port == Some(0) {
        return Err(format!("{key} must not use port 0"));
    }
    Ok(())
}

fn validate_loopback_origin(key: &str, value: &str) -> Result<(), String> {
    let rest = value
        .strip_prefix("http://")
        .ok_or_else(|| format!("{key} must be a loopback HTTP origin with an explicit port"))?;
    if rest.is_empty() || rest.contains(['/', '?', '#']) || rest.contains('@') {
        return Err(format!(
            "{key} must be a loopback HTTP origin with an explicit port"
        ));
    }
    let (host, port) = authority_parts(key, rest)?;
    let Some(port) = port else {
        return Err(format!(
            "{key} must be a loopback HTTP origin with an explicit port"
        ));
    };
    if port == 0 || !matches!(host, "127.0.0.1" | "localhost") {
        return Err(format!(
            "{key} must be a loopback HTTP origin with an explicit port"
        ));
    }
    Ok(())
}

fn authority_parts<'a>(key: &str, authority: &'a str) -> Result<(&'a str, Option<u16>), String> {
    let (host, port) = if authority.starts_with('[') {
        let end = authority
            .find(']')
            .ok_or_else(|| format!("{key} has an invalid host"))?;
        let host = &authority[..=end];
        let remainder = &authority[end + 1..];
        if remainder.is_empty() {
            (host, None)
        } else if let Some(port) = remainder.strip_prefix(':') {
            (host, Some(port))
        } else {
            return Err(format!("{key} has an invalid host"));
        }
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if host.contains(':') {
            return Err(format!("{key} has an invalid host"));
        }
        (host, Some(port))
    } else {
        (authority, None)
    };
    if host.is_empty()
        || (host.starts_with('[') && !host.ends_with(']'))
        || (!host.starts_with('[')
            && host.chars().any(|character| {
                !character.is_ascii_alphanumeric() && !matches!(character, '.' | '-')
            }))
    {
        return Err(format!("{key} has an invalid host"));
    }
    let port = match port {
        Some(port) => Some(
            port.parse::<u16>()
                .map_err(|_| format!("{key} has an invalid port"))?,
        ),
        None => None,
    };
    Ok((host, port))
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
}

fn validate_absolute_file(key: &str, value: &str) -> Result<(), String> {
    let path = validate_absolute_path(key, value)?;
    if !path.is_file() {
        return Err(format!("{key} must name an absolute regular file"));
    }
    Ok(())
}

fn validate_absolute_path<'a>(key: &str, value: &'a str) -> Result<&'a Path, String> {
    if value.chars().any(|character| character.is_control()) {
        return Err(format!("{key} must not contain control characters"));
    }
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(format!("{key} must be an absolute path"));
    }
    Ok(path)
}

fn parse_u64(key: &str, value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{key} must be a nonnegative integer"))
}

fn boolean(file: &EnvFile, key: &str, default: bool) -> Result<bool, String> {
    match file.value(key) {
        None => Ok(default),
        Some("0") => Ok(false),
        Some("1") => Ok(true),
        Some(_) => Err(format!("{key} must be 0 or 1")),
    }
}

fn validate_secret_value(key: &str, value: &str) -> Result<(), String> {
    if value
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(format!(
            "{key} must not contain whitespace or control characters"
        ));
    }
    Ok(())
}

fn endpoint_origin(value: &str) -> String {
    let Some((scheme, rest)) = value.split_once("://") else {
        return "set".to_owned();
    };
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        "set".to_owned()
    } else {
        format!("{scheme}://{authority}")
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{
        CORE_MODEL_ENV_FILE_VARIABLE, load_core_model_at, load_provider_at, path_from_value,
    };

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "factory-cli-managed-env-test-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_private(path: &Path, content: &str) {
        fs::write(path, content).expect("write env file");
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("chmod env file");
    }

    fn write_executable(path: &Path) {
        fs::write(path, "#!/bin/sh\n").expect("write executable");
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("chmod executable");
    }

    fn core_content() -> String {
        [
            "FACTORY_MODEL_URL=https://provider.example/v1",
            "FACTORY_MODEL_TOKEN=super-secret-model-token",
            "FACTORY_MODEL_PROVIDER=opencode-go",
            "FACTORY_MODEL_NAME=deepseek-v4.1-flash",
            "FACTORY_MODEL_TIMEOUT_SECS=60",
            "FACTORY_LEAD_MODEL_MAX_COST_MICROS=300000",
            "FACTORY_ENABLE_AUTONOMOUS_START=1",
            "",
        ]
        .join("\n")
    }

    fn provider_content(temp: &TempDir) -> String {
        let binary = temp.path().join("opencode");
        let bridge = temp.path().join("provider-bridge");
        let egress = temp.path().join("egress-bridge");
        write_executable(&binary);
        write_executable(&bridge);
        write_executable(&egress);
        format!(
            "FACTORY_PROVIDER_SANDBOX=bwrap\n\
             FACTORY_PROVIDER_KIND=opencode\n\
             FACTORY_PROVIDER_BINARY={}\n\
             FACTORY_PROVIDER_BRIDGE={}\n\
             FACTORY_PROVIDER_EGRESS_BRIDGE={}\n\
             FACTORY_PROVIDER_UPSTREAM_URL=https://opencode.ai/inference/go/openai/v1\n\
             FACTORY_PROVIDER_UPSTREAM_TOKEN=super-secret-upstream-token\n\
             FACTORY_PROVIDER_SESSION_ROOT={}\n\
             FACTORY_PROVIDER_MEMORY_LIMIT_BYTES=17179869184\n",
            binary.display(),
            bridge.display(),
            egress.display(),
            temp.path().display()
        )
    }

    #[test]
    fn loads_a_valid_core_model_file_and_redacts_the_token() {
        let temp = TempDir::new();
        let path = temp.path().join("model.env");
        write_private(&path, &core_content());
        let config = load_core_model_at(&path).expect("load model env");
        let summary = config.summary();
        assert!(summary.contains("provider=opencode-go"));
        assert!(summary.contains("model=deepseek-v4.1-flash"));
        assert!(summary.contains("endpoint=https://provider.example"));
        assert!(summary.contains("token=set"));
        assert!(!summary.contains("super-secret-model-token"));
        assert_eq!(config.fingerprint().len(), 64);
    }

    #[test]
    fn core_model_file_requires_url_and_token() {
        let temp = TempDir::new();
        let path = temp.path().join("model.env");
        write_private(&path, "FACTORY_MODEL_URL=https://provider.example/v1\n");
        assert!(load_core_model_at(&path).is_err());
        write_private(&path, "FACTORY_MODEL_TOKEN=super-secret-model-token\n");
        assert!(load_core_model_at(&path).is_err());
    }

    #[test]
    fn core_model_file_accepts_loopback_http_only_with_the_test_switch() {
        let temp = TempDir::new();
        let path = temp.path().join("model.env");
        let base = [
            "FACTORY_MODEL_URL=http://127.0.0.1:9000/v1",
            "FACTORY_MODEL_TOKEN=deterministic-test-token-1234",
            "",
        ]
        .join("\n");
        write_private(&path, &base);
        assert!(load_core_model_at(&path).is_err());
        write_private(&path, &format!("{base}FACTORY_ALLOW_TEST_UPSTREAM=1\n"));
        assert!(load_core_model_at(&path).is_ok());
    }

    #[test]
    fn rejects_unknown_duplicate_and_embedded_secret_syntax() {
        let temp = TempDir::new();
        let path = temp.path().join("model.env");
        write_private(&path, &format!("{}FACTORY_UNKNOWN=1\n", core_content()));
        assert!(load_core_model_at(&path).is_err());
        write_private(
            &path,
            &format!("{}FACTORY_MODEL_NAME=other\n", core_content()),
        );
        assert!(load_core_model_at(&path).is_err());
        write_private(
            &path,
            &core_content().replace(
                "FACTORY_MODEL_URL=https://provider.example/v1",
                "FACTORY_MODEL_URL=https://provider.example/v1?token=secret",
            ),
        );
        assert!(load_core_model_at(&path).is_err());
        write_private(
            &path,
            &core_content().replace(
                "FACTORY_MODEL_URL=https://provider.example/v1",
                "FACTORY_MODEL_URL=https://user:secret@provider.example/v1",
            ),
        );
        assert!(load_core_model_at(&path).is_err());
        write_private(
            &path,
            &core_content().replace(
                "FACTORY_MODEL_TOKEN=super-secret-model-token",
                "FACTORY_MODEL_TOKEN=super-secret-model-token # inline comment",
            ),
        );
        assert!(load_core_model_at(&path).is_err());
    }

    #[test]
    fn rejects_symlinks_relative_paths_and_insecure_modes() {
        let temp = TempDir::new();
        let target = temp.path().join("target.env");
        write_private(&target, &core_content());
        let link = temp.path().join("link.env");
        std::os::unix::fs::symlink(&target, &link).expect("symlink");
        assert!(load_core_model_at(&link).is_err());

        assert!(path_from_value(CORE_MODEL_ENV_FILE_VARIABLE, Some("relative".into())).is_err());
        assert!(path_from_value(CORE_MODEL_ENV_FILE_VARIABLE, Some("".into())).is_err());
        assert_eq!(
            path_from_value(CORE_MODEL_ENV_FILE_VARIABLE, None).expect("unset path"),
            None
        );
        assert_eq!(
            path_from_value(CORE_MODEL_ENV_FILE_VARIABLE, Some("/tmp/model.env".into()))
                .expect("absolute path"),
            Some(PathBuf::from("/tmp/model.env"))
        );

        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).expect("chmod");
        assert!(load_core_model_at(&target).is_err());
    }

    #[test]
    fn fingerprint_is_canonical_and_content_sensitive() {
        let temp = TempDir::new();
        let first = temp.path().join("first.env");
        let second = temp.path().join("second.env");
        write_private(&first, &core_content());
        let reordered = core_content().lines().rev().collect::<Vec<_>>().join("\n");
        write_private(&second, &format!("{reordered}\n"));
        let first = load_core_model_at(&first).expect("first");
        let second = load_core_model_at(&second).expect("second");
        assert_eq!(first.fingerprint(), second.fingerprint());

        let changed = temp.path().join("changed.env");
        write_private(
            &changed,
            &core_content().replace("super-secret-model-token", "different-secret-token"),
        );
        let changed = load_core_model_at(&changed).expect("changed");
        assert_ne!(first.fingerprint(), changed.fingerprint());
    }

    #[test]
    fn loads_a_valid_provider_file_and_redacts_the_token() {
        let temp = TempDir::new();
        let path = temp.path().join("provider.env");
        write_private(&path, &provider_content(&temp));
        let config = load_provider_at(&path).expect("load provider env");
        let summary = config.summary();
        assert!(summary.contains("kind=opencode"));
        assert!(summary.contains("sandbox=bwrap"));
        assert!(summary.contains("upstream=https://opencode.ai"));
        assert!(!summary.contains("super-secret-upstream-token"));
        let environment = config.environment();
        assert!(environment.iter().any(|(key, value)| {
            key == "FACTORY_PROVIDER_UPSTREAM_TOKEN" && value == "super-secret-upstream-token"
        }));
        assert!(
            !environment
                .iter()
                .any(|(key, _)| key == "FACTORY_MODEL_TOKEN")
        );
    }

    #[test]
    fn provider_file_requires_the_explicit_unconfined_opt_in() {
        let temp = TempDir::new();
        let path = temp.path().join("provider.env");
        let content = "FACTORY_PROVIDER_SANDBOX=unconfined\n\
                       FACTORY_PROVIDER_KIND=opencode\n\
                       FACTORY_PROVIDER_BASE_URL=http://127.0.0.1:4123\n\
                       FACTORY_PROVIDER_PASSWORD=unconfined-password-1234\n";
        write_private(&path, content);
        assert!(load_provider_at(&path).is_err());
        write_private(
            &path,
            &format!("{content}FACTORY_ALLOW_UNCONFINED_PROVIDER=1\n"),
        );
        assert!(load_provider_at(&path).is_ok());
    }

    #[test]
    fn provider_file_rejects_unknown_keys_and_relative_binaries() {
        let temp = TempDir::new();
        let path = temp.path().join("provider.env");
        write_private(
            &path,
            &format!(
                "{}FACTORY_MODEL_TOKEN=secret-token-value\n",
                provider_content(&temp)
            ),
        );
        assert!(load_provider_at(&path).is_err());
        write_private(
            &path,
            &provider_content(&temp).replace(
                &temp.path().join("opencode").display().to_string(),
                "opencode",
            ),
        );
        assert!(load_provider_at(&path).is_err());
        write_private(
            &path,
            &provider_content(&temp).replace(
                "FACTORY_PROVIDER_MEMORY_LIMIT_BYTES=17179869184",
                "FACTORY_PROVIDER_MEMORY_LIMIT_BYTES=not-a-number",
            ),
        );
        assert!(load_provider_at(&path).is_err());
        write_private(
            &path,
            &provider_content(&temp).replace(
                "FACTORY_PROVIDER_KIND=opencode",
                "FACTORY_PROVIDER_KIND=unknown",
            ),
        );
        assert!(load_provider_at(&path).is_err());
    }

    #[test]
    fn provider_file_rejects_invalid_tcp_ports_and_args() {
        let temp = TempDir::new();
        let path = temp.path().join("provider.env");
        write_private(
            &path,
            &format!(
                "{}FACTORY_PROVIDER_TCP_PORT=18080\n",
                provider_content(&temp)
            ),
        );
        assert!(load_provider_at(&path).is_err());
        write_private(
            &path,
            &format!(
                "{}FACTORY_PROVIDER_ARGS=not-json\n",
                provider_content(&temp)
            ),
        );
        assert!(load_provider_at(&path).is_err());
        write_private(
            &path,
            &format!(
                "{}FACTORY_PROVIDER_ARGS=[\"serve\", 1]\n",
                provider_content(&temp)
            ),
        );
        assert!(load_provider_at(&path).is_err());
    }

    #[test]
    fn provider_file_must_not_contain_the_other_secret() {
        let temp = TempDir::new();
        let path = temp.path().join("provider.env");
        write_private(
            &path,
            &format!(
                "{}FACTORY_MODEL_TOKEN=secret-token-value\n",
                provider_content(&temp)
            ),
        );
        assert!(load_provider_at(&path).is_err());
        let core = temp.path().join("core.env");
        write_private(
            &core,
            &format!(
                "{}FACTORY_PROVIDER_UPSTREAM_TOKEN=super-secret-upstream-token\n",
                core_content()
            ),
        );
        assert!(load_core_model_at(&core).is_err());
    }
}
