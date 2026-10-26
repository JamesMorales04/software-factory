//! File-or-environment secret loading for container deployments.
//!
//! Compose mounts generated secret files read-only. A variable and its
//! `_FILE` companion are mutually exclusive so precedence is never ambiguous.
//! Secret files must be absolute, regular, small and not group/world
//! accessible on Unix. Values are trimmed of one trailing newline only.

use std::path::Path;

const MAX_SECRET_BYTES: u64 = 64 * 1024;

pub fn required(name: &str) -> Result<String, String> {
    match optional(name)? {
        Some(value) => Ok(value),
        None => Err(format!("{name} or {name}_FILE is required")),
    }
}

pub fn optional(name: &str) -> Result<Option<String>, String> {
    let direct = std::env::var(name).ok();
    let file = std::env::var(format!("{name}_FILE")).ok();
    match (direct, file) {
        (Some(_), Some(_)) => Err(format!("{name} and {name}_FILE must not both be set")),
        (Some(value), None) => Ok(Some(value)),
        (None, Some(path)) => read_file(name, &path).map(Some),
        (None, None) => Ok(None),
    }
}

fn read_file(name: &str, path: &str) -> Result<String, String> {
    let path = Path::new(path);
    if !path.is_absolute() || !path.is_file() {
        return Err(format!("{name}_FILE must be an absolute regular file"));
    }
    let metadata =
        std::fs::metadata(path).map_err(|error| format!("cannot inspect {name}_FILE: {error}"))?;
    if metadata.len() > MAX_SECRET_BYTES {
        return Err(format!("{name}_FILE exceeds the secret size bound"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // The dedicated runtime engine is not the owner of the Factory
        // configuration volume and needs group read on the broker token. The
        // only permitted group bit is read; owner execute and all group/world
        // write/execute bits remain forbidden.
        if metadata.permissions().mode() & 0o137 != 0 {
            return Err(format!(
                "{name}_FILE permissions are broader than group-read"
            ));
        }
    }
    let value = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {name}_FILE: {error}"))?;
    let value = value.strip_suffix('\n').unwrap_or(&value);
    let value = value.strip_suffix('\r').unwrap_or(value);
    if value.trim().is_empty() {
        return Err(format!("{name}_FILE is empty"));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn reads_restricted_secret_file() {
        let path =
            std::env::temp_dir().join(format!("factory-secret-{}.txt", uuid::Uuid::new_v4()));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(b"value\n").unwrap();
        drop(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let name = format!("FACTORY_TEST_SECRET_{}", uuid::Uuid::new_v4().simple());
        unsafe {
            std::env::set_var(format!("{name}_FILE"), &path);
        }
        assert_eq!(required(&name).unwrap(), "value");
        unsafe {
            std::env::remove_var(format!("{name}_FILE"));
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_insecure_permissions() {
        let path =
            std::env::temp_dir().join(format!("factory-secret-open-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"value").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let name = format!("FACTORY_TEST_OPEN_{}", uuid::Uuid::new_v4().simple());
        unsafe {
            std::env::set_var(format!("{name}_FILE"), &path);
        }
        assert!(required(&name).is_err());
        unsafe {
            std::env::remove_var(format!("{name}_FILE"));
        }
        let _ = std::fs::remove_file(path);
    }
}
