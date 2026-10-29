//! Atomic global access revocation shared by device sessions and enrollment.
//!
//! A single revocation transaction must (1) advance the persistent access
//! epoch, (2) revoke every active device session and (3) invalidate every
//! outstanding pairing approval. Keeping the three effects in one SQLite
//! transaction makes a concurrent pairing exchange either happen entirely
//! before the revocation (and therefore get revoked) or entirely after it (and
//! therefore fail the epoch check).

use super::Error;
use sqlx_sqlite::SqliteConnection;

/// Read the current access epoch inside a write transaction.
pub(super) async fn access_epoch(connection: &mut SqliteConnection) -> Result<i64, Error> {
    Ok(
        sqlx::query_scalar::query_scalar("SELECT epoch FROM access_security_epoch WHERE id=1")
            .fetch_one(connection)
            .await?,
    )
}

/// Advance the access epoch so previously approved enrollment authority expires.
pub(super) async fn advance_epoch(connection: &mut SqliteConnection) -> Result<i64, Error> {
    sqlx::query::query("UPDATE access_security_epoch SET epoch=epoch+1 WHERE id=1")
        .execute(&mut *connection)
        .await?;
    access_epoch(connection).await
}

/// Revoke every active device session and record one audit row per session.
pub(super) async fn revoke_active_sessions(
    connection: &mut SqliteConnection,
    actor: &str,
    now_ms: i64,
) -> Result<u64, Error> {
    let ids: Vec<String> = sqlx::query_scalar::query_scalar(
        "SELECT id FROM device_sessions WHERE revoked_at_ms IS NULL",
    )
    .fetch_all(&mut *connection)
    .await?;
    sqlx::query::query("UPDATE device_sessions SET revoked_at_ms=?1 WHERE revoked_at_ms IS NULL")
        .bind(now_ms)
        .execute(&mut *connection)
        .await?;
    for id in &ids {
        sqlx::query::query(
            "INSERT INTO device_session_audit(session_id,actor,action,recorded_at_ms)
             VALUES (?1,?2,'revoked_all',?3)",
        )
        .bind(id)
        .bind(actor)
        .bind(now_ms)
        .execute(&mut *connection)
        .await?;
    }
    Ok(ids.len() as u64)
}

/// Cancel pending, available and approved pairing challenges.
pub(super) async fn invalidate_outstanding_pairings(
    connection: &mut SqliteConnection,
    actor: &str,
    now_ms: i64,
) -> Result<u64, Error> {
    let ids: Vec<String> = sqlx::query_scalar::query_scalar(
        "SELECT id FROM device_pairing_challenges
         WHERE status IN ('AVAILABLE','PENDING','APPROVED')",
    )
    .fetch_all(&mut *connection)
    .await?;
    sqlx::query::query(
        "UPDATE device_pairing_challenges SET status='CANCELLED',decided_at_ms=?1
         WHERE status IN ('AVAILABLE','PENDING','APPROVED')",
    )
    .bind(now_ms)
    .execute(&mut *connection)
    .await?;
    for id in &ids {
        sqlx::query::query(
            "INSERT INTO device_pairing_audit(challenge_id,action,actor,occurred_at_ms)
             VALUES (?1,'cancelled',?2,?3)",
        )
        .bind(id)
        .bind(actor)
        .bind(now_ms)
        .execute(&mut *connection)
        .await?;
    }
    Ok(ids.len() as u64)
}
