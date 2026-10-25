use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const TOKEN_KEYS: [&str; 6] = [
    "FACTORY_OPERATOR_TOKEN",
    "FACTORY_WORKER_BOOTSTRAP_TOKEN",
    "FACTORY_VERIFIER_TOKEN",
    "FACTORY_REVIEWER_TOKEN",
    "FACTORY_ATTEMPT_GRANT_KEY",
    "FACTORY_SUPERVISOR_CAPABILITY_KEY",
];

const TOKEN_BYTES: usize = 32;
const TOKEN_COUNT: usize = TOKEN_KEYS.len();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenFileState {
    Created,
    Existing,
}

pub(crate) fn ensure(path: &Path) -> Result<TokenFileState, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "token env file {} must not be a symbolic link",
                    path.display()
                ));
            }
            if !metadata.is_file() {
                return Err(format!(
                    "token env file {} is not a regular file",
                    path.display()
                ));
            }
            check_private_permissions(path, &metadata)?;
            let content = fs::read_to_string(path).map_err(|error| {
                format!("cannot read token env file {}: {error}", path.display())
            })?;
            validate_content(&content)?;
            Ok(TokenFileState::Existing)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create(path)?;
            Ok(TokenFileState::Created)
        }
        Err(error) => Err(format!(
            "cannot inspect token env file {}: {error}",
            path.display()
        )),
    }
}

fn check_private_permissions(path: &Path, metadata: &fs::Metadata) -> Result<(), String> {
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(format!(
            "token env file {} has mode {mode:o}; group and other permissions must be removed (chmod 600)",
            path.display()
        ));
    }
    if mode & 0o400 == 0 {
        return Err(format!(
            "token env file {} is not readable by its owner (chmod 600)",
            path.display()
        ));
    }
    Ok(())
}

fn create(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_existed = parent.exists();
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    if !parent_existed {
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|error| {
            format!(
                "cannot restrict permissions on token directory {}: {error}",
                parent.display()
            )
        })?;
    }

    let tokens = generate_distinct_tokens()?;
    let temporary = temporary_path(path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| {
            format!(
                "cannot create temporary token env file {}: {error}",
                temporary.display()
            )
        })?;
    let write_result = write_tokens(&mut file, &tokens);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if let Err(error) = file.sync_all() {
        let _ = fs::remove_file(&temporary);
        return Err(format!("cannot flush token env file: {error}"));
    }
    drop(file);
    if let Err(error) = fs::hard_link(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!(
            "cannot install token env file {}: {error}",
            path.display()
        ));
    }
    fs::remove_file(&temporary).map_err(|error| {
        format!(
            "cannot remove temporary token env file {}: {error}",
            temporary.display()
        )
    })?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        format!(
            "cannot set token env file permissions on {}: {error}",
            path.display()
        )
    })?;
    Ok(())
}

fn write_tokens(file: &mut File, tokens: &[String; TOKEN_COUNT]) -> Result<(), String> {
    for (key, token) in TOKEN_KEYS.iter().zip(tokens) {
        writeln!(file, "{key}={token}")
            .map_err(|error| format!("cannot write token env file: {error}"))?;
    }
    Ok(())
}

fn validate_content(content: &str) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    let mut values = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(|| {
            format!(
                "token env file line {} is not KEY=VALUE",
                index.saturating_add(1)
            )
        })?;
        let key = key.trim();
        if !TOKEN_KEYS.contains(&key) {
            return Err(format!(
                "token env file contains unsupported key {key:?}; expected only {TOKEN_KEYS:?}"
            ));
        }
        if !seen.insert(key) {
            return Err(format!("token env file contains duplicate key {key}"));
        }
        if value.len() < 32 {
            return Err(format!(
                "token env file value for {key} must be at least 32 bytes"
            ));
        }
        if values.contains(&value) {
            return Err("token env file role tokens must be distinct".to_owned());
        }
        values.push(value);
    }
    for key in TOKEN_KEYS {
        if !seen.contains(key) {
            return Err(format!("token env file is missing {key}"));
        }
    }
    Ok(())
}

fn generate_distinct_tokens() -> Result<[String; TOKEN_COUNT], String> {
    let mut tokens: Vec<String> = Vec::with_capacity(TOKEN_COUNT);
    while tokens.len() < TOKEN_COUNT {
        let token = generate_token()?;
        if !tokens.contains(&token) {
            tokens.push(token);
        }
    }
    Ok(tokens
        .try_into()
        .expect("token count matches the key count"))
}

pub(crate) fn read(path: &Path, key: &str) -> Result<String, String> {
    ensure(path)?;
    let content = fs::read_to_string(path)
        .map_err(|error| format!("cannot read token env file {}: {error}", path.display()))?;
    for line in content.lines() {
        let line = line.trim();
        if let Some((candidate, value)) = line.split_once('=')
            && candidate.trim() == key
        {
            return Ok(value.trim().to_owned());
        }
    }
    Err(format!("token env file is missing {key}"))
}

pub(crate) fn generate_token() -> Result<String, String> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .map_err(|error| format!("cannot read cryptographic randomness: {error}"))?;
    let mut token = String::with_capacity(TOKEN_BYTES * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut token, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(token)
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("core.env");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    parent.join(format!(".{name}.{}.{nonce}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{TOKEN_KEYS, TokenFileState, ensure};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "factory-cli-token-test-{}-{nonce}",
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

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).expect("metadata").permissions().mode() & 0o777
    }

    #[test]
    fn creates_a_private_token_file_with_distinct_role_tokens() {
        let temp = TempDir::new();
        let path = temp.path().join("factory").join("core.env");
        assert_eq!(
            ensure(&path).expect("ensure token file"),
            TokenFileState::Created
        );

        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().expect("parent")), 0o700);
        let content = fs::read_to_string(&path).expect("token file");
        for key in TOKEN_KEYS {
            assert_eq!(
                content
                    .lines()
                    .filter(|line| line.starts_with(&format!("{key}=")))
                    .count(),
                1,
                "key {key}"
            );
        }
        let values: Vec<&str> = content
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(_, value)| value)
            .collect();
        assert_eq!(values.len(), TOKEN_KEYS.len());
        assert!(values.iter().all(|value| value.len() >= 32));
        let distinct: std::collections::BTreeSet<_> = values.iter().collect();
        assert_eq!(distinct.len(), values.len());
    }

    #[test]
    fn preserves_an_existing_private_token_file() {
        let temp = TempDir::new();
        let path = temp.path().join("core.env");
        assert_eq!(ensure(&path).expect("create"), TokenFileState::Created);
        let before = fs::read_to_string(&path).expect("read");
        assert_eq!(ensure(&path).expect("reuse"), TokenFileState::Existing);
        assert_eq!(fs::read_to_string(&path).expect("read"), before);
        assert_eq!(mode(&path), 0o600);
    }

    #[test]
    fn rejects_group_or_world_accessible_token_files() {
        let temp = TempDir::new();
        let path = temp.path().join("core.env");
        ensure(&path).expect("create");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("chmod");
        assert!(ensure(&path).is_err());
    }

    #[test]
    fn rejects_duplicate_tokens_and_missing_keys() {
        let temp = TempDir::new();
        let path = temp.path().join("core.env");
        let duplicate = "a".repeat(64);
        let content = TOKEN_KEYS
            .iter()
            .map(|key| format!("{key}={duplicate}\n"))
            .collect::<String>();
        fs::write(&path, content).expect("write");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("chmod");
        assert!(ensure(&path).is_err());

        fs::write(&path, "FACTORY_OPERATOR_TOKEN=short\n").expect("write");
        assert!(ensure(&path).is_err());
    }
}
