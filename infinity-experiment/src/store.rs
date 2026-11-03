//! Durable SQLx/SQLite implementations of Infinity's platform traits.
//!
//! Design notes:
//!
//! * Every append is a single `BEGIN IMMEDIATE` transaction with a
//!   `(thread_id, dedup_id)` uniqueness constraint. A retried write is a
//!   no-op; a torn multi-message append cannot become visible.
//! * Message ordinals are per-thread and monotonic. Infinity interprets
//!   bounds as leading-row counts, which maps directly to `ORDER BY ordinal`.
//! * Thread rows carry the parent edge and spawn order, so a fresh process
//!   reconstructs independent child contexts from storage alone.
//! * The store is the same SQLite database used by the effect/callback
//!   journal, allowing effect intent and conversation state to commit in one
//!   place without a second consistency boundary.

use std::collections::HashSet;
use std::str::FromStr;
use std::time::Duration;

use async_trait::async_trait;
use infinity_agent_core::ThreadId;
use infinity_agent_core::message::InfinityMessage;
use infinity_agent_core::stores::ThreadInfo;
use infinity_agent_core::system::UserChoice;
use infinity_agent_core::traits::{ConversationStore, SpawnContext, StateStore};
use rap_protocol::{ChoiceId, ToolCallId};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};

pub const SCHEMA: &str = include_str!("../migrations/0001_init.sql");

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid durable state: {0}")]
    Invalid(String),
}

#[derive(Clone)]
pub struct SqliteStore {
    pool: SqlitePool,
    write_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
}

impl SqliteStore {
    pub async fn open(path: impl AsRef<std::path::Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(15));
        Self::from_options(options).await
    }

    pub async fn open_memory() -> Result<Self, StoreError> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?
            .journal_mode(SqliteJournalMode::Memory)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true);
        Self::from_options(options).await
    }

    async fn from_options(options: SqliteConnectOptions) -> Result<Self, StoreError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .acquire_timeout(Duration::from_secs(15))
            .connect_with(options)
            .await?;
        for statement in SCHEMA.split(";\n") {
            let statement = statement.trim();
            if statement.is_empty() {
                continue;
            }
            sqlx::query(statement).execute(&pool).await?;
        }
        Ok(Self {
            pool,
            write_lock: Default::default(),
        })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Serialize writes that must observe the current maximum ordinal.
    pub async fn write_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.write_lock.lock().await
    }

    /// Start an immediate write transaction. SQLite's deferred transactions
    /// can fail with `SQLITE_BUSY_SNAPSHOT` when a concurrent writer commits
    /// between the first read and the first write; `BEGIN IMMEDIATE` takes the
    /// write lock up front, which is what the append/spawn paths require.
    pub async fn begin_immediate(
        &self,
    ) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>, StoreError> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *connection)
            .await?;
        Ok(connection)
    }

    pub async fn commit_immediate(
        &self,
        connection: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    ) -> Result<(), StoreError> {
        sqlx::query("COMMIT").execute(&mut **connection).await?;
        Ok(())
    }

    pub async fn rollback_immediate(
        &self,
        connection: &mut sqlx::pool::PoolConnection<sqlx::Sqlite>,
    ) {
        let _ = sqlx::query("ROLLBACK").execute(&mut **connection).await;
    }

    pub async fn thread_info(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<Option<ThreadInfo>, StoreError> {
        let row = sqlx::query(
            "SELECT parent_thread_id, root_thread_id, spawn_message_order, \
                    spawn_tool_call_id, closed, is_subscription_event, is_compaction, fresh_context \
             FROM threads WHERE thread_id = ?",
        )
        .bind(thread_id.as_str())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| ThreadInfo {
            parent_thread_id: row
                .get::<Option<String>, _>("parent_thread_id")
                .map(ThreadId::from),
            root_thread_id: ThreadId::from(row.get::<String, _>("root_thread_id")),
            spawn_message_order: row.get("spawn_message_order"),
            spawn_tool_call_id: row
                .get::<Option<String>, _>("spawn_tool_call_id")
                .map(ToolCallId::from),
            closed: row.get::<i64, _>("closed") != 0,
            is_subscription_event: row.get::<i64, _>("is_subscription_event") != 0,
            is_compaction: row.get::<i64, _>("is_compaction") != 0,
            fresh_context: row.get::<i64, _>("fresh_context") != 0,
        }))
    }

    pub async fn all_thread_ids(&self) -> Result<Vec<ThreadId>, StoreError> {
        let rows = sqlx::query("SELECT thread_id FROM threads ORDER BY rowid")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| ThreadId::from(row.get::<String, _>("thread_id")))
            .collect())
    }

    /// Direct children of a thread, oldest first.
    pub async fn children(&self, thread_id: &ThreadId<str>) -> Result<Vec<ThreadId>, StoreError> {
        let rows =
            sqlx::query("SELECT thread_id FROM threads WHERE parent_thread_id = ? ORDER BY rowid")
                .bind(thread_id.as_str())
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|row| ThreadId::from(row.get::<String, _>("thread_id")))
            .collect())
    }

    /// Every descendant of `root_id`, oldest first, including the root.
    pub async fn subtree(&self, root_id: &ThreadId<str>) -> Result<Vec<ThreadId>, StoreError> {
        let rows = sqlx::query(
            "WITH RECURSIVE tree(thread_id) AS ( \
                 SELECT thread_id FROM threads WHERE thread_id = ? \
                 UNION ALL \
                 SELECT t.thread_id FROM threads t JOIN tree ON t.parent_thread_id = tree.thread_id \
             ) SELECT thread_id FROM tree ORDER BY thread_id",
        )
        .bind(root_id.as_str())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| ThreadId::from(row.get::<String, _>("thread_id")))
            .collect())
    }

    pub async fn mark_stopped(&self, thread_id: &ThreadId<str>) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO thread_stops(thread_id, stopped) VALUES (?, 1) \
                     ON CONFLICT(thread_id) DO UPDATE SET stopped = 1",
        )
        .bind(thread_id.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn clear_stopped(&self, thread_id: &ThreadId<str>) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM thread_stops WHERE thread_id = ?")
            .bind(thread_id.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn append_event(
        &self,
        run_id: &str,
        thread_id: &ThreadId<str>,
        kind: &str,
        payload: &serde_json::Value,
    ) -> Result<(), StoreError> {
        let payload_json = serde_json::to_string(payload)?;
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest;
        hasher.update(run_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(thread_id.as_str().as_bytes());
        hasher.update(b"\0");
        hasher.update(kind.as_bytes());
        hasher.update(b"\0");
        hasher.update(payload_json.as_bytes());
        let event_key = hex::encode(hasher.finalize());
        sqlx::query(
            "INSERT OR IGNORE INTO agent_events(event_key, run_id, thread_id, kind, payload_json) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(event_key)
        .bind(run_id)
        .bind(thread_id.as_str())
        .bind(kind)
        .bind(payload_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn events(&self, run_id: &str) -> Result<Vec<AgentEventRow>, StoreError> {
        let rows = sqlx::query(
            "SELECT seq, thread_id, kind, payload_json, created_at \
             FROM agent_events WHERE run_id = ? ORDER BY seq",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(AgentEventRow {
                    seq: row.get("seq"),
                    thread_id: ThreadId::from(row.get::<String, _>("thread_id")),
                    kind: row.get("kind"),
                    payload: serde_json::from_str(&row.get::<String, _>("payload_json"))?,
                    created_at: row.get("created_at"),
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentEventRow {
    pub seq: i64,
    pub thread_id: ThreadId,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: String,
}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        StoreError::Invalid(format!("filesystem: {error}"))
    }
}

impl From<sqlx::migrate::MigrateError> for StoreError {
    fn from(error: sqlx::migrate::MigrateError) -> Self {
        StoreError::Invalid(format!("migration: {error}"))
    }
}

#[async_trait]
impl ConversationStore for SqliteStore {
    type Error = StoreError;

    async fn ensure_root_thread(&self, thread_id: &ThreadId<str>) -> Result<(), Self::Error> {
        let _guard = self.write_guard().await;
        sqlx::query("INSERT OR IGNORE INTO threads(thread_id, root_thread_id) VALUES (?, ?)")
            .bind(thread_id.as_str())
            .bind(thread_id.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn thread_exists(&self, thread_id: &ThreadId<str>) -> Result<bool, Self::Error> {
        let row = sqlx::query("SELECT 1 FROM threads WHERE thread_id = ?")
            .bind(thread_id.as_str())
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.is_some())
    }

    async fn load_history_up_to(
        &self,
        session_id: &ThreadId<str>,
        start_from: Option<i64>,
        up_to: Option<i64>,
    ) -> Result<Vec<InfinityMessage>, Self::Error> {
        let start = start_from.unwrap_or(0).max(0);
        let limit = match up_to {
            Some(end) => (end.max(start) - start).max(0),
            None => -1,
        };
        let rows = sqlx::query(
            "SELECT message_json FROM messages WHERE thread_id = ? \
             ORDER BY ordinal LIMIT ? OFFSET ?",
        )
        .bind(session_id.as_str())
        .bind(limit)
        .bind(start)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let json: String = row.get("message_json");
                Ok(serde_json::from_str(&json)?)
            })
            .collect()
    }

    async fn append_messages(
        &self,
        session_id: &ThreadId<str>,
        messages: Vec<(InfinityMessage, String)>,
    ) -> Result<(), Self::Error> {
        if messages.is_empty() {
            return Ok(());
        }
        let _guard = self.write_guard().await;
        let mut tx = self.begin_immediate().await?;
        let outcome: Result<(), StoreError> = async {
            let next: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM messages WHERE thread_id = ?",
            )
            .bind(session_id.as_str())
            .fetch_one(&mut *tx)
            .await?;
            for (offset, (message, dedup_id)) in messages.into_iter().enumerate() {
                let json = serde_json::to_string(&message)?;
                sqlx::query(
                    "INSERT OR IGNORE INTO messages(thread_id, ordinal, message_json, dedup_id) \
                     VALUES (?, ?, ?, ?)",
                )
                .bind(session_id.as_str())
                .bind(next + offset as i64)
                .bind(json)
                .bind(dedup_id)
                .execute(&mut *tx)
                .await?;
            }
            Ok(())
        }
        .await;
        match outcome {
            Ok(()) => {
                self.commit_immediate(&mut tx).await?;
                Ok(())
            }
            Err(error) => {
                self.rollback_immediate(&mut tx).await;
                Err(error)
            }
        }
    }

    async fn spawn_thread(
        &self,
        parent_thread_id: &ThreadId<str>,
        spawn_tool_call_id: &ToolCallId<str>,
        is_for_subscription_event: bool,
        context: SpawnContext,
    ) -> Result<ThreadId, Self::Error> {
        let new_id = ThreadId::from(uuid::Uuid::new_v4().to_string());
        let (spawn_order, fresh) = match context {
            SpawnContext::InheritUpTo(order) => (order as i64, false),
            SpawnContext::Fresh => (0, true),
        };
        let _guard = self.write_guard().await;
        let mut tx = self.begin_immediate().await?;
        let outcome: Result<(), StoreError> = async {
            let parent = sqlx::query("SELECT root_thread_id FROM threads WHERE thread_id = ?")
                .bind(parent_thread_id.as_str())
                .fetch_optional(&mut *tx)
                .await?;
            let root = parent
                .map(|row| row.get::<String, _>("root_thread_id"))
                .unwrap_or_else(|| parent_thread_id.as_str().to_owned());
            sqlx::query(
                "INSERT INTO threads(thread_id, parent_thread_id, root_thread_id, \
                  spawn_message_order, spawn_tool_call_id, is_subscription_event, fresh_context) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(new_id.as_str())
            .bind(parent_thread_id.as_str())
            .bind(root)
            .bind(spawn_order)
            .bind(spawn_tool_call_id.as_str())
            .bind(is_for_subscription_event as i64)
            .bind(fresh as i64)
            .execute(&mut *tx)
            .await?;
            Ok(())
        }
        .await;
        match outcome {
            Ok(()) => {
                self.commit_immediate(&mut tx).await?;
                Ok(new_id)
            }
            Err(error) => {
                self.rollback_immediate(&mut tx).await;
                Err(error)
            }
        }
    }

    async fn is_thread_closed(&self, thread_id: &ThreadId<str>) -> Result<bool, Self::Error> {
        let closed: Option<i64> =
            sqlx::query_scalar("SELECT closed FROM threads WHERE thread_id = ?")
                .bind(thread_id.as_str())
                .fetch_optional(&self.pool)
                .await?;
        Ok(closed.unwrap_or(0) != 0)
    }

    async fn close_thread(&self, thread_id: &ThreadId<str>) -> Result<(), Self::Error> {
        sqlx::query("UPDATE threads SET closed = 1 WHERE thread_id = ?")
            .bind(thread_id.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn is_subscription_event_thread(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<bool, Self::Error> {
        let value: Option<i64> =
            sqlx::query_scalar("SELECT is_subscription_event FROM threads WHERE thread_id = ?")
                .bind(thread_id.as_str())
                .fetch_optional(&self.pool)
                .await?;
        Ok(value.unwrap_or(0) != 0)
    }

    async fn is_fresh_context_thread(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<bool, Self::Error> {
        let value: Option<i64> =
            sqlx::query_scalar("SELECT fresh_context FROM threads WHERE thread_id = ?")
                .bind(thread_id.as_str())
                .fetch_optional(&self.pool)
                .await?;
        Ok(value.unwrap_or(0) != 0)
    }

    async fn get_thread_parent_info(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<Option<(ThreadId, ToolCallId)>, Self::Error> {
        let row = sqlx::query(
            "SELECT parent_thread_id, spawn_tool_call_id FROM threads WHERE thread_id = ?",
        )
        .bind(thread_id.as_str())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|row| {
            let parent: Option<String> = row.get("parent_thread_id");
            let call: Option<String> = row.get("spawn_tool_call_id");
            parent
                .zip(call)
                .map(|(p, c)| (ThreadId::from(p), ToolCallId::from(c)))
        }))
    }

    async fn get_ancestor_chain(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<Vec<(ThreadId, i64)>, Self::Error> {
        let mut result = Vec::new();
        let mut current = thread_id.as_str().to_owned();
        loop {
            let row = sqlx::query(
                "SELECT parent_thread_id, spawn_message_order FROM threads WHERE thread_id = ?",
            )
            .bind(&current)
            .fetch_optional(&self.pool)
            .await?;
            let Some(row) = row else { break };
            let Some(parent) = row.get::<Option<String>, _>("parent_thread_id") else {
                break;
            };
            let order: i64 = row
                .get::<Option<i64>, _>("spawn_message_order")
                .unwrap_or(0);
            result.push((ThreadId::from(parent.clone()), order));
            current = parent;
        }
        result.reverse();
        Ok(result)
    }

    async fn mark_thread_as_compaction(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<(), Self::Error> {
        sqlx::query("UPDATE threads SET is_compaction = 1 WHERE thread_id = ?")
            .bind(thread_id.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn is_compaction_thread(&self, thread_id: &ThreadId<str>) -> Result<bool, Self::Error> {
        let value: Option<i64> =
            sqlx::query_scalar("SELECT is_compaction FROM threads WHERE thread_id = ?")
                .bind(thread_id.as_str())
                .fetch_optional(&self.pool)
                .await?;
        Ok(value.unwrap_or(0) != 0)
    }

    async fn get_thread_spawn_order(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<Option<i64>, Self::Error> {
        let value: Option<i64> =
            sqlx::query_scalar("SELECT spawn_message_order FROM threads WHERE thread_id = ?")
                .bind(thread_id.as_str())
                .fetch_optional(&self.pool)
                .await?;
        Ok(value)
    }

    async fn save_compaction_summary(
        &self,
        thread_id: &ThreadId<str>,
        summary: &str,
        up_to_order: i64,
    ) -> Result<(), Self::Error> {
        sqlx::query(
            "INSERT INTO compaction_summaries(thread_id, summary, up_to_order) VALUES (?, ?, ?)",
        )
        .bind(thread_id.as_str())
        .bind(summary)
        .bind(up_to_order)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load_latest_compaction_summary_up_to(
        &self,
        thread_id: &ThreadId<str>,
        up_to_order: Option<i64>,
    ) -> Result<Option<(String, i64)>, Self::Error> {
        let row = sqlx::query(
            "SELECT summary, up_to_order FROM compaction_summaries \
             WHERE thread_id = ? AND (? IS NULL OR up_to_order <= ?) \
             ORDER BY id DESC LIMIT 1",
        )
        .bind(thread_id.as_str())
        .bind(up_to_order)
        .bind(up_to_order)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| (row.get("summary"), row.get("up_to_order"))))
    }
}

#[async_trait]
impl StateStore for SqliteStore {
    type Error = StoreError;

    async fn get_processed_ids(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<HashSet<String>, Self::Error> {
        let rows = sqlx::query("SELECT message_id FROM processed_ids WHERE thread_id = ?")
            .bind(thread_id.as_str())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| row.get::<String, _>("message_id"))
            .collect())
    }

    async fn add_processed_message_ids(
        &self,
        thread_id: &ThreadId<str>,
        message_ids: Vec<String>,
    ) -> Result<(), Self::Error> {
        if message_ids.is_empty() {
            return Ok(());
        }
        let _guard = self.write_guard().await;
        let mut tx = self.begin_immediate().await?;
        let outcome: Result<(), StoreError> = async {
            for message_id in message_ids {
                sqlx::query(
                    "INSERT OR IGNORE INTO processed_ids(thread_id, message_id) VALUES (?, ?)",
                )
                .bind(thread_id.as_str())
                .bind(message_id)
                .execute(&mut *tx)
                .await?;
            }
            Ok(())
        }
        .await;
        match outcome {
            Ok(()) => {
                self.commit_immediate(&mut tx).await?;
                Ok(())
            }
            Err(error) => {
                self.rollback_immediate(&mut tx).await;
                Err(error)
            }
        }
    }

    async fn get_metadata(
        &self,
        root_thread_id: &ThreadId<str>,
    ) -> Result<Option<serde_json::Value>, Self::Error> {
        let json: Option<String> =
            sqlx::query_scalar("SELECT metadata_json FROM thread_metadata WHERE thread_id = ?")
                .bind(root_thread_id.as_str())
                .fetch_optional(&self.pool)
                .await?;
        Ok(match json {
            Some(json) => Some(serde_json::from_str(&json)?),
            None => None,
        })
    }

    async fn set_metadata(
        &self,
        root_thread_id: &ThreadId<str>,
        metadata: serde_json::Value,
    ) -> Result<(), Self::Error> {
        sqlx::query(
            "INSERT INTO thread_metadata(thread_id, metadata_json) VALUES (?, ?) \
             ON CONFLICT(thread_id) DO UPDATE SET metadata_json = excluded.metadata_json",
        )
        .bind(root_thread_id.as_str())
        .bind(serde_json::to_string(&metadata)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_active_subscriptions(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<Vec<ToolCallId>, Self::Error> {
        let rows = sqlx::query("SELECT tool_call_id FROM subscriptions WHERE thread_id = ?")
            .bind(thread_id.as_str())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| ToolCallId::from(row.get::<String, _>("tool_call_id")))
            .collect())
    }

    async fn add_active_subscription(
        &self,
        thread_id: &ThreadId<str>,
        tool_call_id: &ToolCallId<str>,
    ) -> Result<(), Self::Error> {
        sqlx::query("INSERT OR IGNORE INTO subscriptions(thread_id, tool_call_id) VALUES (?, ?)")
            .bind(thread_id.as_str())
            .bind(tool_call_id.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn remove_active_subscription(
        &self,
        thread_id: &ThreadId<str>,
        tool_call_id: &ToolCallId<str>,
    ) -> Result<(), Self::Error> {
        sqlx::query("DELETE FROM subscriptions WHERE thread_id = ? AND tool_call_id = ?")
            .bind(thread_id.as_str())
            .bind(tool_call_id.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn add_pending_user_choice(
        &self,
        thread_id: &ThreadId<str>,
        choice: UserChoice,
    ) -> Result<(), Self::Error> {
        sqlx::query(
            "INSERT INTO pending_choices(thread_id, choice_id, choice_json) VALUES (?, ?, ?) \
             ON CONFLICT(thread_id, choice_id) DO UPDATE SET choice_json = excluded.choice_json",
        )
        .bind(thread_id.as_str())
        .bind(choice.id.as_str())
        .bind(serde_json::to_string(&choice)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn remove_pending_user_choice(
        &self,
        thread_id: &ThreadId<str>,
        choice_id: &ChoiceId<str>,
    ) -> Result<(), Self::Error> {
        sqlx::query("DELETE FROM pending_choices WHERE thread_id = ? AND choice_id = ?")
            .bind(thread_id.as_str())
            .bind(choice_id.as_str())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_pending_user_choices(
        &self,
        thread_id: &ThreadId<str>,
    ) -> Result<Vec<UserChoice>, Self::Error> {
        let rows = sqlx::query("SELECT choice_json FROM pending_choices WHERE thread_id = ?")
            .bind(thread_id.as_str())
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                let json: String = row.get("choice_json");
                Ok(serde_json::from_str(&json)?)
            })
            .collect()
    }

    async fn is_thread_stopped(&self, thread_id: &ThreadId<str>) -> Result<bool, Self::Error> {
        let stopped: Option<i64> =
            sqlx::query_scalar("SELECT stopped FROM thread_stops WHERE thread_id = ?")
                .bind(thread_id.as_str())
                .fetch_optional(&self.pool)
                .await?;
        Ok(stopped.unwrap_or(0) != 0)
    }
}
