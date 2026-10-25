//! Durable, best-effort OTLP/HTTP log export for Factory events.
//!
//! The `events` ledger remains the source of truth. This module only reads it
//! in sequence order and posts sanitized OTLP JSON logs to the collector named
//! by `FACTORY_OTLP_ENDPOINT`. It never participates in Core authorization,
//! transactions or request handling. Exports use a bounded SQLite batch and a
//! bounded wake-up queue; a full queue drops only wake-up hints, which are
//! accounted for, because the next poll still reads the durable suffix. The
//! durable cursor advances only after a successful export, so a restart resumes
//! without replaying accepted records.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    },
    time::Duration,
};

use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;
use tokio::sync::{mpsc, watch};

use crate::{
    db::{Database, EVENTS_PAGE_MAX, Event},
    telemetry,
};

const DEFAULT_BATCH_LIMIT: usize = 128;
const DEFAULT_QUEUE_CAPACITY: usize = 64;
const DEFAULT_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const INITIAL_BACKOFF: Duration = Duration::from_millis(250);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const MAX_QUEUE_CAPACITY: usize = 4_096;
const MAX_ERROR_CHARS: usize = 512;
const MAX_KIND_CHARS: usize = 128;
const MAX_SUBJECT_CHARS: usize = 256;
const MAX_CORRELATION_CHARS: usize = 320;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error(
        "FACTORY_OTLP_ENDPOINT must be an absolute http(s) URL without credentials or fragments"
    )]
    Endpoint,
    #[error("OTLP HTTP client could not be initialized")]
    Client,
    #[error("OTLP exporter requires a Tokio runtime")]
    Runtime,
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("database error: {0}")]
    Database(#[from] crate::db::Error),
    #[error("OTLP request failed")]
    Request(#[source] reqwest::Error),
    #[error("OTLP collector returned HTTP {0}")]
    Upstream(u16),
}

#[derive(Clone, Debug)]
pub struct Config {
    endpoint: String,
    batch_limit: usize,
    queue_capacity: usize,
    interval: Duration,
    timeout: Duration,
}

impl Config {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            batch_limit: DEFAULT_BATCH_LIMIT,
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            interval: DEFAULT_INTERVAL,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Read `FACTORY_OTLP_ENDPOINT`; absence disables export. An empty or
    /// non-Unicode value is a configuration error, not a silent disable.
    pub fn from_env() -> Result<Option<Self>, ConfigError> {
        match std::env::var("FACTORY_OTLP_ENDPOINT") {
            Ok(endpoint) if !endpoint.trim().is_empty() => Ok(Some(Self::new(endpoint))),
            Ok(_) | Err(std::env::VarError::NotUnicode(_)) => Err(ConfigError::Endpoint),
            Err(std::env::VarError::NotPresent) => Ok(None),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct StatusSnapshot {
    pub enabled: bool,
    pub cursor: i64,
    pub exported: u64,
    pub dropped: u64,
    pub failed_attempts: u64,
    pub pending: u64,
    pub last_error: Option<String>,
}

struct ExporterStatus {
    enabled: AtomicBool,
    cursor: AtomicI64,
    exported: AtomicU64,
    dropped: AtomicU64,
    failed_attempts: AtomicU64,
    pending: AtomicU64,
    last_error: Mutex<Option<String>>,
}

impl ExporterStatus {
    fn new(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            cursor: AtomicI64::new(0),
            exported: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            failed_attempts: AtomicU64::new(0),
            pending: AtomicU64::new(0),
            last_error: Mutex::new(None),
        }
    }

    fn snapshot(&self) -> StatusSnapshot {
        StatusSnapshot {
            enabled: self.enabled.load(Ordering::Relaxed),
            cursor: self.cursor.load(Ordering::Relaxed),
            exported: self.exported.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            failed_attempts: self.failed_attempts.load(Ordering::Relaxed),
            pending: self.pending.load(Ordering::Relaxed),
            last_error: match self.last_error.lock() {
                Ok(value) => value.clone(),
                Err(poisoned) => poisoned.into_inner().clone(),
            },
        }
    }

    fn record_failure(&self, error: &ExportError) {
        self.failed_attempts.fetch_add(1, Ordering::Relaxed);
        let message = bounded_redacted(&error.to_string(), MAX_ERROR_CHARS);
        let mut guard = match self.last_error.lock() {
            Ok(value) => value,
            Err(poisoned) => poisoned.into_inner(),
        };
        *guard = Some(message);
    }

    fn clear_error(&self) {
        let mut guard = match self.last_error.lock() {
            Ok(value) => value,
            Err(poisoned) => poisoned.into_inner(),
        };
        *guard = None;
    }

    fn add_dropped(&self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }
}

/// Bounded, best-effort wake-up hint queue. Dropping a hint never drops an
/// event because the exporter always re-reads the durable cursor.
struct WakeQueue {
    sender: mpsc::Sender<()>,
    status: Arc<ExporterStatus>,
}

impl WakeQueue {
    fn notify(&self) {
        if let Err(mpsc::error::TrySendError::Full(())) = self.sender.try_send(()) {
            self.status.add_dropped();
        }
    }
}

struct Control {
    shutdown: watch::Sender<bool>,
    wake: WakeQueue,
    join: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

pub struct ExporterHandle {
    status: Arc<ExporterStatus>,
    control: Option<Control>,
}

impl ExporterHandle {
    pub fn disabled() -> Self {
        Self::with_status(ExporterStatus::new(false))
    }

    pub fn disabled_with_error(error: impl AsRef<str>) -> Self {
        let status = ExporterStatus::new(false);
        let message = bounded_redacted(error.as_ref(), MAX_ERROR_CHARS);
        if let Ok(mut guard) = status.last_error.lock() {
            *guard = Some(message);
        }
        Self::with_status(status)
    }

    fn with_status(status: ExporterStatus) -> Self {
        Self {
            status: Arc::new(status),
            control: None,
        }
    }

    pub fn status(&self) -> StatusSnapshot {
        self.status.snapshot()
    }

    /// Request an immediate poll. Safe to call from request or persistence
    /// paths; it only tries a bounded channel slot.
    pub fn notify(&self) {
        if let Some(control) = &self.control {
            control.wake.notify();
        }
    }

    pub async fn shutdown(&self) {
        let Some(control) = &self.control else {
            return;
        };
        let _ = control.shutdown.send(true);
        let mut handle = match control.join.lock() {
            Ok(mut guard) => guard.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(handle) = handle.as_mut() {
            match tokio::time::timeout(Duration::from_secs(5), &mut *handle).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    tracing::warn!(%error, "OTLP exporter task ended with an error");
                }
                Err(_) => {
                    handle.abort();
                    tracing::warn!("OTLP exporter did not stop within 5s; task aborted");
                }
            }
        }
    }
}

struct Exporter {
    db: Database,
    config: Config,
    status: Arc<ExporterStatus>,
    client: reqwest::Client,
    endpoint: reqwest::Url,
}

impl Exporter {
    fn new(db: Database, mut config: Config) -> Result<Self, ConfigError> {
        let endpoint = validate_endpoint(&config.endpoint)?;
        config.batch_limit = config.batch_limit.clamp(1, EVENTS_PAGE_MAX);
        config.queue_capacity = config.queue_capacity.clamp(1, MAX_QUEUE_CAPACITY);
        config.interval = config.interval.max(Duration::from_millis(50));
        config.timeout = config.timeout.max(Duration::from_millis(100));
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ConfigError::Client)?;
        Ok(Self {
            db,
            config,
            status: Arc::new(ExporterStatus::new(true)),
            client,
            endpoint,
        })
    }

    async fn run(self, mut shutdown: watch::Receiver<bool>, mut wake: mpsc::Receiver<()>) {
        let mut backoff = INITIAL_BACKOFF;
        loop {
            let outcome = tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                    continue;
                }
                outcome = self.export_once() => outcome,
            };
            match outcome {
                Ok(exported) if exported > 0 => backoff = INITIAL_BACKOFF,
                Ok(_) => {
                    backoff = INITIAL_BACKOFF;
                    if !wait(&mut shutdown, &mut wake, self.config.interval).await {
                        break;
                    }
                }
                Err(_) => {
                    if !wait(&mut shutdown, &mut wake, backoff).await {
                        break;
                    }
                    backoff = backoff.saturating_mul(2).min(MAX_BACKOFF);
                }
            }
        }
    }

    #[cfg(test)]
    fn status(&self) -> StatusSnapshot {
        self.status.snapshot()
    }

    async fn export_once(&self) -> Result<usize, ExportError> {
        let result = self.try_export().await;
        match &result {
            Ok(exported) if *exported > 0 => self.status.clear_error(),
            Ok(_) => {}
            Err(error) => self.status.record_failure(error),
        }
        result
    }

    async fn try_export(&self) -> Result<usize, ExportError> {
        let cursor = self.db.observability_cursor().await?;
        let events = self
            .db
            .events_after(cursor, self.config.batch_limit)
            .await?;
        let pending = self.db.event_count_after(cursor).await?;
        self.status.cursor.store(cursor, Ordering::Relaxed);
        self.status.pending.store(pending, Ordering::Relaxed);
        let Some(last) = events.last().map(|event| event.sequence) else {
            return Ok(0);
        };
        let response = self
            .client
            .post(self.endpoint.clone())
            .json(&otlp_payload(&events))
            .send()
            .await
            .map_err(ExportError::Request)?;
        if !response.status().is_success() {
            return Err(ExportError::Upstream(u16::from(response.status())));
        }
        self.db.advance_observability_cursor(last).await?;
        self.status.cursor.store(last, Ordering::Relaxed);
        self.status.pending.store(
            pending.saturating_sub(events.len() as u64),
            Ordering::Relaxed,
        );
        self.status
            .exported
            .fetch_add(events.len() as u64, Ordering::Relaxed);
        Ok(events.len())
    }
}

/// Start the background exporter. The returned handle owns the task and must be
/// shut down by Core before exit.
pub fn start(db: Database, config: Config) -> Result<ExporterHandle, ConfigError> {
    if tokio::runtime::Handle::try_current().is_err() {
        return Err(ConfigError::Runtime);
    }
    let exporter = Exporter::new(db, config)?;
    let status = Arc::clone(&exporter.status);
    let capacity = exporter.config.queue_capacity;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let (wake_tx, wake_rx) = mpsc::channel(capacity);
    let wake = WakeQueue {
        sender: wake_tx,
        status: Arc::clone(&status),
    };
    let join = tokio::spawn(exporter.run(shutdown_rx, wake_rx));
    Ok(ExporterHandle {
        status,
        control: Some(Control {
            shutdown: shutdown_tx,
            wake,
            join: Mutex::new(Some(join)),
        }),
    })
}

async fn wait(
    shutdown: &mut watch::Receiver<bool>,
    wake: &mut mpsc::Receiver<()>,
    duration: Duration,
) -> bool {
    tokio::select! {
        changed = shutdown.changed() => changed.is_ok() && !*shutdown.borrow(),
        _ = tokio::time::sleep(duration) => true,
        Some(()) = wake.recv() => true,
    }
}

fn validate_endpoint(raw: &str) -> Result<reqwest::Url, ConfigError> {
    let mut url = reqwest::Url::parse(raw.trim()).map_err(|_| ConfigError::Endpoint)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(ConfigError::Endpoint);
    }
    if url.path().is_empty() || url.path() == "/" {
        url.set_path("/v1/logs");
    }
    Ok(url)
}

fn unbounded_redacted(value: &str) -> String {
    telemetry::redact(value)
}

fn bounded_redacted(value: &str, max_chars: usize) -> String {
    unbounded_redacted(value).chars().take(max_chars).collect()
}

fn unix_nanos(recorded_at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(recorded_at)
        .ok()
        .and_then(|timestamp| timestamp.timestamp_nanos_opt())
        .map_or_else(|| "0".to_owned(), |nanos| nanos.to_string())
}

fn otlp_payload(events: &[Event]) -> Value {
    let records = events.iter().map(log_record).collect::<Vec<_>>();
    json!({
        "resourceLogs": [{
            "resource": {
                "attributes": [
                    {"key": "service.name", "value": {"stringValue": "software-factory-core"}},
                    {"key": "service.version", "value": {"stringValue": env!("CARGO_PKG_VERSION")}}
                ]
            },
            "scopeLogs": [{
                "scope": {"name": "factory.core.events", "version": "1"},
                "logRecords": records
            }]
        }]
    })
}

fn log_record(event: &Event) -> Value {
    let kind = bounded_redacted(&event.kind, MAX_KIND_CHARS);
    let subject = bounded_redacted(&event.subject_id, MAX_SUBJECT_CHARS);
    let correlation = bounded_redacted(
        &format!("{}:{}", event.kind, event.subject_id),
        MAX_CORRELATION_CHARS,
    );
    json!({
        "timeUnixNano": unix_nanos(&event.recorded_at),
        "severityNumber": 9,
        "severityText": "INFO",
        "body": {"stringValue": kind},
        "attributes": [
            {
                "key": "factory.event.sequence",
                "value": {"intValue": event.sequence.to_string()}
            },
            {
                "key": "factory.event.kind",
                "value": {"stringValue": kind}
            },
            {
                "key": "factory.event.subject_id",
                "value": {"stringValue": subject}
            },
            {
                "key": "factory.event.correlation_id",
                "value": {"stringValue": correlation}
            },
            {
                "key": "factory.event.recorded_at",
                "value": {"stringValue": bounded_redacted(&event.recorded_at, 64)}
            }
        ]
    })
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering as AtomicOrdering},
        },
    };

    use axum::{Router, body::Bytes, extract::State, http::StatusCode, routing::post};
    use serde_json::Value;

    use super::*;

    fn temp_path(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "factory-observability-{prefix}-{}.sqlite",
            uuid::Uuid::new_v4()
        ))
    }

    async fn temp_db(prefix: &str) -> (PathBuf, Database) {
        let path = temp_path(prefix);
        let db = Database::open(&path).await.unwrap();
        (path, db)
    }

    #[derive(Clone)]
    struct Capture {
        bodies: Arc<Mutex<Vec<Value>>>,
        requests: Arc<AtomicUsize>,
    }

    async fn collector(State(capture): State<Capture>, body: Bytes) -> StatusCode {
        capture.requests.fetch_add(1, AtomicOrdering::Relaxed);
        if let Ok(value) = serde_json::from_slice::<Value>(&body) {
            capture
                .bodies
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(value);
        }
        StatusCode::OK
    }

    async fn mock_collector() -> (String, Capture, tokio::task::JoinHandle<()>) {
        let capture = Capture {
            bodies: Arc::new(Mutex::new(Vec::new())),
            requests: Arc::new(AtomicUsize::new(0)),
        };
        let app = Router::new()
            .route("/v1/logs", post(collector))
            .with_state(capture.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}/v1/logs"), capture, server)
    }

    fn log_records(body: &Value) -> &Vec<Value> {
        body["resourceLogs"][0]["scopeLogs"][0]["logRecords"]
            .as_array()
            .expect("log records")
    }

    fn sequence_attribute(record: &Value) -> &str {
        record["attributes"]
            .as_array()
            .expect("attributes")
            .iter()
            .find(|attribute| attribute["key"] == "factory.event.sequence")
            .and_then(|attribute| attribute["value"]["intValue"].as_str())
            .expect("sequence attribute")
    }

    #[tokio::test]
    async fn exports_sanitized_batch_with_stable_correlation() {
        telemetry::register_secret("canary-secret-observability");
        let (path, db) = temp_db("sanitized").await;
        let first = db
            .insert_event_for_test(
                "subject-alpha",
                "action.created",
                "objective: canary-secret-observability",
            )
            .await
            .unwrap();
        let second = db
            .insert_event_for_test(
                "subject-beta",
                "campaign.created",
                "source body canary-secret-observability",
            )
            .await
            .unwrap();
        let (endpoint, capture, server) = mock_collector().await;
        let mut config = Config::new(endpoint);
        config.batch_limit = 16;
        let exporter = Exporter::new(db.clone(), config).unwrap();

        assert_eq!(exporter.export_once().await.unwrap(), 2);

        assert_eq!(capture.requests.load(AtomicOrdering::Relaxed), 1);
        let (sequences, text) = {
            let bodies = capture
                .bodies
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let records = log_records(&bodies[0]);
            assert_eq!(records.len(), 2);
            let sequences = records
                .iter()
                .map(|record| sequence_attribute(record).to_owned())
                .collect::<Vec<_>>();
            (sequences, bodies[0].to_string())
        };
        assert_eq!(sequences, vec![first.to_string(), second.to_string()]);
        assert!(!text.contains("canary-secret-observability"));
        assert!(!text.contains("source body"));
        assert!(!text.contains("objective:"));
        assert!(text.contains("subject-alpha"));
        assert!(text.contains("factory.event.correlation_id"));
        assert!(text.contains("action.created:subject-alpha"));
        assert_eq!(db.observability_cursor().await.unwrap(), second);

        let status = exporter.status();
        assert!(status.enabled);
        assert_eq!(status.exported, 2);
        assert_eq!(status.pending, 0);
        assert_eq!(status.cursor, second);
        assert_eq!(status.dropped, 0);
        assert!(status.last_error.is_none());

        server.abort();
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn batch_limit_bounds_each_export_and_accounts_pending() {
        let (path, db) = temp_db("bounded").await;
        for index in 0..5 {
            db.insert_event_for_test(&format!("subject-{index}"), "action.created", "created")
                .await
                .unwrap();
        }
        let (endpoint, capture, server) = mock_collector().await;
        let mut config = Config::new(endpoint);
        config.batch_limit = 2;
        let exporter = Exporter::new(db.clone(), config).unwrap();

        assert_eq!(exporter.export_once().await.unwrap(), 2);
        assert_eq!(exporter.status().pending, 3);
        assert_eq!(exporter.export_once().await.unwrap(), 2);
        assert_eq!(exporter.status().pending, 1);
        assert_eq!(exporter.export_once().await.unwrap(), 1);
        assert_eq!(exporter.status().pending, 0);
        assert_eq!(exporter.export_once().await.unwrap(), 0);
        assert_eq!(capture.requests.load(AtomicOrdering::Relaxed), 3);
        assert_eq!(exporter.status().exported, 5);

        server.abort();
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn full_wake_queue_drops_only_accounted_hints() {
        let status = Arc::new(ExporterStatus::new(true));
        let (sender, _receiver) = mpsc::channel(1);
        let wake = WakeQueue {
            sender,
            status: Arc::clone(&status),
        };
        wake.notify();
        wake.notify();
        assert_eq!(status.snapshot().dropped, 1);
    }

    #[tokio::test]
    async fn failed_export_keeps_cursor_and_does_not_block_writes() {
        let (path, db) = temp_db("failure").await;
        db.insert_event_for_test("subject", "action.created", "created")
            .await
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let mut config = Config::new(format!("http://{address}/v1/logs"));
        config.timeout = Duration::from_millis(250);
        let exporter = Exporter::new(db.clone(), config).unwrap();

        assert!(matches!(
            exporter.export_once().await,
            Err(ExportError::Request(_))
        ));
        let status = exporter.status();
        assert_eq!(status.exported, 0);
        assert_eq!(status.failed_attempts, 1);
        assert_eq!(status.pending, 1);
        assert!(status.last_error.is_some());
        assert_eq!(db.observability_cursor().await.unwrap(), 0);

        db.insert_event_for_test("subject", "action.created", "still accepted")
            .await
            .unwrap();
        assert_eq!(db.event_count_after(0).await.unwrap(), 2);

        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn restart_resumes_from_durable_cursor_without_duplicate_export() {
        let (path, db) = temp_db("restart").await;
        for index in 0..3 {
            db.insert_event_for_test(&format!("subject-{index}"), "action.created", "created")
                .await
                .unwrap();
        }
        let (endpoint, capture, server) = mock_collector().await;
        let config = Config::new(endpoint);
        let exporter = Exporter::new(db.clone(), config.clone()).unwrap();
        assert_eq!(exporter.export_once().await.unwrap(), 3);
        assert_eq!(capture.requests.load(AtomicOrdering::Relaxed), 1);
        db.close().await;

        let reopened = Database::open(&path).await.unwrap();
        assert_eq!(reopened.observability_cursor().await.unwrap(), 3);
        let restarted = Exporter::new(reopened.clone(), config).unwrap();
        assert_eq!(restarted.export_once().await.unwrap(), 0);
        assert_eq!(capture.requests.load(AtomicOrdering::Relaxed), 1);
        assert_eq!(restarted.status().exported, 0);

        reopened
            .insert_event_for_test("subject-new", "action.created", "created")
            .await
            .unwrap();
        assert_eq!(restarted.export_once().await.unwrap(), 1);
        assert_eq!(capture.requests.load(AtomicOrdering::Relaxed), 2);

        server.abort();
        reopened.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_credentialed_or_unsafe_endpoints() {
        for endpoint in [
            "ftp://collector.invalid/v1/logs",
            "http://user:password@collector.invalid/v1/logs",
            "http://collector.invalid/v1/logs#fragment",
            "not-a-url",
        ] {
            assert!(matches!(
                validate_endpoint(endpoint),
                Err(ConfigError::Endpoint)
            ));
        }
        let url = validate_endpoint("http://collector.invalid").unwrap();
        assert_eq!(url.path(), "/v1/logs");
    }
}
