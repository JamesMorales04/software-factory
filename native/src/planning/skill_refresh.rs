//! Deterministic technology skill-pack refresh policy and Core scheduler.
//!
//! The scheduler is advisory maintenance only. It can persist a curated builtin
//! revision when the persisted revision is due and the builtin content is newer
//! and different. It never extends a review date in place, invents a source,
//! approves a plan, lowers a planning lane, hides a stale pack or grants
//! authority.

use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, OnceLock},
    time::Duration,
};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, watch};

use crate::{
    agentic::detect_stack,
    db::{Database, Error},
    planning::skills::{
        BUILTIN_TECHNOLOGIES, MAX_SKILL_PACK_STATUSES, SkillPackState, SkillPackStatus,
        builtin_skill_pack, classify_skill_packs, required_technologies,
    },
};

pub const SKILL_REFRESH_INTERVAL_ENV: &str = "FACTORY_SKILL_REFRESH_INTERVAL_SECONDS";
pub const DEFAULT_SKILL_REFRESH_INTERVAL_SECONDS: u64 = 24 * 60 * 60;
pub const MIN_SKILL_REFRESH_INTERVAL_SECONDS: u64 = 60 * 60;
pub const MAX_SKILL_REFRESH_INTERVAL_SECONDS: u64 = 7 * 24 * 60 * 60;
const MAX_PROJECTS_PER_PASS: usize = 64;
const MAX_EVENT_DETAIL_CHARS: usize = 700;

const EVENT_REFRESH_DUE: &str = "skill.pack.refresh_due";
const EVENT_REFRESHED: &str = "skill.pack.refreshed";
const EVENT_REFRESH_BLOCKED: &str = "skill.pack.refresh_blocked";

static PASS_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static SCHEDULER: OnceLock<SkillRefreshHandle> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillRefreshConfigError {
    #[error(
        "FACTORY_SKILL_REFRESH_INTERVAL_SECONDS must be an integer between 3600 and 604800 seconds"
    )]
    InvalidInterval,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRefreshConfig {
    interval_seconds: u64,
}

impl SkillRefreshConfig {
    pub fn from_env() -> Result<Self, SkillRefreshConfigError> {
        let value = std::env::var(SKILL_REFRESH_INTERVAL_ENV).ok();
        Self::from_seconds(value.as_deref())
    }

    pub fn from_seconds(value: Option<&str>) -> Result<Self, SkillRefreshConfigError> {
        let interval_seconds = match value {
            None => DEFAULT_SKILL_REFRESH_INTERVAL_SECONDS,
            Some(value) => value
                .trim()
                .parse::<u64>()
                .map_err(|_| SkillRefreshConfigError::InvalidInterval)?,
        };
        if !(MIN_SKILL_REFRESH_INTERVAL_SECONDS..=MAX_SKILL_REFRESH_INTERVAL_SECONDS)
            .contains(&interval_seconds)
        {
            return Err(SkillRefreshConfigError::InvalidInterval);
        }
        Ok(Self { interval_seconds })
    }

    pub const fn interval_seconds(&self) -> u64 {
        self.interval_seconds
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRefreshReport {
    pub started_at: String,
    pub finished_at: String,
    pub checked: usize,
    pub projects_checked: usize,
    pub projects_skipped: usize,
    pub project_total: usize,
    pub refreshed: Vec<String>,
    pub due: Vec<String>,
    pub blocked: Vec<String>,
    pub failed: Vec<String>,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRefreshRuntime {
    pub running: bool,
    pub interval_seconds: u64,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub last_outcome: Option<SkillRefreshReport>,
    pub last_error: Option<String>,
}

impl SkillRefreshRuntime {
    pub fn not_started() -> Self {
        Self {
            running: false,
            interval_seconds: DEFAULT_SKILL_REFRESH_INTERVAL_SECONDS,
            last_run_at: None,
            next_run_at: None,
            last_outcome: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRefreshStatus {
    pub running: bool,
    pub interval_seconds: u64,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub last_outcome: Option<SkillRefreshReport>,
    pub last_error: Option<String>,
    pub packs: Vec<SkillPackStatus>,
    pub due: usize,
    pub blocked: usize,
    pub truncated: bool,
    pub project_offset: usize,
    pub projects_checked: usize,
    pub projects_skipped: usize,
    pub project_total: usize,
}

impl SkillRefreshStatus {
    fn from_parts(
        runtime: SkillRefreshRuntime,
        packs: Vec<SkillPackStatus>,
        truncated: bool,
        projects: ProjectWindowStatus,
    ) -> Self {
        let due = packs
            .iter()
            .filter(|pack| {
                matches!(
                    pack.state,
                    SkillPackState::Refreshable | SkillPackState::Due
                )
            })
            .count();
        let blocked = packs.iter().filter(|pack| pack.blocking).count();
        Self {
            running: runtime.running,
            interval_seconds: runtime.interval_seconds,
            last_run_at: runtime.last_run_at,
            next_run_at: runtime.next_run_at,
            last_outcome: runtime.last_outcome,
            last_error: runtime.last_error,
            packs,
            due,
            blocked,
            truncated,
            project_offset: projects.offset,
            projects_checked: projects.checked,
            projects_skipped: projects.skipped,
            project_total: projects.total,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProjectWindowStatus {
    offset: usize,
    checked: usize,
    skipped: usize,
    total: usize,
}

#[derive(Debug)]
struct TargetSnapshot {
    persisted: Vec<crate::planning::SkillPack>,
    technologies: Vec<String>,
    technologies_truncated: bool,
    projects: ProjectWindowStatus,
    next_project_offset: usize,
}

#[derive(Clone)]
pub struct SkillRefreshHandle {
    interval_seconds: u64,
    state: Arc<Mutex<SkillRefreshRuntime>>,
    shutdown: watch::Sender<bool>,
    join: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl SkillRefreshHandle {
    pub fn start(db: Database, config: SkillRefreshConfig) -> Self {
        let (shutdown, shutdown_rx) = watch::channel(false);
        let state = Arc::new(Mutex::new(SkillRefreshRuntime {
            running: true,
            interval_seconds: config.interval_seconds,
            last_run_at: None,
            next_run_at: None,
            last_outcome: None,
            last_error: None,
        }));
        let join = Arc::new(Mutex::new(None));
        let handle = Self {
            interval_seconds: config.interval_seconds,
            state: Arc::clone(&state),
            shutdown,
            join: Arc::clone(&join),
        };
        let task = tokio::spawn(run_loop(db, config.interval_seconds, shutdown_rx, state));
        *join.try_lock().expect("new scheduler join slot") = Some(task);
        handle
    }

    pub async fn runtime(&self) -> SkillRefreshRuntime {
        self.state.lock().await.clone()
    }

    pub async fn run_now(&self, db: &Database, now: &str) -> Result<SkillRefreshReport, Error> {
        execute_pass(db, self.interval_seconds, &self.state, now).await
    }

    /// Cancel the single scheduler task and wait for it to exit. This is
    /// idempotent and safe to call before the database closes.
    pub async fn shutdown(&self) {
        let _ = self.shutdown.send(true);
        if let Some(join) = self.join.lock().await.take() {
            let _ = join.await;
        }
    }
}

async fn run_loop(
    db: Database,
    interval_seconds: u64,
    mut shutdown: watch::Receiver<bool>,
    state: Arc<Mutex<SkillRefreshRuntime>>,
) {
    loop {
        if *shutdown.borrow() {
            break;
        }
        let started_at = Utc::now().to_rfc3339();
        if let Err(error) = execute_pass(&db, interval_seconds, &state, &started_at).await {
            tracing::warn!(
                error = %bounded(&error.to_string(), MAX_EVENT_DETAIL_CHARS),
                "skill-pack refresh pass failed"
            );
        }
        let sleep = tokio::time::sleep(Duration::from_secs(interval_seconds));
        tokio::pin!(sleep);
        tokio::select! {
            _ = &mut sleep => {}
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
    }
    let mut runtime = state.lock().await;
    runtime.running = false;
    runtime.next_run_at = None;
}

async fn execute_pass(
    db: &Database,
    interval_seconds: u64,
    state: &Arc<Mutex<SkillRefreshRuntime>>,
    started_at: &str,
) -> Result<SkillRefreshReport, Error> {
    let result = run_pass(db, started_at).await;
    let mut runtime = state.lock().await;
    runtime.last_run_at = Some(Utc::now().to_rfc3339());
    runtime.next_run_at = Some(
        (Utc::now()
            + chrono::Duration::seconds(i64::try_from(interval_seconds).unwrap_or(i64::MAX)))
        .to_rfc3339(),
    );
    match &result {
        Ok(report) => {
            runtime.last_outcome = Some(report.clone());
            runtime.last_error = None;
        }
        Err(error) => {
            runtime.last_outcome = None;
            runtime.last_error = Some(bounded(&error.to_string(), MAX_EVENT_DETAIL_CHARS));
        }
    }
    result
}

/// Run one deterministic refresh pass. Duplicate passes are serialized and
/// event insertion is idempotent, so a retry or concurrent interval tick cannot
/// create duplicate durable effects.
pub async fn run_pass(db: &Database, now: &str) -> Result<SkillRefreshReport, Error> {
    let _guard = PASS_LOCK.lock().await;
    let started_at = now.to_string();
    let project_offset = db.skill_refresh_project_offset().await?;
    let targets = load_targets(db, project_offset).await?;
    let statuses = classify_skill_packs(&targets.technologies, &targets.persisted, now);
    let mut report = SkillRefreshReport {
        started_at,
        finished_at: String::new(),
        checked: statuses.len(),
        projects_checked: targets.projects.checked,
        projects_skipped: targets.projects.skipped,
        project_total: targets.projects.total,
        refreshed: Vec::new(),
        due: Vec::new(),
        blocked: Vec::new(),
        failed: Vec::new(),
        ok: true,
    };

    for status in &statuses {
        if matches!(
            status.state,
            SkillPackState::Refreshable | SkillPackState::Due
        ) {
            report.due.push(status.technology.clone());
            record_event(
                db,
                &status.technology,
                EVENT_REFRESH_DUE,
                &due_detail(status),
            )
            .await?;
        }

        match status.state {
            SkillPackState::Refreshable => {
                let Some(pack) = builtin_skill_pack(&status.technology) else {
                    report.failed.push(status.technology.clone());
                    report.blocked.push(status.technology.clone());
                    record_event(
                        db,
                        &status.technology,
                        EVENT_REFRESH_BLOCKED,
                        "builtin refresh source disappeared",
                    )
                    .await?;
                    continue;
                };
                match db.save_skill_pack(&pack).await {
                    Ok(saved) => {
                        report.refreshed.push(status.technology.clone());
                        record_event(
                            db,
                            &status.technology,
                            EVENT_REFRESHED,
                            &format!(
                                "technology={};sourceCheckedAt={};contentReviewedAt={};reviewAfter={};previousSha256={};newSha256={}",
                                status.technology,
                                pack.source_checked_at,
                                pack.content_reviewed_at,
                                pack.review_after,
                                status
                                    .persisted
                                    .as_ref()
                                    .map(|pack| pack.content_sha256.as_str())
                                    .unwrap_or("none"),
                                saved.content_sha256
                            ),
                        )
                        .await?;
                    }
                    Err(error) => {
                        report.failed.push(status.technology.clone());
                        report.blocked.push(status.technology.clone());
                        report.ok = false;
                        record_event(
                            db,
                            &status.technology,
                            EVENT_REFRESH_BLOCKED,
                            &format!(
                                "technology={};reason=persist-failed;error={}",
                                status.technology,
                                bounded(&error.to_string(), 300)
                            ),
                        )
                        .await?;
                    }
                }
            }
            SkillPackState::Due | SkillPackState::Stale | SkillPackState::Missing => {
                report.blocked.push(status.technology.clone());
                record_event(
                    db,
                    &status.technology,
                    EVENT_REFRESH_BLOCKED,
                    &blocked_detail(status),
                )
                .await?;
            }
            SkillPackState::Fresh => {}
        }
    }

    db.set_skill_refresh_project_offset(targets.next_project_offset)
        .await?;
    report.finished_at = Utc::now().to_rfc3339();
    let refreshed = report.refreshed.len();
    let blocked = report.blocked.len();
    let failed = report.failed.len();
    if failed > 0 {
        tracing::warn!(
            checked = report.checked,
            projects_checked = report.projects_checked,
            projects_skipped = report.projects_skipped,
            project_total = report.project_total,
            refreshed,
            blocked,
            failed,
            "skill-pack refresh pass completed with failures"
        );
    } else {
        tracing::info!(
            checked = report.checked,
            projects_checked = report.projects_checked,
            projects_skipped = report.projects_skipped,
            project_total = report.project_total,
            refreshed,
            blocked,
            "skill-pack refresh pass completed"
        );
    }
    Ok(report)
}

fn project_window(total: usize, offset: usize, limit: usize) -> (usize, usize, usize) {
    if total == 0 {
        return (0, 0, 0);
    }
    let start = offset % total;
    let checked = (total - start).min(limit);
    let next = if start + checked >= total {
        0
    } else {
        start + checked
    };
    (start, checked, next)
}

async fn load_targets(db: &Database, offset: usize) -> Result<TargetSnapshot, Error> {
    let records = db.skill_packs().await?;
    let mut persisted = Vec::with_capacity(records.len());
    let mut technologies = BTreeSet::new();
    for record in &records {
        technologies.insert(record.technology.to_ascii_lowercase());
        match crate::planning::SkillPack::from_persisted_json(&record.content_json) {
            Ok(pack) => persisted.push(pack),
            Err(_) => tracing::warn!(
                technology = %bounded(&record.technology, 200),
                "stored skill pack is not a valid planning artifact; it is not trusted"
            ),
        }
    }
    let projects = db.projects().await?;
    let total = projects.len();
    let (start, checked, next_project_offset) =
        project_window(total, offset, MAX_PROJECTS_PER_PASS);
    for index in 0..checked {
        let project = &projects[(start + index) % total];
        let stack = detect_stack(Path::new(&project.repository));
        technologies.extend(required_technologies(&stack));
    }
    technologies.extend(
        BUILTIN_TECHNOLOGIES
            .iter()
            .map(|value| (*value).to_string()),
    );
    let technology_total = technologies.len();
    let technologies = technologies
        .into_iter()
        .take(MAX_SKILL_PACK_STATUSES)
        .collect::<Vec<_>>();
    Ok(TargetSnapshot {
        persisted,
        technologies,
        technologies_truncated: technology_total > MAX_SKILL_PACK_STATUSES,
        projects: ProjectWindowStatus {
            offset: start,
            checked,
            skipped: total.saturating_sub(checked),
            total,
        },
        next_project_offset,
    })
}

fn due_detail(status: &SkillPackStatus) -> String {
    let review_after = status
        .persisted
        .as_ref()
        .or(status.builtin.as_ref())
        .map(|pack| pack.review_after.as_str())
        .unwrap_or("unknown");
    bounded(
        &format!(
            "technology={};state={:?};reviewAfter={};reason={}",
            status.technology, status.state, review_after, status.reason
        ),
        MAX_EVENT_DETAIL_CHARS,
    )
}

fn blocked_detail(status: &SkillPackStatus) -> String {
    bounded(
        &format!(
            "technology={};state={:?};reason={}",
            status.technology, status.state, status.reason
        ),
        MAX_EVENT_DETAIL_CHARS,
    )
}

async fn record_event(
    db: &Database,
    technology: &str,
    kind: &str,
    detail: &str,
) -> Result<(), Error> {
    db.record_event_once(
        &format!("skill-pack:{}", bounded(technology, 200)),
        kind,
        &bounded(detail, MAX_EVENT_DETAIL_CHARS),
    )
    .await?;
    Ok(())
}

fn bounded(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

pub async fn status(db: &Database, now: &str) -> Result<SkillRefreshStatus, Error> {
    let project_offset = db.skill_refresh_project_offset().await?;
    let targets = load_targets(db, project_offset).await?;
    let packs = classify_skill_packs(&targets.technologies, &targets.persisted, now);
    let runtime = match scheduler() {
        Some(handle) => handle.runtime().await,
        None => SkillRefreshRuntime::not_started(),
    };
    Ok(SkillRefreshStatus::from_parts(
        runtime,
        packs,
        targets.technologies_truncated,
        targets.projects,
    ))
}

pub async fn run_now(db: &Database, now: &str) -> Result<SkillRefreshReport, Error> {
    match scheduler() {
        Some(handle) => handle.run_now(db, now).await,
        None => run_pass(db, now).await,
    }
}

/// Start the single Core-owned refresh scheduler. Repeated calls return the
/// existing handle; the interval is set once at startup.
pub fn start_scheduler(db: Database, config: SkillRefreshConfig) -> &'static SkillRefreshHandle {
    SCHEDULER.get_or_init(|| SkillRefreshHandle::start(db, config))
}

pub fn scheduler() -> Option<&'static SkillRefreshHandle> {
    SCHEDULER.get()
}

pub async fn shutdown_scheduler() {
    if let Some(handle) = SCHEDULER.get() {
        handle.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::SkillPack;

    fn temp_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "factory-skill-refresh-{}.sqlite",
            uuid::Uuid::new_v4()
        ))
    }

    fn due_pack(technology: &str) -> SkillPack {
        let mut pack = builtin_skill_pack(technology).unwrap_or_else(|| {
            let mut pack = builtin_skill_pack("rust").unwrap();
            pack.technology = technology.into();
            pack
        });
        pack.technology = technology.into();
        pack.content_reviewed_at = "2026-01-01T00:00:00Z".into();
        pack.review_after = "2026-04-01T00:00:00Z".into();
        pack.best_practices
            .push("persisted legacy revision content".into());
        pack
    }

    #[test]
    fn interval_is_env_parsed_and_bounded() {
        assert_eq!(
            SkillRefreshConfig::from_seconds(None)
                .unwrap()
                .interval_seconds(),
            DEFAULT_SKILL_REFRESH_INTERVAL_SECONDS
        );
        assert!(SkillRefreshConfig::from_seconds(Some("3600")).is_ok());
        assert!(SkillRefreshConfig::from_seconds(Some("604800")).is_ok());
        assert!(SkillRefreshConfig::from_seconds(Some("3599")).is_err());
        assert!(SkillRefreshConfig::from_seconds(Some("604801")).is_err());
        assert!(SkillRefreshConfig::from_seconds(Some("not-a-number")).is_err());
    }

    #[test]
    fn project_window_rotates_and_reports_skipped_projects() {
        assert_eq!(project_window(130, 0, 64), (0, 64, 64));
        assert_eq!(project_window(130, 64, 64), (64, 64, 128));
        assert_eq!(project_window(130, 128, 64), (128, 2, 0));
        assert_eq!(project_window(3, 0, 64), (0, 3, 0));
        assert_eq!(project_window(0, 0, 64), (0, 0, 0));
    }

    #[tokio::test]
    async fn refresh_pass_reports_and_rotates_skipped_projects() {
        let path = temp_path();
        let db = Database::open(&path).await.unwrap();
        for index in 0..70 {
            db.insert_project_for_test(
                &format!("project-{index}"),
                &format!("/repo/{index}"),
                &(Utc::now() + chrono::Duration::milliseconds(i64::from(index))).to_rfc3339(),
            )
            .await
            .unwrap();
        }
        let first = run_pass(&db, "2026-11-09T00:00:00Z").await.unwrap();
        assert_eq!(first.project_total, 70);
        assert_eq!(first.projects_checked, 64);
        assert_eq!(first.projects_skipped, 6);
        assert_eq!(db.skill_refresh_project_offset().await.unwrap(), 64);
        let second = run_pass(&db, "2026-11-09T00:00:01Z").await.unwrap();
        assert_eq!(second.projects_checked, 6);
        assert_eq!(second.projects_skipped, 64);
        assert_eq!(db.skill_refresh_project_offset().await.unwrap(), 0);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn refresh_creates_an_immutable_revision_and_replays_idempotently() {
        let path = temp_path();
        let db = Database::open(&path).await.unwrap();
        let due = due_pack("rust");
        let saved_due = db.save_skill_pack(&due).await.unwrap();
        let before = db.skill_packs().await.unwrap().len();

        let now = "2026-11-09T00:00:00Z";
        let first = run_pass(&db, now).await.unwrap();
        assert!(first.refreshed.contains(&"rust".to_string()));
        assert!(first.ok);
        let after = db.skill_packs().await.unwrap();
        assert_eq!(
            after.len(),
            before + 1,
            "refresh must append exactly one revision and must not pre-seed the catalog"
        );
        let rust = after
            .iter()
            .filter(|record| record.technology == "rust")
            .collect::<Vec<_>>();
        assert_eq!(rust.len(), 2);
        assert_eq!(rust[0].content_sha256, saved_due.content_sha256);
        assert_eq!(
            rust[1].review_after,
            crate::planning::skills::BUILTIN_REVIEW_AFTER
        );

        let second = run_pass(&db, now).await.unwrap();
        assert!(second.refreshed.is_empty());
        assert_eq!(db.skill_packs().await.unwrap().len(), after.len());
        let events = db.events().await.unwrap();
        assert_eq!(
            events
                .iter()
                .filter(
                    |event| event.kind == EVENT_REFRESHED && event.subject_id == "skill-pack:rust"
                )
                .count(),
            1
        );
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn scheduler_shutdown_is_idempotent() {
        let path = temp_path();
        let db = Database::open(&path).await.unwrap();
        let handle = SkillRefreshHandle::start(
            db.clone(),
            SkillRefreshConfig::from_seconds(Some("3600")).unwrap(),
        );
        handle.shutdown().await;
        handle.shutdown().await;
        assert!(!handle.runtime().await.running);
        db.close().await;
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn due_and_blocked_events_are_idempotent() {
        let path = temp_path();
        let db = Database::open(&path).await.unwrap();
        db.save_skill_pack(&due_pack("legacy-tool")).await.unwrap();
        let now = "2026-11-09T00:00:00Z";
        let first = run_pass(&db, now).await.unwrap();
        assert!(first.due.contains(&"legacy-tool".to_string()));
        assert!(first.blocked.contains(&"legacy-tool".to_string()));
        let _ = run_pass(&db, now).await.unwrap();
        let events = db.events().await.unwrap();
        for kind in [EVENT_REFRESH_DUE, EVENT_REFRESH_BLOCKED] {
            assert_eq!(
                events
                    .iter()
                    .filter(
                        |event| event.kind == kind && event.subject_id == "skill-pack:legacy-tool"
                    )
                    .count(),
                1,
                "{kind} must be durable and idempotent"
            );
        }
        db.close().await;
        let _ = std::fs::remove_file(path);
    }
}
