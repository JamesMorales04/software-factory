//! Dedicated Codex App Server authentication state.
//!
//! Factory uses the official App Server login and account APIs. It never reads
//! or copies a host `CODEX_HOME`; the caller must provide an absolute,
//! access-restricted directory through `FACTORY_CODEX_HOME`. Account and model
//! checks are pure functions over the App Server responses so ambiguous or
//! unsupported states fail closed.

use std::{
    env,
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

pub const RECEIPT_PROVIDER: &str = "codex";
pub const APP_SERVER_PROVIDER: &str = "openai";
pub const REVIEW_MODEL: &str = "gpt-6-luna";

#[derive(Debug, thiserror::Error)]
pub enum CodexAuthError {
    #[error("FACTORY_CODEX_HOME is required")]
    HomeMissing,
    #[error("Codex home is invalid: {0}")]
    InvalidHome(String),
    #[error("Codex authentication is unavailable: {0}")]
    Unavailable(String),
    #[error("Codex is logged out")]
    LoggedOut,
    #[error("Codex API-key authentication is not accepted for the review profile")]
    ApiKeyNotAccepted,
    #[error("Codex account type {0:?} is not a ChatGPT account")]
    UnsupportedAccount(String),
    #[error("Codex account state is unavailable or ambiguous")]
    AmbiguousAccount,
    #[error("Codex model {0:?} is not available exactly once")]
    ModelUnavailable(String),
    #[error("Codex model catalog is ambiguous for {0:?}")]
    AmbiguousModel(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGptAccount {
    pub email: Option<String>,
    pub plan_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountState {
    LoggedOut,
    ChatGpt(ChatGptAccount),
    ApiKey,
    Unsupported(String),
}

#[derive(Debug, Clone)]
pub struct CodexHome {
    path: PathBuf,
}

impl CodexHome {
    pub fn from_env() -> Result<Self, CodexAuthError> {
        let value = env::var("FACTORY_CODEX_HOME").map_err(|_| CodexAuthError::HomeMissing)?;
        Self::from_path(PathBuf::from(value))
    }

    pub fn from_path(path: PathBuf) -> Result<Self, CodexAuthError> {
        if !path.is_absolute() {
            return Err(CodexAuthError::InvalidHome("path must be absolute".into()));
        }
        if path.parent().is_none() {
            return Err(CodexAuthError::InvalidHome(
                "filesystem root is not a Codex home".into(),
            ));
        }
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(CodexAuthError::InvalidHome(
                        "path must be a real directory, not a symlink".into(),
                    ));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.permissions().mode() & 0o077 != 0 {
                        return Err(CodexAuthError::InvalidHome(
                            "directory must not be group- or world-accessible".into(),
                        ));
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir_all(&path)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
                }
            }
            Err(error) => return Err(error.into()),
        }
        let canonical = std::fs::canonicalize(&path)?;
        if canonical.parent().is_none() {
            return Err(CodexAuthError::InvalidHome(
                "filesystem root is not a Codex home".into(),
            ));
        }
        Ok(Self { path: canonical })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn auth_file(&self) -> PathBuf {
        self.path.join("auth.json")
    }

    pub fn config_file(&self) -> PathBuf {
        self.path.join("config.toml")
    }

    /// Install the reviewer's official App Server permission profile. The
    /// profile extends read-only access but denies the tool sandbox access to
    /// the Codex credential home, the reviewer configuration (which contains
    /// the Core reviewer token) and project mounts. The app-server process
    /// itself loads auth/config before applying this profile to tools.
    pub fn validate_private_file_for_review(&self) -> Result<(), CodexAuthError> {
        self.validate_private_file(&self.config_file(), "config.toml")
    }

    pub fn ensure_review_permissions(&self) -> Result<(), CodexAuthError> {
        let home = serde_json::to_string(self.path.to_string_lossy().as_ref())
            .map_err(|_| CodexAuthError::InvalidHome("home path encoding".into()))?;
        let config = format!(
            "default_permissions = \"factory-review\"\n\n\
             [permissions.factory-review]\n\
             description = \"Factory reviewer: read-only candidate, credentials denied\"\n\
             extends = \":read-only\"\n\n\
             [permissions.factory-review.filesystem]\n\
             {home} = \"deny\"\n\
             \"/config\" = \"deny\"\n\
             \"/projects\" = \"deny\"\n\n\
             [features]\n\
             shell_tool = false\n\
             view_image = false\n\
             sleep_tool = false\n\
             unified_exec = false\n\
             code_mode = false\n\
             web_search_request = false\n\
             web_search_cached = false\n\
             standalone_web_search = false\n\
             multi_agent = false\n\
             multi_agent_v2 = false\n"
        );
        let path = self.config_file();
        let temporary = self
            .path
            .join(format!(".config.toml.{}", uuid::Uuid::new_v4()));
        std::fs::write(&temporary, config)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&temporary, &path)?;
        Ok(())
    }

    /// The App Server owns the auth file. Factory only verifies that an
    /// existing file is a regular, owner-only file and never reads its bytes.
    pub fn validate_auth_file(&self) -> Result<(), CodexAuthError> {
        self.validate_private_file(&self.auth_file(), "auth.json")
    }

    fn validate_private_file(&self, path: &Path, label: &str) -> Result<(), CodexAuthError> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(CodexAuthError::InvalidHome(format!(
                        "{label} must be a regular file"
                    )));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.permissions().mode() & 0o077 != 0 {
                        return Err(CodexAuthError::InvalidHome(format!(
                            "{label} must not be group- or world-accessible"
                        )));
                    }
                }
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

impl From<std::io::Error> for CodexAuthError {
    fn from(error: std::io::Error) -> Self {
        CodexAuthError::Unavailable(error.to_string())
    }
}

pub fn parse_account_response(value: &Value) -> Result<AccountState, CodexAuthError> {
    let requires_auth = value
        .get("requiresOpenaiAuth")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            CodexAuthError::Unavailable("account/read omitted requiresOpenaiAuth".into())
        })?;
    let account = value.get("account").ok_or_else(|| {
        CodexAuthError::Unavailable("account/read omitted the account field".into())
    })?;
    if account.is_null() {
        return Ok(AccountState::LoggedOut);
    }
    let kind = account
        .get("type")
        .and_then(Value::as_str)
        .ok_or(CodexAuthError::AmbiguousAccount)?;
    match kind {
        "chatgpt" => {
            if !requires_auth {
                return Err(CodexAuthError::AmbiguousAccount);
            }
            let email = match account.get("email") {
                Some(Value::String(value)) if !value.trim().is_empty() => Some(value.clone()),
                Some(Value::Null) | None => None,
                _ => return Err(CodexAuthError::AmbiguousAccount),
            };
            let plan_type = account
                .get("planType")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or(CodexAuthError::AmbiguousAccount)?
                .to_owned();
            Ok(AccountState::ChatGpt(ChatGptAccount { email, plan_type }))
        }
        "apiKey" => Ok(AccountState::ApiKey),
        other => Ok(AccountState::Unsupported(other.to_owned())),
    }
}

pub fn require_chatgpt(value: &Value) -> Result<ChatGptAccount, CodexAuthError> {
    match parse_account_response(value)? {
        AccountState::ChatGpt(account) => Ok(account),
        AccountState::LoggedOut => Err(CodexAuthError::LoggedOut),
        AccountState::ApiKey => Err(CodexAuthError::ApiKeyNotAccepted),
        AccountState::Unsupported(kind) => Err(CodexAuthError::UnsupportedAccount(kind)),
    }
}

pub fn require_review_model(value: &Value, expected: &str) -> Result<(), CodexAuthError> {
    let models = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| CodexAuthError::ModelUnavailable(expected.into()))?;
    let mut matches = 0usize;
    for model in models {
        let Some(id) = model.get("id").and_then(Value::as_str) else {
            continue;
        };
        if id != expected {
            continue;
        }
        let Some(name) = model.get("model").and_then(Value::as_str) else {
            return Err(CodexAuthError::AmbiguousModel(expected.into()));
        };
        if name != expected || model.get("hidden").and_then(Value::as_bool) == Some(true) {
            return Err(CodexAuthError::ModelUnavailable(expected.into()));
        }
        matches += 1;
    }
    match matches {
        1 => Ok(()),
        0 => Err(CodexAuthError::ModelUnavailable(expected.into())),
        _ => Err(CodexAuthError::AmbiguousModel(expected.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chatgpt() -> Value {
        json!({
            "account": {"type":"chatgpt","email":"owner@example.invalid","planType":"pro"},
            "requiresOpenaiAuth": true,
            "workspaceRouting": null
        })
    }

    #[test]
    fn account_states_are_exact_and_fail_closed() {
        assert_eq!(
            require_chatgpt(&chatgpt()).unwrap(),
            ChatGptAccount {
                email: Some("owner@example.invalid".into()),
                plan_type: "pro".into()
            }
        );
        assert!(matches!(
            parse_account_response(&json!({
                "account": null, "requiresOpenaiAuth": true
            })),
            Ok(AccountState::LoggedOut)
        ));
        assert!(matches!(
            parse_account_response(&json!({
                "account": {"type":"apiKey"}, "requiresOpenaiAuth": true
            })),
            Ok(AccountState::ApiKey)
        ));
        assert!(matches!(
            require_chatgpt(&json!({
                "account": {"type":"apiKey"}, "requiresOpenaiAuth": true
            })),
            Err(CodexAuthError::ApiKeyNotAccepted)
        ));
        assert!(matches!(
            require_chatgpt(&json!({"account": null, "requiresOpenaiAuth": false})),
            Err(CodexAuthError::LoggedOut)
        ));
        assert!(matches!(
            require_chatgpt(&json!({
                "account": {"type":"chatgpt","email":null,"planType":"pro"},
                "requiresOpenaiAuth": false
            })),
            Err(CodexAuthError::AmbiguousAccount)
        ));
        assert!(matches!(
            require_chatgpt(&json!({
                "account": {"type":"amazonBedrock"}, "requiresOpenaiAuth": true
            })),
            Err(CodexAuthError::UnsupportedAccount(_))
        ));
    }

    #[test]
    fn model_must_be_present_exactly_once_and_not_hidden() {
        let exact =
            |id: &str, name: &str, hidden: bool| json!({"id":id,"model":name,"hidden":hidden});
        assert!(require_review_model(
            &json!({
                "data": [exact("gpt-6.1-sol","gpt-6.1-sol",false), exact(REVIEW_MODEL,REVIEW_MODEL,false)]
            }),
            REVIEW_MODEL
        )
        .is_ok());
        assert!(matches!(
            require_review_model(&json!({"data": []}), REVIEW_MODEL),
            Err(CodexAuthError::ModelUnavailable(_))
        ));
        assert!(matches!(
            require_review_model(
                &json!({"data": [exact(REVIEW_MODEL,REVIEW_MODEL,true)]}),
                REVIEW_MODEL
            ),
            Err(CodexAuthError::ModelUnavailable(_))
        ));
        assert!(matches!(
            require_review_model(
                &json!({"data": [exact(REVIEW_MODEL,REVIEW_MODEL,false), exact(REVIEW_MODEL,REVIEW_MODEL,false)]}),
                REVIEW_MODEL
            ),
            Err(CodexAuthError::AmbiguousModel(_))
        ));
        assert!(matches!(
            require_review_model(
                &json!({"data": [exact(REVIEW_MODEL,"other",false)]}),
                REVIEW_MODEL
            ),
            Err(CodexAuthError::ModelUnavailable(_))
        ));
    }

    #[test]
    fn codex_home_rejects_insecure_paths_and_creates_private_directory() {
        let root =
            std::env::temp_dir().join(format!("factory-codex-home-{}", uuid::Uuid::new_v4()));
        let home = CodexHome::from_path(root.clone()).unwrap();
        assert_eq!(home.path(), std::fs::canonicalize(&root).unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(home.path()).unwrap().permissions().mode() & 0o777,
                0o700
            );
            std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(matches!(
                CodexHome::from_path(root.clone()),
                Err(CodexAuthError::InvalidHome(_))
            ));
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn review_permissions_deny_credentials_and_projects() {
        let root =
            std::env::temp_dir().join(format!("factory-codex-profile-{}", uuid::Uuid::new_v4()));
        let home = CodexHome::from_path(root.clone()).unwrap();
        home.ensure_review_permissions().unwrap();
        let config = std::fs::read_to_string(home.config_file()).unwrap();
        assert!(config.contains("default_permissions = \"factory-review\""));
        assert!(config.contains(&format!("\"{}\" = \"deny\"", home.path().to_string_lossy())));
        assert!(config.contains("\"/config\" = \"deny\""));
        assert!(config.contains("\"/projects\" = \"deny\""));
        assert!(config.contains("shell_tool = false"));
        assert!(config.contains("web_search_request = false"));
        assert!(config.contains("multi_agent_v2 = false"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(home.config_file())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn auth_file_must_be_private_and_regular() {
        let root =
            std::env::temp_dir().join(format!("factory-codex-auth-{}", uuid::Uuid::new_v4()));
        let home = CodexHome::from_path(root.clone()).unwrap();
        std::fs::write(home.auth_file(), "{}").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(home.auth_file(), std::fs::Permissions::from_mode(0o644))
                .unwrap();
            assert!(matches!(
                home.validate_auth_file(),
                Err(CodexAuthError::InvalidHome(_))
            ));
            std::fs::set_permissions(home.auth_file(), std::fs::Permissions::from_mode(0o600))
                .unwrap();
            assert!(home.validate_auth_file().is_ok());
            let link = home.path().join("auth-link.json");
            std::os::unix::fs::symlink(home.auth_file(), &link).unwrap();
            let link_home = CodexHome {
                path: home.path().to_path_buf(),
            };
            std::fs::remove_file(home.auth_file()).unwrap();
            std::fs::rename(link, home.auth_file()).unwrap();
            assert!(matches!(
                link_home.validate_auth_file(),
                Err(CodexAuthError::InvalidHome(_))
            ));
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
