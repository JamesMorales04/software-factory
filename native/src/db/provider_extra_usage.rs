//! Durable Owner declaration for the upstream Extra Usage setting.
//!
//! The declaration is a factual statement about one exact provider
//! configuration: the provider account is identified by the provider name, its
//! canonical base URL and the existing credential fingerprint. It is not a
//! policy, budget, routing or execution authorization; every admission still
//! passes the frozen route, settings and campaign checks.
//!
//! Invalidation is factual only: a changed credential fingerprint or base URL,
//! or an explicit revocation. Settings revision changes (model, timeouts,
//! display, pricing) do not change the upstream Extra Usage setting and
//! therefore do not invalidate the declaration.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::row::Row;
use sqlx_sqlite::{SqliteConnection, SqliteRow};

use super::Error;

pub const MAX_STATEMENT_CHARS: usize = 2_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderExtraUsageDeclaration {
    pub declaration_id: String,
    pub provider: String,
    pub base_url: String,
    pub credential_fingerprint: String,
    pub principal_id: String,
    pub statement: String,
    pub extra_usage: String,
    pub declared_at: String,
    pub revoked_at: Option<String>,
    pub revoked_by: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DeclareProviderExtraUsage<'a> {
    pub provider: &'a str,
    pub base_url: &'a str,
    pub credential_fingerprint: &'a str,
    pub principal_id: &'a str,
    pub statement: &'a str,
    /// Audit metadata only; it is intentionally not part of the binding.
    pub settings_revision: u32,
    pub idempotency_key: &'a str,
}

pub struct DeclaredProviderExtraUsage {
    pub declaration: ProviderExtraUsageDeclaration,
    pub created: bool,
}

fn validate_declaration(input: &DeclareProviderExtraUsage<'_>) -> Result<(), Error> {
    let statement = input.statement.trim();
    if input.provider != "opencode-go" {
        return Err(Error::Invalid(
            "the Owner Extra Usage declaration is only valid for opencode-go",
        ));
    }
    if !crate::provider_catalog::is_canonical_opencode_go_base_url(input.base_url)
        && !crate::provider_catalog::is_test_upstream_url(input.base_url)
    {
        return Err(Error::Invalid(
            "the Owner Extra Usage declaration requires the canonical OpenCode Go endpoint",
        ));
    }
    if input.provider.trim().is_empty()
        || input.provider.len() > 64
        || !(input.base_url.starts_with("https://") || input.base_url.starts_with("http://"))
        || input.base_url.len() > 512
        || input.credential_fingerprint.trim().is_empty()
        || input.credential_fingerprint.len() > 128
        || input.principal_id.trim().is_empty()
        || input.principal_id.len() > 128
        || statement.len() < 20
        || statement.len() > MAX_STATEMENT_CHARS
        || input.idempotency_key.trim().is_empty()
        || input.idempotency_key.len() > 200
    {
        return Err(Error::Invalid(
            "provider Extra Usage declaration is not bounded",
        ));
    }
    Ok(())
}

fn row_to_declaration(row: &SqliteRow) -> Result<ProviderExtraUsageDeclaration, sqlx::Error> {
    Ok(ProviderExtraUsageDeclaration {
        declaration_id: row.try_get("declaration_id")?,
        provider: row.try_get("provider")?,
        base_url: row.try_get("base_url")?,
        credential_fingerprint: row.try_get("credential_fingerprint")?,
        principal_id: row.try_get("principal_id")?,
        statement: row.try_get("statement")?,
        extra_usage: row.try_get("extra_usage")?,
        declared_at: row.try_get("declared_at")?,
        revoked_at: row.try_get("revoked_at")?,
        revoked_by: row.try_get("revoked_by")?,
    })
}

const DECLARATION_COLUMNS: &str = "declaration_id,provider,base_url,credential_fingerprint,
    principal_id,statement,extra_usage,declared_at,revoked_at,revoked_by";

impl super::Database {
    /// Active declaration for the exact provider configuration, if any.
    pub async fn active_provider_extra_usage_declaration(
        &self,
        provider: &str,
        base_url: &str,
        credential_fingerprint: &str,
    ) -> Result<Option<ProviderExtraUsageDeclaration>, Error> {
        let mut connection = self.pool.acquire().await?;
        active_declaration_in(&mut connection, provider, base_url, credential_fingerprint).await
    }

    /// Latest declaration for a provider, including a revoked historical row.
    pub async fn provider_extra_usage_declaration(
        &self,
        provider: &str,
    ) -> Result<Option<ProviderExtraUsageDeclaration>, Error> {
        let mut connection = self.pool.acquire().await?;
        let row = sqlx::query::query(&format!(
            "SELECT {DECLARATION_COLUMNS} FROM provider_extra_usage_declarations
             WHERE provider=?1 ORDER BY declared_at DESC LIMIT 1"
        ))
        .bind(provider)
        .fetch_optional(&mut *connection)
        .await?;
        row.as_ref()
            .map(row_to_declaration)
            .transpose()
            .map_err(Error::from)
    }

    /// Record the Owner declaration once. Repeating the same request or the
    /// same exact configuration returns the existing active declaration
    /// instead of creating a second authority.
    pub async fn declare_provider_extra_usage(
        &self,
        input: DeclareProviderExtraUsage<'_>,
    ) -> Result<DeclaredProviderExtraUsage, Error> {
        validate_declaration(&input)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(row) = sqlx::query::query(&format!(
            "SELECT {DECLARATION_COLUMNS} FROM provider_extra_usage_declarations
             WHERE provider=?1 AND idempotency_key=?2"
        ))
        .bind(input.provider)
        .bind(input.idempotency_key)
        .fetch_optional(&mut *transaction)
        .await?
        {
            let declaration = row_to_declaration(&row)?;
            if declaration.base_url != input.base_url
                || declaration.credential_fingerprint != input.credential_fingerprint
            {
                return Err(Error::Conflict(
                    "the provider Extra Usage idempotency key is bound to another configuration",
                ));
            }
            transaction.commit().await?;
            return Ok(DeclaredProviderExtraUsage {
                declaration,
                created: false,
            });
        }
        if let Some(declaration) = active_declaration_in(
            &mut transaction,
            input.provider,
            input.base_url,
            input.credential_fingerprint,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(DeclaredProviderExtraUsage {
                declaration,
                created: false,
            });
        }
        let declaration = ProviderExtraUsageDeclaration {
            declaration_id: uuid::Uuid::new_v4().to_string(),
            provider: input.provider.to_owned(),
            base_url: input.base_url.to_owned(),
            credential_fingerprint: input.credential_fingerprint.to_owned(),
            principal_id: input.principal_id.to_owned(),
            statement: input.statement.trim().to_owned(),
            extra_usage: "DISABLED".into(),
            declared_at: Utc::now().to_rfc3339(),
            revoked_at: None,
            revoked_by: None,
        };
        sqlx::query::query(
            "INSERT INTO provider_extra_usage_declarations
             (declaration_id,provider,base_url,credential_fingerprint,principal_id,
              statement,extra_usage,declared_at,revoked_at,revoked_by,idempotency_key)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,NULL,NULL,?9)",
        )
        .bind(&declaration.declaration_id)
        .bind(&declaration.provider)
        .bind(&declaration.base_url)
        .bind(&declaration.credential_fingerprint)
        .bind(&declaration.principal_id)
        .bind(&declaration.statement)
        .bind(&declaration.extra_usage)
        .bind(&declaration.declared_at)
        .bind(input.idempotency_key)
        .execute(&mut *transaction)
        .await?;
        record_audit(
            &mut transaction,
            input.provider,
            input.settings_revision,
            "extra_usage_declared",
            input.principal_id,
            &format!(
                "declaration={} base_url={} credential_fingerprint={}",
                declaration.declaration_id,
                declaration.base_url,
                declaration.credential_fingerprint
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok(DeclaredProviderExtraUsage {
            declaration,
            created: true,
        })
    }

    /// Revoke the active declaration for a provider. A later admission must ask
    /// the Owner again; the historical row is kept for audit.
    pub async fn revoke_provider_extra_usage(
        &self,
        provider: &str,
        base_url: &str,
        credential_fingerprint: &str,
        principal_id: &str,
    ) -> Result<ProviderExtraUsageDeclaration, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query::query(&format!(
            "SELECT {DECLARATION_COLUMNS} FROM provider_extra_usage_declarations
             WHERE provider=?1 AND base_url=?2 AND credential_fingerprint=?3
               AND revoked_at IS NULL
             ORDER BY declared_at DESC LIMIT 1"
        ))
        .bind(provider)
        .bind(base_url)
        .bind(credential_fingerprint)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            return Err(Error::NotFound);
        };
        let mut declaration = row_to_declaration(&row)?;
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "UPDATE provider_extra_usage_declarations
             SET revoked_at=?2, revoked_by=?3
             WHERE declaration_id=?1 AND revoked_at IS NULL",
        )
        .bind(&declaration.declaration_id)
        .bind(&now)
        .bind(principal_id)
        .execute(&mut *transaction)
        .await?;
        declaration.revoked_at = Some(now);
        declaration.revoked_by = Some(principal_id.to_owned());
        record_audit(
            &mut transaction,
            provider,
            0,
            "extra_usage_revoked",
            principal_id,
            &format!("declaration={}", declaration.declaration_id),
        )
        .await?;
        transaction.commit().await?;
        Ok(declaration)
    }
}

/// Revoke every active declaration for one exact configuration. Used when a
/// real credential or endpoint change makes the old Owner statement stale, so
/// switching back cannot silently reactivate it.
pub(crate) async fn revoke_configuration_in(
    connection: &mut SqliteConnection,
    provider: &str,
    base_url: &str,
    credential_fingerprint: &str,
    actor: &str,
    reason: &str,
) -> Result<u64, Error> {
    let now = Utc::now().to_rfc3339();
    let result = sqlx::query::query(
        "UPDATE provider_extra_usage_declarations
         SET revoked_at=?1, revoked_by=?2
         WHERE provider=?3 AND base_url=?4 AND credential_fingerprint=?5
           AND revoked_at IS NULL",
    )
    .bind(&now)
    .bind(actor)
    .bind(provider)
    .bind(base_url)
    .bind(credential_fingerprint)
    .execute(&mut *connection)
    .await?;
    if result.rows_affected() > 0 {
        record_audit(
            connection,
            provider,
            0,
            "extra_usage_invalidated",
            actor,
            &format!("reason={reason} base_url={base_url} fingerprint={credential_fingerprint}"),
        )
        .await?;
    }
    Ok(result.rows_affected())
}

async fn active_declaration_in(
    connection: &mut SqliteConnection,
    provider: &str,
    base_url: &str,
    credential_fingerprint: &str,
) -> Result<Option<ProviderExtraUsageDeclaration>, Error> {
    let row = sqlx::query::query(&format!(
        "SELECT {DECLARATION_COLUMNS} FROM provider_extra_usage_declarations
         WHERE provider=?1 AND base_url=?2 AND credential_fingerprint=?3
           AND revoked_at IS NULL
         ORDER BY declared_at DESC LIMIT 1"
    ))
    .bind(provider)
    .bind(base_url)
    .bind(credential_fingerprint)
    .fetch_optional(&mut *connection)
    .await?;
    row.as_ref()
        .map(row_to_declaration)
        .transpose()
        .map_err(Error::from)
}

async fn record_audit(
    connection: &mut SqliteConnection,
    provider: &str,
    revision: u32,
    action: &str,
    actor: &str,
    detail: &str,
) -> Result<(), Error> {
    sqlx::query::query(
        "INSERT INTO provider_settings_audit
         (audit_id,provider,revision,action,actor,detail,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(provider)
    .bind(i64::from(revision))
    .bind(action)
    .bind(actor)
    .bind(detail)
    .bind(Utc::now().to_rfc3339())
    .execute(&mut *connection)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    async fn fixture_db() -> (PathBuf, super::super::Database) {
        let path = std::env::temp_dir().join(format!(
            "factory-provider-extra-usage-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let db = super::super::Database::open(&path).await.unwrap();
        (path, db)
    }

    fn declaration<'a>(
        provider: &'a str,
        base_url: &'a str,
        fingerprint: &'a str,
        key: &'a str,
    ) -> DeclareProviderExtraUsage<'a> {
        DeclareProviderExtraUsage {
            provider,
            base_url,
            credential_fingerprint: fingerprint,
            principal_id: "operator-token",
            statement: "I confirm OpenCode Go Extra Usage is disabled for this provider configuration.",
            settings_revision: 7,
            idempotency_key: key,
        }
    }

    #[tokio::test]
    async fn declaration_is_durable_idempotent_and_revision_independent() {
        let (path, db) = fixture_db().await;
        let first = db
            .declare_provider_extra_usage(declaration(
                "opencode-go",
                "https://opencode.ai/inference/go/openai/v1",
                "fingerprint-a",
                "declare-1",
            ))
            .await
            .unwrap();
        assert!(first.created);
        let replay = db
            .declare_provider_extra_usage(declaration(
                "opencode-go",
                "https://opencode.ai/inference/go/openai/v1",
                "fingerprint-a",
                "declare-1",
            ))
            .await
            .unwrap();
        assert!(!replay.created);
        assert_eq!(
            replay.declaration.declaration_id,
            first.declaration.declaration_id
        );
        let same_configuration = db
            .declare_provider_extra_usage(declaration(
                "opencode-go",
                "https://opencode.ai/inference/go/openai/v1",
                "fingerprint-a",
                "declare-2",
            ))
            .await
            .unwrap();
        assert!(!same_configuration.created);
        assert_eq!(
            same_configuration.declaration.declaration_id,
            first.declaration.declaration_id
        );
        assert!(
            db.active_provider_extra_usage_declaration(
                "opencode-go",
                "https://opencode.ai/inference/go/openai/v1",
                "fingerprint-a"
            )
            .await
            .unwrap()
            .is_some()
        );
        // A settings revision change does not change the account binding.
        assert!(
            db.active_provider_extra_usage_declaration(
                "opencode-go",
                "https://opencode.ai/inference/go/openai/v1",
                "fingerprint-a"
            )
            .await
            .unwrap()
            .is_some()
        );
        // A different credential fingerprint or endpoint is a different account
        // configuration and is not covered by the existing declaration.
        assert!(
            db.active_provider_extra_usage_declaration(
                "opencode-go",
                "https://opencode.ai/inference/go/openai/v1",
                "fingerprint-b"
            )
            .await
            .unwrap()
            .is_none()
        );
        assert!(
            db.active_provider_extra_usage_declaration(
                "opencode-go",
                "https://other.example/v1",
                "fingerprint-a"
            )
            .await
            .unwrap()
            .is_none()
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn revocation_is_factual_and_requires_a_new_declaration() {
        let (path, db) = fixture_db().await;
        let first = db
            .declare_provider_extra_usage(declaration(
                "opencode-go",
                "https://opencode.ai/inference/go/openai/v1",
                "fingerprint-a",
                "declare-revoke-1",
            ))
            .await
            .unwrap();
        let revoked = db
            .revoke_provider_extra_usage(
                "opencode-go",
                "https://opencode.ai/inference/go/openai/v1",
                "fingerprint-a",
                "operator-token",
            )
            .await
            .unwrap();
        assert!(revoked.revoked_at.is_some());
        assert_eq!(revoked.declaration_id, first.declaration.declaration_id);
        assert!(
            db.active_provider_extra_usage_declaration(
                "opencode-go",
                "https://opencode.ai/inference/go/openai/v1",
                "fingerprint-a"
            )
            .await
            .unwrap()
            .is_none()
        );
        assert!(
            db.revoke_provider_extra_usage(
                "opencode-go",
                "https://opencode.ai/inference/go/openai/v1",
                "fingerprint-a",
                "operator-token",
            )
            .await
            .is_err()
        );
        let renewed = db
            .declare_provider_extra_usage(declaration(
                "opencode-go",
                "https://opencode.ai/inference/go/openai/v1",
                "fingerprint-a",
                "declare-revoke-2",
            ))
            .await
            .unwrap();
        assert!(renewed.created);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn configuration_changes_invalidate_and_cannot_reactivate() {
        let (path, db) = fixture_db().await;
        let a = "https://opencode.ai/inference/go/openai/v1";
        let settings = |base_url: &str, fingerprint: &str| super::super::ProviderSettingsWrite {
            provider: "opencode-go".into(),
            kind: "opencode".into(),
            model: "deepseek-v4.1-flash".into(),
            base_url: base_url.into(),
            auth_mode: "api_key".into(),
            credential_ciphertext: "ciphertext".into(),
            credential_nonce: "nonce".into(),
            credential_fingerprint: fingerprint.into(),
            enabled: true,
            is_default: true,
            max_turns: 8,
            max_input_tokens: 128_000,
            max_output_tokens: 8_192,
            max_cost_micros: 1_000_000,
            input_micros_per_million: 0,
            output_micros_per_million: 0,
        };
        db.save_provider_settings(&settings(a, "fingerprint-a"), "owner")
            .await
            .unwrap();
        db.declare_provider_extra_usage(declaration(
            "opencode-go",
            a,
            "fingerprint-a",
            "invalidate-1",
        ))
        .await
        .unwrap();
        db.save_provider_settings(
            &settings("https://example.invalid/v1", "fingerprint-a"),
            "owner",
        )
        .await
        .unwrap();
        assert!(
            db.active_provider_extra_usage_declaration("opencode-go", a, "fingerprint-a")
                .await
                .unwrap()
                .is_none(),
            "changing the endpoint must revoke the old declaration"
        );
        // Switching back to the previous endpoint does not resurrect it.
        db.save_provider_settings(&settings(a, "fingerprint-a"), "owner")
            .await
            .unwrap();
        assert!(
            db.active_provider_extra_usage_declaration("opencode-go", a, "fingerprint-a")
                .await
                .unwrap()
                .is_none()
        );
        // A credential-fingerprint change behaves the same way.
        db.save_provider_settings(&settings(a, "fingerprint-b"), "owner")
            .await
            .unwrap();
        db.save_provider_settings(&settings(a, "fingerprint-a"), "owner")
            .await
            .unwrap();
        assert!(
            db.active_provider_extra_usage_declaration("opencode-go", a, "fingerprint-a")
                .await
                .unwrap()
                .is_none(),
            "a fingerprint change must not reactivate by reverting"
        );
        assert!(
            db.active_provider_extra_usage_declaration("opencode-go", a, "fingerprint-a")
                .await
                .unwrap()
                .is_none(),
            "a revoked declaration must not reactivate by reverting settings"
        );
        let renewed = db
            .declare_provider_extra_usage(declaration(
                "opencode-go",
                a,
                "fingerprint-a",
                "invalidate-2",
            ))
            .await
            .unwrap();
        assert!(renewed.created);
        db.clear_provider_credential("opencode-go", "owner")
            .await
            .unwrap();
        assert!(
            db.active_provider_extra_usage_declaration("opencode-go", a, "fingerprint-a")
                .await
                .unwrap()
                .is_none(),
            "clearing the credential must revoke the declaration"
        );
        db.save_provider_settings(&settings(a, "fingerprint-a"), "owner")
            .await
            .unwrap();
        let after_clear = db
            .declare_provider_extra_usage(declaration(
                "opencode-go",
                a,
                "fingerprint-a",
                "invalidate-3",
            ))
            .await
            .unwrap();
        assert!(
            after_clear.created,
            "a new declaration is required after clear"
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn revocation_targets_the_current_configuration_only() {
        let (path, db) = fixture_db().await;
        let a = "https://opencode.ai/inference/go/openai/v1";
        let b = "https://opencode.ai/inference/go/openai/v1";
        db.declare_provider_extra_usage(declaration("opencode-go", a, "fingerprint-a", "multi-a"))
            .await
            .unwrap();
        db.declare_provider_extra_usage(declaration("opencode-go", b, "fingerprint-b", "multi-b"))
            .await
            .unwrap();
        db.revoke_provider_extra_usage("opencode-go", a, "fingerprint-a", "owner")
            .await
            .unwrap();
        assert!(
            db.active_provider_extra_usage_declaration("opencode-go", a, "fingerprint-a")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            db.active_provider_extra_usage_declaration("opencode-go", b, "fingerprint-b")
                .await
                .unwrap()
                .is_some(),
            "revoking one configuration must not revoke another active one"
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn unbounded_or_mismatched_declarations_are_refused() {
        let (path, db) = fixture_db().await;
        let mut short = declaration(
            "opencode-go",
            "https://opencode.ai/inference/go/openai/v1",
            "fingerprint-a",
            "declare-short",
        );
        short.statement = "short";
        assert!(db.declare_provider_extra_usage(short).await.is_err());
        db.declare_provider_extra_usage(declaration(
            "opencode-go",
            "https://opencode.ai/inference/go/openai/v1",
            "fingerprint-a",
            "declare-bound",
        ))
        .await
        .unwrap();
        assert!(
            db.declare_provider_extra_usage(declaration(
                "opencode-go",
                "https://other.example/v1",
                "fingerprint-a",
                "declare-bound",
            ))
            .await
            .is_err(),
            "an idempotency key cannot be reused for another configuration"
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }
}
