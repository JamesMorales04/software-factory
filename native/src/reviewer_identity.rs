//! Trusted reviewer identity configuration.
//!
//! Compose provisions one token, principal and role per reviewer process. Core
//! authenticates the token and derives the principal and role from this trusted
//! configuration; a receipt can never choose its own identity. Tokens are held
//! only in memory and are registered with telemetry redaction by the caller.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::planning::{ReviewerPlan, ReviewerRole};
use crate::review_roles::{ReviewerIdentityAssignment, identity_assignments};

const MAX_IDENTITIES: usize = 16;
const MIN_TOKEN_BYTES: usize = 32;
const MAX_CONFIG_BYTES: u64 = 64 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ReviewerIdentityConfig {
    principal_id: String,
    role: ReviewerRole,
    token_file: String,
}

#[derive(Clone)]
pub struct ReviewerIdentity {
    principal_id: String,
    role: ReviewerRole,
    token: String,
}

impl ReviewerIdentity {
    pub fn principal_id(&self) -> &str {
        &self.principal_id
    }

    pub fn role(&self) -> ReviewerRole {
        self.role
    }
}

#[derive(Clone)]
pub struct ReviewerIdentitySet {
    identities: Vec<ReviewerIdentity>,
}

impl ReviewerIdentitySet {
    /// Load trusted identities from `FACTORY_REVIEWER_IDENTITIES_JSON` or
    /// `FACTORY_REVIEWER_IDENTITIES_FILE`. A single legacy identity is accepted
    /// only when neither variable is configured; it is always Functional and
    /// therefore cannot staff a multi-role plan.
    pub fn from_env() -> Result<Self, String> {
        let raw = std::env::var("FACTORY_REVIEWER_IDENTITIES_JSON").ok();
        let file = std::env::var("FACTORY_REVIEWER_IDENTITIES_FILE").ok();
        match (raw, file) {
            (Some(_), Some(_)) => Err(
                "FACTORY_REVIEWER_IDENTITIES_JSON and FACTORY_REVIEWER_IDENTITIES_FILE must not both be set"
                    .into(),
            ),
            (Some(raw), None) => Self::from_json(&raw),
            (None, Some(path)) => Self::from_config_file(&path),
            (None, None) => Self::legacy_single_identity(),
        }
    }

    pub fn from_config_file(path: &str) -> Result<Self, String> {
        let path = std::path::Path::new(path);
        if !path.is_absolute() || !path.is_file() {
            return Err("FACTORY_REVIEWER_IDENTITIES_FILE must be an absolute regular file".into());
        }
        let metadata = std::fs::metadata(path)
            .map_err(|error| format!("cannot inspect reviewer identities file: {error}"))?;
        if metadata.len() == 0 || metadata.len() > MAX_CONFIG_BYTES {
            return Err("reviewer identities file is empty or exceeds its bound".into());
        }
        let raw = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read reviewer identities file: {error}"))?;
        Self::from_json(&raw)
    }

    pub fn from_json(raw: &str) -> Result<Self, String> {
        let configs: Vec<ReviewerIdentityConfig> = serde_json::from_str(raw)
            .map_err(|_| "reviewer identities configuration is invalid JSON".to_string())?;
        if configs.is_empty() || configs.len() > MAX_IDENTITIES {
            return Err("reviewer identities configuration is empty or exceeds its bound".into());
        }
        let mut material = Vec::with_capacity(configs.len());
        for config in configs {
            let token = crate::secrets::read_restricted_file(
                "FACTORY_REVIEWER_IDENTITY_TOKEN",
                &config.token_file,
            )?;
            material.push((config.principal_id, config.role, token));
        }
        Self::from_material(material)
    }

    /// Validated constructor shared by configuration loading and tests.
    pub fn from_material(material: Vec<(String, ReviewerRole, String)>) -> Result<Self, String> {
        if material.is_empty() || material.len() > MAX_IDENTITIES {
            return Err("reviewer identities configuration is empty or exceeds its bound".into());
        }
        let mut principals = BTreeSet::new();
        let mut roles = BTreeSet::new();
        let mut tokens = BTreeSet::new();
        let mut identities = Vec::with_capacity(material.len());
        for (principal_id, role, token) in material {
            if principal_id.trim().is_empty()
                || principal_id.len() > 128
                || principal_id.chars().any(|character| character.is_control())
            {
                return Err("reviewer principal identity is invalid".into());
            }
            if !principals.insert(principal_id.clone()) {
                return Err("reviewer principals must be unique".into());
            }
            if !roles.insert(role) {
                return Err("each reviewer role must have exactly one identity".into());
            }
            if token.len() < MIN_TOKEN_BYTES {
                return Err("reviewer identity token is too short".into());
            }
            if !tokens.insert(token.clone()) {
                return Err("reviewer tokens must be unique per identity".into());
            }
            identities.push(ReviewerIdentity {
                principal_id,
                role,
                token,
            });
        }
        Ok(Self { identities })
    }

    fn legacy_single_identity() -> Result<Self, String> {
        let token = crate::secrets::required("FACTORY_REVIEWER_TOKEN")?;
        if token.len() < MIN_TOKEN_BYTES {
            return Err("FACTORY_REVIEWER_TOKEN must be at least 32 bytes".into());
        }
        let principal = std::env::var("FACTORY_REVIEWER_PRINCIPAL")
            .unwrap_or_else(|_| "reviewer-process".into());
        if principal.trim().is_empty() || principal.len() > 128 {
            return Err("FACTORY_REVIEWER_PRINCIPAL is invalid".into());
        }
        Ok(Self {
            identities: vec![ReviewerIdentity {
                principal_id: principal,
                role: ReviewerRole::Functional,
                token,
            }],
        })
    }

    pub fn authenticate(&self, token: &str) -> Option<&ReviewerIdentity> {
        self.identities
            .iter()
            .find(|identity| constant_time_eq(identity.token.as_bytes(), token.as_bytes()))
    }

    pub fn tokens(&self) -> impl Iterator<Item = &str> {
        self.identities
            .iter()
            .map(|identity| identity.token.as_str())
    }

    pub fn principals(&self) -> impl Iterator<Item = &str> {
        self.identities
            .iter()
            .map(|identity| identity.principal_id.as_str())
    }

    pub fn role_principals(&self) -> BTreeMap<ReviewerRole, String> {
        self.identities
            .iter()
            .map(|identity| (identity.role, identity.principal_id.clone()))
            .collect()
    }

    /// Fail closed unless every required role has one distinct identity.
    /// The message describes identity/process independence only.
    pub fn validate_plan(&self, plan: &ReviewerPlan) -> Result<(), String> {
        plan.validate()?;
        let principals = self.role_principals();
        let missing = plan
            .required
            .iter()
            .filter(|role| !principals.contains_key(role))
            .map(|role| format!("{role:?}"))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(format!(
                "reviewer independence requires {} distinct authenticated reviewer identities in separate processes for roles {}; this deployment is missing {}",
                plan.required_count(),
                plan.required
                    .iter()
                    .map(|role| format!("{role:?}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                missing.join(", ")
            ));
        }
        Ok(())
    }

    pub fn assignments_for_plan(
        &self,
        plan: &ReviewerPlan,
        provider: &str,
        model: &str,
    ) -> Result<Vec<ReviewerIdentityAssignment>, String> {
        self.validate_plan(plan)?;
        identity_assignments(plan, provider, model, &self.role_principals())
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    fn token_file(value: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "factory-reviewer-token-{}-{}.txt",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(value.as_bytes()).unwrap();
        drop(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        path
    }

    fn config_entry(token: &std::path::Path, principal: &str, role: &str) -> String {
        format!(
            r#"{{"principalId":"{principal}","role":"{role}","tokenFile":"{}"}}"#,
            token.display()
        )
    }

    fn config(token: &std::path::Path, principal: &str, role: &str) -> String {
        format!("[{}]", config_entry(token, principal, role))
    }

    #[test]
    fn parses_unique_identities_and_authenticates_each_token() {
        let functional = token_file("functional-token-aaaaaaaaaaaaaaaaaaaa");
        let security = token_file("security-token-bbbbbbbbbbbbbbbbbbbbbb");
        let raw = format!(
            "[{},{}]",
            config_entry(&functional, "reviewer-functional", "FUNCTIONAL"),
            config_entry(&security, "reviewer-security", "SECURITY")
        );
        let set = ReviewerIdentitySet::from_json(&raw).unwrap();
        assert_eq!(set.principals().count(), 2);
        assert_eq!(
            set.authenticate("security-token-bbbbbbbbbbbbbbbbbbbbbb")
                .unwrap()
                .role(),
            ReviewerRole::Security
        );
        assert!(set.authenticate("unknown").is_none());
        let _ = std::fs::remove_file(functional);
        let _ = std::fs::remove_file(security);
    }

    #[test]
    fn duplicate_tokens_principals_and_roles_are_rejected() {
        let first = token_file("shared-token-aaaaaaaaaaaaaaaaaaaaaa");
        let second = token_file("shared-token-aaaaaaaaaaaaaaaaaaaaaa");
        let duplicate_token = format!(
            "[{},{}]",
            config_entry(&first, "reviewer-functional", "FUNCTIONAL"),
            config_entry(&second, "reviewer-security", "SECURITY")
        );
        assert!(ReviewerIdentitySet::from_json(&duplicate_token).is_err());
        let other = token_file("other-token-ccccccccccccccccccccccc");
        let duplicate_principal = format!(
            "[{},{}]",
            config_entry(&first, "reviewer-same", "FUNCTIONAL"),
            config_entry(&other, "reviewer-same", "SECURITY")
        );
        assert!(ReviewerIdentitySet::from_json(&duplicate_principal).is_err());
        let _ = std::fs::remove_file(first);
        let _ = std::fs::remove_file(second);
        let _ = std::fs::remove_file(other);
    }

    #[test]
    fn plan_validation_fails_closed_for_a_missing_role() {
        let functional = token_file("functional-token-aaaaaaaaaaaaaaaaaaaa");
        let set = ReviewerIdentitySet::from_json(&config(
            &functional,
            "reviewer-functional",
            "FUNCTIONAL",
        ))
        .unwrap();
        let plan = ReviewerPlan::for_lane(crate::planning::PlanningLane::FormalSpec, false);
        let error = set.validate_plan(&plan).unwrap_err();
        assert!(error.contains("distinct authenticated reviewer identities"));
        assert!(!error.to_lowercase().contains("model"));
        assert!(!error.to_lowercase().contains("provider"));
        let _ = std::fs::remove_file(functional);
    }
}
