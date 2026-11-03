//! Durable effect journal and callback inbox.
//!
//! Infinity guarantees that a dispatched tool call is durable before the tool
//! runs, and deduplicates tool results against conversation history. It does
//! not guarantee that an external side effect happened exactly once. This
//! module adds the missing boundary:
//!
//! ```text
//! INTENT -> STARTED -> APPLIED -> COMPLETED
//!                  \-> AMBIGUOUS (never retried automatically)
//! ```
//!
//! `APPLIED` means the external marker exists and its hash is recorded, but
//! the process may die before the callback is delivered. Recovery verifies the
//! marker, promotes the effect to `COMPLETED`, and re-delivers its callback
//! under the original `(effect_key, attempt)` identity. A stale callback from
//! an older attempt is rejected by a fencing check.
//!
//! The callback inbox uses a unique `(effect_key, attempt)` key and an atomic
//! `PENDING -> DELIVERING` claim, so concurrent receivers cannot deliver the
//! same result twice.

use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::store::{SqliteStore, StoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EffectState {
    Intent,
    Started,
    Applied,
    Completed,
    Ambiguous,
    Failed,
    Cancelled,
}

impl EffectState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Intent => "INTENT",
            Self::Started => "STARTED",
            Self::Applied => "APPLIED",
            Self::Completed => "COMPLETED",
            Self::Ambiguous => "AMBIGUOUS",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
        }
    }

    fn parse(value: &str) -> Result<Self, StoreError> {
        Ok(match value {
            "INTENT" => Self::Intent,
            "STARTED" => Self::Started,
            "APPLIED" => Self::Applied,
            "COMPLETED" => Self::Completed,
            "AMBIGUOUS" => Self::Ambiguous,
            "FAILED" => Self::Failed,
            "CANCELLED" => Self::Cancelled,
            other => return Err(StoreError::Invalid(format!("unknown effect state {other}"))),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectRecord {
    pub effect_key: String,
    pub run_id: String,
    pub thread_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub request_json: String,
    pub state: EffectState,
    pub attempt: i64,
    pub lease_token: Option<String>,
    pub marker_path: Option<String>,
    pub marker_sha256: Option<String>,
    pub result_json: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeginOutcome {
    /// This caller owns a fresh attempt and must perform the effect.
    New {
        attempt: i64,
        lease: String,
    },
    /// The effect is already complete; reuse the recorded result.
    Completed {
        result: String,
    },
    /// The external marker exists but the callback may still be outstanding.
    Applied {
        result: Option<String>,
    },
    /// Another attempt is in flight or the outcome is ambiguous. Never repeat.
    InFlight {
        attempt: i64,
    },
    Cancelled,
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallbackDecision {
    Accepted,
    Duplicate,
    Stale,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackRecord {
    pub callback_id: String,
    pub run_id: String,
    pub effect_key: String,
    pub thread_id: String,
    pub tool_call_id: String,
    pub attempt: i64,
    pub payload_json: String,
    pub status: String,
    pub received_at: String,
    pub delivered_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateRecord {
    pub run_id: String,
    pub candidate_sha256: String,
    pub manifest: serde_json::Value,
    pub review: Option<serde_json::Value>,
    pub created_at: String,
}

#[derive(Clone)]
pub struct EffectJournal {
    store: SqliteStore,
}

impl EffectJournal {
    pub fn new(store: SqliteStore) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &SqliteStore {
        &self.store
    }

    pub async fn start_run(
        &self,
        run_id: &str,
        objective: &str,
        root_thread_id: &str,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO runs(run_id, objective, status, root_thread_id) VALUES (?, ?, 'RUNNING', ?) \
             ON CONFLICT(run_id) DO NOTHING",
        )
        .bind(run_id)
        .bind(objective)
        .bind(root_thread_id)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    pub async fn set_run_status(&self, run_id: &str, status: &str) -> Result<(), StoreError> {
        sqlx::query("UPDATE runs SET status = ?, updated_at = ? WHERE run_id = ?")
            .bind(status)
            .bind(now())
            .bind(run_id)
            .execute(self.store.pool())
            .await?;
        Ok(())
    }

    pub async fn run_status(&self, run_id: &str) -> Result<Option<String>, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT status FROM runs WHERE run_id = ?")
                .bind(run_id)
                .fetch_optional(self.store.pool())
                .await?,
        )
    }

    pub async fn request_cancel(&self, run_id: &str) -> Result<(), StoreError> {
        sqlx::query("UPDATE runs SET status = 'CANCELLING', updated_at = ? WHERE run_id = ?")
            .bind(now())
            .bind(run_id)
            .execute(self.store.pool())
            .await?;
        Ok(())
    }

    pub async fn is_cancelled(&self, run_id: &str) -> Result<bool, StoreError> {
        let status: Option<String> = sqlx::query_scalar("SELECT status FROM runs WHERE run_id = ?")
            .bind(run_id)
            .fetch_optional(self.store.pool())
            .await?;
        Ok(matches!(
            status.as_deref(),
            Some("CANCELLING" | "CANCELLED")
        ))
    }

    /// Attempt to claim an effect. The `(run_id, tool_call_id)` pair is the
    /// identity: retries and post-restart callbacks resolve to one row.
    pub async fn begin_effect(
        &self,
        run_id: &str,
        thread_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        request_json: &str,
    ) -> Result<(String, BeginOutcome), StoreError> {
        let effect_key = format!("{run_id}:{tool_call_id}");
        let _guard = self.store.write_guard().await;
        let mut tx = self.store.begin_immediate().await?;
        let outcome: Result<(BeginOutcome, String), StoreError> = async {
            if self.is_cancelled_tx(&mut tx, run_id).await? {
                return Ok((BeginOutcome::Cancelled, effect_key.clone()));
            }
            let existing = sqlx::query("SELECT * FROM effects WHERE effect_key = ?")
                .bind(&effect_key)
                .fetch_optional(&mut *tx)
                .await?
                .map(|row| effect_from_row(&row))
                .transpose()?;
            if let Some(effect) = existing {
                let outcome = match effect.state {
                    EffectState::Completed => BeginOutcome::Completed {
                        result: effect.result_json.unwrap_or_default(),
                    },
                    EffectState::Applied => BeginOutcome::Applied {
                        result: effect.result_json,
                    },
                    EffectState::Intent | EffectState::Started => BeginOutcome::InFlight {
                        attempt: effect.attempt,
                    },
                    EffectState::Ambiguous => BeginOutcome::InFlight {
                        attempt: effect.attempt,
                    },
                    EffectState::Cancelled => BeginOutcome::Cancelled,
                    EffectState::Failed => BeginOutcome::Failed {
                        error: effect.error.unwrap_or_else(|| "effect failed".to_owned()),
                    },
                };
                return Ok((outcome, effect_key));
            }
            let lease = Uuid::new_v4().to_string();
            let now = now();
            sqlx::query(
                "INSERT INTO effects(effect_key, run_id, thread_id, tool_call_id, tool_name, \
                  request_json, state, attempt, lease_token, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, 'INTENT', 1, ?, ?, ?)",
            )
            .bind(&effect_key)
            .bind(run_id)
            .bind(thread_id)
            .bind(tool_call_id)
            .bind(tool_name)
            .bind(request_json)
            .bind(&lease)
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            Ok((BeginOutcome::New { attempt: 1, lease }, effect_key))
        }
        .await;
        match outcome {
            Ok((outcome, effect_key)) => {
                self.store.commit_immediate(&mut tx).await?;
                Ok((effect_key, outcome))
            }
            Err(error) => {
                self.store.rollback_immediate(&mut tx).await;
                Err(error)
            }
        }
    }

    async fn is_cancelled_tx(
        &self,
        tx: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
        run_id: &str,
    ) -> Result<bool, StoreError> {
        let status: Option<String> = sqlx::query_scalar("SELECT status FROM runs WHERE run_id = ?")
            .bind(run_id)
            .fetch_optional(&mut **tx)
            .await?;
        Ok(matches!(
            status.as_deref(),
            Some("CANCELLING" | "CANCELLED")
        ))
    }

    pub async fn mark_started(
        &self,
        effect_key: &str,
        attempt: i64,
        lease: &str,
    ) -> Result<bool, StoreError> {
        let rows = sqlx::query(
            "UPDATE effects SET state = 'STARTED', updated_at = ? \
             WHERE effect_key = ? AND attempt = ? AND lease_token = ? \
               AND state = 'INTENT'",
        )
        .bind(now())
        .bind(effect_key)
        .bind(attempt)
        .bind(lease)
        .execute(self.store.pool())
        .await?
        .rows_affected();
        Ok(rows == 1)
    }

    pub async fn mark_applied(
        &self,
        effect_key: &str,
        attempt: i64,
        lease: &str,
        marker_path: &str,
        marker_sha256: &str,
    ) -> Result<bool, StoreError> {
        let rows = sqlx::query(
            "UPDATE effects SET state = 'APPLIED', marker_path = ?, marker_sha256 = ?, \
              updated_at = ? WHERE effect_key = ? AND attempt = ? AND lease_token = ? \
              AND state = 'STARTED'",
        )
        .bind(marker_path)
        .bind(marker_sha256)
        .bind(now())
        .bind(effect_key)
        .bind(attempt)
        .bind(lease)
        .execute(self.store.pool())
        .await?
        .rows_affected();
        Ok(rows == 1)
    }

    pub async fn complete(
        &self,
        effect_key: &str,
        attempt: i64,
        result_json: &str,
    ) -> Result<bool, StoreError> {
        let rows = sqlx::query(
            "UPDATE effects SET state = 'COMPLETED', result_json = ?, updated_at = ? \
             WHERE effect_key = ? AND attempt = ? AND state IN ('APPLIED','STARTED')",
        )
        .bind(result_json)
        .bind(now())
        .bind(effect_key)
        .bind(attempt)
        .execute(self.store.pool())
        .await?
        .rows_affected();
        Ok(rows == 1)
    }

    pub async fn mark_ambiguous(&self, effect_key: &str, error: &str) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE effects SET state = 'AMBIGUOUS', error = ?, updated_at = ? \
             WHERE effect_key = ? AND state IN ('INTENT','STARTED','APPLIED')",
        )
        .bind(error)
        .bind(now())
        .bind(effect_key)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    /// Promote an effect to COMPLETED after a verified marker is found by
    /// restart reconciliation. Works from any in-flight or ambiguous state.
    pub async fn restore_completed(
        &self,
        effect_key: &str,
        result_json: &str,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE effects SET state = 'COMPLETED', result_json = ?, updated_at = ? \
             WHERE effect_key = ? AND state != 'CANCELLED'",
        )
        .bind(result_json)
        .bind(now())
        .bind(effect_key)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    /// Reset claims left behind by a process killed between claim and ack.
    pub async fn reset_delivering(&self, run_id: &str) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE callbacks SET status = 'PENDING' \
             WHERE run_id = ? AND status = 'DELIVERING'",
        )
        .bind(run_id)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    pub async fn mark_callback_status(
        &self,
        callback_id: &str,
        status: &str,
    ) -> Result<(), StoreError> {
        sqlx::query("UPDATE callbacks SET status = ? WHERE callback_id = ?")
            .bind(status)
            .bind(callback_id)
            .execute(self.store.pool())
            .await?;
        Ok(())
    }

    /// Record a one-time recovery result for an effect whose outcome is
    /// ambiguous. This is not a success callback: the text tells the agent
    /// that automatic replay was refused and a human must reconcile.
    pub async fn submit_recovery_callback(
        &self,
        effect_key: &str,
        payload_json: &str,
    ) -> Result<bool, StoreError> {
        let Some(effect) = self.effect(effect_key).await? else {
            return Ok(false);
        };
        let callback_id = format!("{effect_key}:recovery-{}", effect.attempt);
        let rows = sqlx::query(
            "INSERT OR IGNORE INTO callbacks(callback_id, run_id, effect_key, thread_id, \
              tool_call_id, attempt, payload_json, status) \
             VALUES (?, ?, ?, ?, ?, ?, ?, 'PENDING')",
        )
        .bind(&callback_id)
        .bind(&effect.run_id)
        .bind(effect_key)
        .bind(&effect.thread_id)
        .bind(&effect.tool_call_id)
        .bind(effect.attempt)
        .bind(payload_json)
        .execute(self.store.pool())
        .await?
        .rows_affected();
        Ok(rows == 1)
    }

    pub async fn cancel_effect(&self, effect_key: &str) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE effects SET state = 'CANCELLED', updated_at = ? \
             WHERE effect_key = ? AND state IN ('INTENT','STARTED')",
        )
        .bind(now())
        .bind(effect_key)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    pub async fn effect(&self, effect_key: &str) -> Result<Option<EffectRecord>, StoreError> {
        let row = sqlx::query("SELECT * FROM effects WHERE effect_key = ?")
            .bind(effect_key)
            .fetch_optional(self.store.pool())
            .await?;
        row.map(|row| effect_from_row(&row)).transpose()
    }

    pub async fn effects_for_run(&self, run_id: &str) -> Result<Vec<EffectRecord>, StoreError> {
        let rows = sqlx::query("SELECT * FROM effects WHERE run_id = ? ORDER BY created_at")
            .bind(run_id)
            .fetch_all(self.store.pool())
            .await?;
        rows.iter().map(effect_from_row).collect()
    }

    /// Record the callback exactly once per `(effect_key, attempt)`.
    pub async fn submit_callback(
        &self,
        effect_key: &str,
        attempt: i64,
        payload_json: &str,
    ) -> Result<CallbackDecision, StoreError> {
        let Some(effect) = self.effect(effect_key).await? else {
            return Ok(CallbackDecision::Rejected);
        };
        if effect.attempt != attempt {
            return Ok(CallbackDecision::Stale);
        }
        if !matches!(effect.state, EffectState::Completed | EffectState::Applied) {
            return Ok(CallbackDecision::Rejected);
        }
        let callback_id = format!("{effect_key}:{attempt}");
        let rows = sqlx::query(
            "INSERT OR IGNORE INTO callbacks(callback_id, run_id, effect_key, thread_id, \
              tool_call_id, attempt, payload_json, status) \
             VALUES (?, ?, ?, ?, ?, ?, ?, 'PENDING')",
        )
        .bind(&callback_id)
        .bind(&effect.run_id)
        .bind(effect_key)
        .bind(&effect.thread_id)
        .bind(&effect.tool_call_id)
        .bind(attempt)
        .bind(payload_json)
        .execute(self.store.pool())
        .await?
        .rows_affected();
        Ok(if rows == 1 {
            CallbackDecision::Accepted
        } else {
            CallbackDecision::Duplicate
        })
    }

    pub async fn claim_pending(
        &self,
        run_id: &str,
        limit: usize,
    ) -> Result<Vec<CallbackRecord>, StoreError> {
        let rows = sqlx::query(
            "SELECT * FROM callbacks WHERE run_id = ? AND status = 'PENDING' \
             ORDER BY received_at LIMIT ?",
        )
        .bind(run_id)
        .bind(limit as i64)
        .fetch_all(self.store.pool())
        .await?;
        let mut claimed = Vec::new();
        for row in rows {
            let callback = callback_from_row(&row);
            let updated = sqlx::query(
                "UPDATE callbacks SET status = 'DELIVERING' \
                 WHERE callback_id = ? AND status = 'PENDING'",
            )
            .bind(&callback.callback_id)
            .execute(self.store.pool())
            .await?
            .rows_affected();
            if updated == 1 {
                claimed.push(callback);
            }
        }
        Ok(claimed)
    }

    pub async fn mark_delivered(&self, callback_id: &str) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE callbacks SET status = 'DELIVERED', delivered_at = ? WHERE callback_id = ?",
        )
        .bind(now())
        .bind(callback_id)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    pub async fn release_callback(&self, callback_id: &str) -> Result<(), StoreError> {
        sqlx::query("UPDATE callbacks SET status = 'PENDING' WHERE callback_id = ?")
            .bind(callback_id)
            .execute(self.store.pool())
            .await?;
        Ok(())
    }

    pub async fn callbacks_for_run(&self, run_id: &str) -> Result<Vec<CallbackRecord>, StoreError> {
        let rows = sqlx::query("SELECT * FROM callbacks WHERE run_id = ? ORDER BY received_at")
            .bind(run_id)
            .fetch_all(self.store.pool())
            .await?;
        Ok(rows.iter().map(callback_from_row).collect())
    }

    pub async fn save_candidate(
        &self,
        run_id: &str,
        candidate_sha256: &str,
        manifest: &serde_json::Value,
        review: Option<&serde_json::Value>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO candidates(run_id, candidate_sha256, manifest_json, review_json) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(run_id) DO UPDATE SET candidate_sha256 = excluded.candidate_sha256, \
              manifest_json = excluded.manifest_json, review_json = excluded.review_json",
        )
        .bind(run_id)
        .bind(candidate_sha256)
        .bind(serde_json::to_string(manifest)?)
        .bind(review.map(serde_json::to_string).transpose()?)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    pub async fn candidate(&self, run_id: &str) -> Result<Option<CandidateRecord>, StoreError> {
        let row = sqlx::query("SELECT * FROM candidates WHERE run_id = ?")
            .bind(run_id)
            .fetch_optional(self.store.pool())
            .await?;
        Ok(match row {
            Some(row) => Some(CandidateRecord {
                run_id: row.get("run_id"),
                candidate_sha256: row.get("candidate_sha256"),
                manifest: serde_json::from_str(&row.get::<String, _>("manifest_json"))?,
                review: row
                    .get::<Option<String>, _>("review_json")
                    .map(|json| serde_json::from_str(&json))
                    .transpose()?,
                created_at: row.get("created_at"),
            }),
            None => None,
        })
    }

    /// Wait until no effect is in an in-flight state. Used by the harness;
    /// production recovery would not busy-wait.
    pub async fn wait_effects_settled(
        &self,
        run_id: &str,
        timeout: Duration,
    ) -> Result<bool, StoreError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let pending: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM effects WHERE run_id = ? \
                 AND state IN ('INTENT','STARTED','APPLIED')",
            )
            .bind(run_id)
            .fetch_one(self.store.pool())
            .await?;
            if pending == 0 {
                return Ok(true);
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(false);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

fn effect_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<EffectRecord, StoreError> {
    Ok(EffectRecord {
        effect_key: row.get("effect_key"),
        run_id: row.get("run_id"),
        thread_id: row.get("thread_id"),
        tool_call_id: row.get("tool_call_id"),
        tool_name: row.get("tool_name"),
        request_json: row.get("request_json"),
        state: EffectState::parse(&row.get::<String, _>("state"))?,
        attempt: row.get("attempt"),
        lease_token: row.get("lease_token"),
        marker_path: row.get("marker_path"),
        marker_sha256: row.get("marker_sha256"),
        result_json: row.get("result_json"),
        error: row.get("error"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn callback_from_row(row: &sqlx::sqlite::SqliteRow) -> CallbackRecord {
    CallbackRecord {
        callback_id: row.get("callback_id"),
        run_id: row.get("run_id"),
        effect_key: row.get("effect_key"),
        thread_id: row.get("thread_id"),
        tool_call_id: row.get("tool_call_id"),
        attempt: row.get("attempt"),
        payload_json: row.get("payload_json"),
        status: row.get("status"),
        received_at: row.get("received_at"),
        delivered_at: row.get("delivered_at"),
    }
}

pub fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
