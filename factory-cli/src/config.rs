use std::{
    env,
    path::{Path, PathBuf},
};

pub(crate) const DEFAULT_CORE_URL: &str = "http://127.0.0.1:8765";
pub(crate) const DEFAULT_IMAGE: &str = "localhost/software-factory-core:dev";
pub(crate) const DEFAULT_CONTAINER_NAME: &str = "factory-core";
pub(crate) const DEFAULT_VOLUME: &str = "factory-core-data";
pub(crate) const CORE_CONTAINER_PORT: u16 = 8765;
pub(crate) const CONTAINER_BIND: &str = "0.0.0.0:8765";
pub(crate) const CONTAINER_DATABASE: &str = "/data/factory.sqlite";

pub(crate) fn core_url() -> Result<String, String> {
    read("FACTORY_CORE_URL", DEFAULT_CORE_URL)
}

pub(crate) fn core_url_is_explicit() -> bool {
    env::var_os("FACTORY_CORE_URL").is_some()
}

pub(crate) fn image() -> Result<String, String> {
    read("FACTORY_IMAGE", DEFAULT_IMAGE)
}

pub(crate) fn container_name() -> Result<String, String> {
    read("FACTORY_CONTAINER_NAME", DEFAULT_CONTAINER_NAME)
}

pub(crate) fn volume() -> Result<String, String> {
    read("FACTORY_VOLUME", DEFAULT_VOLUME)
}

pub(crate) fn token_file() -> Result<PathBuf, String> {
    if let Some(value) = env::var_os("FACTORY_ENV_FILE") {
        let value = value
            .into_string()
            .map_err(|_| "FACTORY_ENV_FILE is not valid UTF-8".to_owned())?;
        if value.is_empty() {
            return Err("FACTORY_ENV_FILE is empty".to_owned());
        }
        return Ok(PathBuf::from(value));
    }
    token_file_from(env::var_os("XDG_CONFIG_HOME"), env::var_os("HOME"))
}

fn token_file_from(
    xdg_config_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Result<PathBuf, String> {
    if let Some(xdg) = xdg_config_home {
        let xdg = xdg
            .into_string()
            .map_err(|_| "XDG_CONFIG_HOME is not valid UTF-8".to_owned())?;
        if !xdg.is_empty() {
            return Ok(PathBuf::from(xdg).join("factory").join("core.env"));
        }
    }
    if let Some(home) = home {
        let home = home
            .into_string()
            .map_err(|_| "HOME is not valid UTF-8".to_owned())?;
        if !home.is_empty() {
            return Ok(PathBuf::from(home)
                .join(".config")
                .join("factory")
                .join("core.env"));
        }
    }
    Err("cannot determine the token env file path; set FACTORY_ENV_FILE".to_owned())
}

fn read(name: &str, default: &str) -> Result<String, String> {
    match env::var(name) {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(default.to_owned()),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{name} is not valid UTF-8")),
    }
}

pub(crate) fn supervisor_url() -> Result<String, String> {
    read("FACTORY_SUPERVISOR_URL", "http://127.0.0.1:8766")
}

pub(crate) fn host_execution() -> Result<bool, String> {
    match read("FACTORY_HOST_EXECUTION", "1")?.as_str() {
        "1" => Ok(true),
        "0" => Ok(false),
        other => Err(format!(
            "FACTORY_HOST_EXECUTION must be 0 or 1, found {other:?}"
        )),
    }
}

pub(crate) fn project_roots() -> Result<Vec<PathBuf>, String> {
    let Some(value) = env::var_os("FACTORY_PROJECT_ROOTS") else {
        return Ok(Vec::new());
    };
    let value = value
        .into_string()
        .map_err(|_| "FACTORY_PROJECT_ROOTS is not valid UTF-8".to_owned())?;
    let mut roots = Vec::new();
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err("FACTORY_PROJECT_ROOTS contains an empty entry".to_owned());
        }
        if part.contains([':', ',']) {
            return Err("FACTORY_PROJECT_ROOTS paths must not contain ':' or ','".to_owned());
        }
        let path = PathBuf::from(part);
        if !path.is_absolute() {
            return Err("FACTORY_PROJECT_ROOTS entries must be absolute".to_owned());
        }
        if path == Path::new("/") {
            return Err("FACTORY_PROJECT_ROOTS must not contain /".to_owned());
        }
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("cannot resolve project root {}: {error}", path.display()))?;
        if !canonical.is_dir() {
            return Err(format!(
                "project root is not a directory: {}",
                canonical.display()
            ));
        }
        if let Some(home) = env::var_os("HOME")
            && canonical.as_os_str() == home.as_os_str()
        {
            return Err("the host HOME cannot be a project root".to_owned());
        }
        if !roots.contains(&canonical) {
            roots.push(canonical);
        }
    }
    Ok(roots)
}

pub(crate) fn supervisor_workspace_root() -> Result<PathBuf, String> {
    state_path("FACTORY_SUPERVISOR_WORKSPACE_ROOT", "workspaces")
}

pub(crate) fn supervisor_state_root() -> Result<PathBuf, String> {
    state_path("FACTORY_SUPERVISOR_STATE_ROOT", "supervisor-state")
}

fn state_path(variable: &str, name: &str) -> Result<PathBuf, String> {
    if let Some(value) = env::var_os(variable) {
        let value = value
            .into_string()
            .map_err(|_| format!("{variable} is not valid UTF-8"))?;
        if value.is_empty() {
            return Err(format!("{variable} is empty"));
        }
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(format!("{variable} must be absolute"));
        }
        return Ok(path);
    }
    let base = if let Some(value) = env::var_os("XDG_STATE_HOME") {
        let value = value
            .into_string()
            .map_err(|_| "XDG_STATE_HOME is not valid UTF-8".to_owned())?;
        if value.is_empty() {
            home()?.join(".local").join("state")
        } else {
            PathBuf::from(value)
        }
    } else {
        home()?.join(".local").join("state")
    };
    Ok(base.join("factory").join(name))
}

fn home() -> Result<PathBuf, String> {
    let value = env::var_os("HOME").ok_or_else(|| "HOME is not set".to_owned())?;
    let value = value
        .into_string()
        .map_err(|_| "HOME is not valid UTF-8".to_owned())?;
    if value.is_empty() {
        return Err("HOME is empty".to_owned());
    }
    Ok(PathBuf::from(value))
}

pub(crate) fn supervisor_images() -> Result<String, String> {
    read("FACTORY_SUPERVISOR_IMAGES", "docker.io/library/alpine:3.20")
}

pub(crate) fn worker_fixture_image() -> Result<String, String> {
    read(
        "FACTORY_WORKER_FIXTURE_IMAGE",
        "docker.io/library/alpine:3.20",
    )
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, path::PathBuf};

    use super::token_file_from;

    #[test]
    fn prefers_xdg_config_home_for_the_default_token_file() {
        assert_eq!(
            token_file_from(
                Some(OsString::from("/xdg")),
                Some(OsString::from("/home/operator"))
            ),
            Ok(PathBuf::from("/xdg/factory/core.env"))
        );
    }

    #[test]
    fn falls_back_to_home_and_ignores_empty_values() {
        assert_eq!(
            token_file_from(None, Some(OsString::from("/home/operator"))),
            Ok(PathBuf::from("/home/operator/.config/factory/core.env"))
        );
        assert_eq!(
            token_file_from(
                Some(OsString::from("")),
                Some(OsString::from("/home/operator"))
            ),
            Ok(PathBuf::from("/home/operator/.config/factory/core.env"))
        );
    }

    #[test]
    fn fails_without_a_config_or_home_directory() {
        assert!(token_file_from(None, None).is_err());
        assert!(token_file_from(Some(OsString::from("")), Some(OsString::from(""))).is_err());
    }
}
