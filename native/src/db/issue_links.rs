//! Persistence for frozen issue links and the idempotent delivery ledger.
//!
//! Links are written in the same transaction as the intake request. Delivery
//! events are append-only logical intents; an event row is reused by its
//! `(issue_link_id, event_key)` identity so a retry can never create a second
//! logical comment, label or close.

use super::*;

use chrono::Utc;
use sqlx::row::Row;
use sqlx_sqlite::{SqliteConnection, SqliteRow};
use uuid::Uuid;

use crate::issues::{
    IssueDeliveryAction, IssueDeliveryEventRecord, IssueDeliveryEventStatus, IssueDeliveryFailure,
    IssueDeliveryState, IssueLinkProjection, IssueLinkRecord, IssueLinkResolution, IssueLinkState,
    IssueLinkSummary, IssueRole, MAX_EVENT_KEY_CHARS, MAX_ISSUES_PER_REQUEST, NewIssueLinkSpec,
    NewIssueSelection, derive_issue_link_state, valid_delivery_marker,
};

#[derive(Debug, Clone)]
pub struct NewIssueDeliveryEvent<'a> {
    pub issue_link_id: &'a str,
    pub campaign_id: Option<&'a str>,
    pub candidate_id: Option<&'a str>,
    pub action: IssueDeliveryAction,
    pub event_key: &'a str,
    pub marker: Option<&'a str>,
    pub body: Option<&'a str>,
    pub label_name: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct IssueDeliveryEventCompletion<'a> {
    pub status: IssueDeliveryEventStatus,
    pub external_id: Option<&'a str>,
    pub error_kind: Option<&'a str>,
    pub error_detail: Option<&'a str>,
}

struct LinkFacts {
    lead_state: Option<String>,
    campaign_status: Option<String>,
    gate_outcome: Option<String>,
}

impl Database {
    /// Resolve operator-selected issue keys against the Project's bound GitHub
    /// repository and synchronized open issues. Closed, foreign, unsynchronized
    /// and duplicate selections are rejected before any request row exists.
    pub(super) async fn resolve_selected_issue_links_in_tx(
        connection: &mut SqliteConnection,
        project_id: &str,
        selections: &[NewIssueSelection],
    ) -> Result<Vec<NewIssueLinkSpec>, Error> {
        if selections.is_empty() {
            return Ok(Vec::new());
        }
        if selections.len() > MAX_ISSUES_PER_REQUEST {
            return Err(intake_issue_conflict(
                "ISSUE_SELECTION_LIMIT",
                "a request may freeze at most 20 issues",
            ));
        }
        let binding = load_binding(connection, project_id).await?;
        let mut seen = std::collections::HashSet::new();
        let mut specs = Vec::with_capacity(selections.len());
        for selection in selections {
            crate::github::validate_repo_coordinates(&selection.owner, &selection.repo)
                .map_err(|_| Error::Invalid("selected issue repository"))?;
            if selection.number == 0 {
                return Err(intake_issue_conflict(
                    "ISSUE_NUMBER_INVALID",
                    "issue numbers must be positive",
                ));
            }
            if !selection.owner.eq_ignore_ascii_case(&binding.owner)
                || !selection.repo.eq_ignore_ascii_case(&binding.repo)
            {
                return Err(intake_issue_conflict(
                    "ISSUE_REPOSITORY_FOREIGN",
                    "selected issue belongs to a repository other than the Project binding",
                ));
            }
            let repository_full_name = format!("{}/{}", binding.owner, binding.repo);
            if !seen.insert((repository_full_name.to_ascii_lowercase(), selection.number)) {
                return Err(intake_issue_conflict(
                    "ISSUE_DUPLICATE",
                    "the same issue was selected more than once",
                ));
            }
            let row =
                sqlx::query::query(
                    "SELECT github_id,state,title,is_pull_request
                 FROM github_issues
                 WHERE project_id=?1 AND repo_full_name=?2 AND number=?3",
                )
                .bind(project_id)
                .bind(&binding.repo_full_name)
                .bind(i64::try_from(selection.number).map_err(|_| {
                    Error::Invalid("selected issue number exceeds the supported bound")
                })?)
                .fetch_optional(&mut *connection)
                .await?
                .ok_or_else(|| {
                    intake_issue_conflict(
                        "ISSUE_UNSUPPORTED",
                        "the selected issue is not present in the synchronized repository snapshot",
                    )
                })?;
            let state: String = row.try_get("state")?;
            let is_pull_request: i64 = row.try_get("is_pull_request")?;
            if is_pull_request != 0 {
                return Err(intake_issue_conflict(
                    "ISSUE_UNSUPPORTED",
                    "pull requests cannot be selected as delivery issues",
                ));
            }
            if state == "closed" {
                return Err(intake_issue_conflict(
                    "ISSUE_CLOSED",
                    "closed issues cannot be selected for new work",
                ));
            }
            specs.push(NewIssueLinkSpec {
                owner: binding.owner.clone(),
                repo: binding.repo.clone(),
                number: selection.number,
                github_issue_id: row.try_get("github_id")?,
                title: row.try_get("title")?,
                role: selection.role,
                work_unit_id: None,
            });
        }
        if !specs.iter().any(|spec| spec.role == IssueRole::Primary) {
            return Err(intake_issue_conflict(
                "ISSUE_PRIMARY_REQUIRED",
                "at least one selected issue must have the PRIMARY role",
            ));
        }
        Ok(specs)
    }

    pub(super) async fn intake_request_issue_links_match_in_tx(
        connection: &mut SqliteConnection,
        request_id: &str,
        selections: &[NewIssueSelection],
    ) -> Result<bool, Error> {
        let rows = sqlx::query::query(
            "SELECT repository_owner,repository_name,issue_number,role
             FROM issue_links WHERE request_id=?1 ORDER BY issue_number",
        )
        .bind(request_id)
        .fetch_all(&mut *connection)
        .await?;
        let existing = rows
            .iter()
            .map(|row| {
                Ok((
                    row.try_get::<String, _>("repository_owner")?,
                    row.try_get::<String, _>("repository_name")?,
                    row.try_get::<i64, _>("issue_number")? as u64,
                    row.try_get::<String, _>("role")?,
                ))
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;
        if existing.len() != selections.len() {
            return Ok(false);
        }
        let mut requested = selections
            .iter()
            .map(|selection| {
                (
                    selection.owner.to_ascii_lowercase(),
                    selection.repo.to_ascii_lowercase(),
                    selection.number,
                    selection.role.as_str().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        requested.sort();
        let mut existing = existing
            .into_iter()
            .map(|(owner, repo, number, role)| {
                (
                    owner.to_ascii_lowercase(),
                    repo.to_ascii_lowercase(),
                    number,
                    role,
                )
            })
            .collect::<Vec<_>>();
        existing.sort();
        Ok(existing == requested)
    }

    pub(super) async fn insert_issue_links_in_tx(
        connection: &mut SqliteConnection,
        project_id: &str,
        request_id: &str,
        specs: &[NewIssueLinkSpec],
    ) -> Result<Vec<IssueLinkRecord>, Error> {
        if specs.is_empty() {
            return Ok(Vec::new());
        }
        if specs.len() > MAX_ISSUES_PER_REQUEST {
            return Err(Error::Invalid(
                "issue link selection exceeds the supported bound",
            ));
        }
        let binding = load_binding(connection, project_id).await?;
        let mut seen = std::collections::HashSet::new();
        let mut created = Vec::with_capacity(specs.len());
        for spec in specs {
            if spec.number == 0
                || spec.github_issue_id <= 0
                || spec.title.len() > 512
                || spec
                    .work_unit_id
                    .as_deref()
                    .is_some_and(|unit| unit.trim().is_empty() || unit.len() > 128)
            {
                return Err(Error::Invalid("frozen issue link"));
            }
            if !spec.owner.eq_ignore_ascii_case(&binding.owner)
                || !spec.repo.eq_ignore_ascii_case(&binding.repo)
            {
                return Err(intake_issue_conflict(
                    "ISSUE_REPOSITORY_FOREIGN",
                    "selected issue belongs to a repository other than the Project binding",
                ));
            }
            if !seen.insert((binding.repo_full_name.to_ascii_lowercase(), spec.number)) {
                return Err(intake_issue_conflict(
                    "ISSUE_DUPLICATE",
                    "the same issue was selected more than once",
                ));
            }
            let now = Utc::now().to_rfc3339();
            let record = IssueLinkRecord {
                id: Uuid::new_v4().to_string(),
                project_id: project_id.into(),
                request_id: request_id.into(),
                campaign_id: None,
                candidate_id: None,
                repository_owner: binding.owner.clone(),
                repository_name: binding.repo.clone(),
                repository_full_name: binding.repo_full_name.clone(),
                issue_number: spec.number,
                github_issue_id: Some(spec.github_issue_id),
                issue_title: spec.title.trim().to_owned(),
                role: spec.role,
                state: IssueLinkState::Ready,
                delivery_state: IssueDeliveryState::None,
                resolution: IssueLinkResolution::Unresolved,
                work_unit_id: spec.work_unit_id.clone(),
                pull_request_number: None,
                pull_request_url: None,
                merge_sha: None,
                merged_at: None,
                created_at: now.clone(),
                updated_at: now,
                latest_error: None,
            };
            sqlx::query::query(
                "INSERT INTO issue_links
                 (id,project_id,request_id,campaign_id,candidate_id,repository_owner,
                  repository_name,repository_full_name,issue_number,github_issue_id,issue_title,
                  role,state,delivery_state,resolution,work_unit_id,pull_request_number,
                  pull_request_url,merge_sha,merged_at,created_at,updated_at)
                 VALUES (?1,?2,?3,NULL,NULL,?4,?5,?6,?7,?8,?9,?10,'READY','NONE','UNRESOLVED',
                         ?11,NULL,NULL,NULL,NULL,?12,?12)",
            )
            .bind(&record.id)
            .bind(project_id)
            .bind(request_id)
            .bind(&record.repository_owner)
            .bind(&record.repository_name)
            .bind(&record.repository_full_name)
            .bind(
                i64::try_from(record.issue_number)
                    .map_err(|_| Error::Invalid("issue number exceeds the supported bound"))?,
            )
            .bind(record.github_issue_id)
            .bind(&record.issue_title)
            .bind(record.role.as_str())
            .bind(&record.work_unit_id)
            .bind(&record.created_at)
            .execute(&mut *connection)
            .await?;
            record_event(
                connection,
                request_id,
                "issue_link.created",
                &format!(
                    "link={} issue={}#{} role={}",
                    record.id,
                    record.repository_full_name,
                    record.issue_number,
                    record.role.as_str()
                ),
            )
            .await?;
            created.push(record);
        }
        Ok(created)
    }

    pub async fn issue_links_for_request(
        &self,
        request_id: &str,
    ) -> Result<Vec<IssueLinkRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        let rows = sqlx::query::query(
            "SELECT * FROM issue_links WHERE request_id=?1 ORDER BY created_at,issue_number",
        )
        .bind(request_id)
        .fetch_all(&mut *connection)
        .await?;
        let mut links = Vec::with_capacity(rows.len());
        for row in rows {
            links.push(project_issue_link(&mut connection, &row).await?);
        }
        Ok(links)
    }

    pub async fn issue_links_for_campaign(
        &self,
        campaign_id: &str,
    ) -> Result<Vec<IssueLinkRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        let rows = sqlx::query::query(
            "SELECT il.* FROM issue_links il
             JOIN intake_requests ir ON ir.id=il.request_id
             WHERE il.campaign_id=?1 OR ir.campaign_id=?1
             ORDER BY il.created_at,il.issue_number",
        )
        .bind(campaign_id)
        .fetch_all(&mut *connection)
        .await?;
        let mut links = Vec::with_capacity(rows.len());
        for row in rows {
            links.push(project_issue_link(&mut connection, &row).await?);
        }
        Ok(links)
    }

    pub async fn issue_link_projections_for_request(
        &self,
        request_id: &str,
    ) -> Result<Vec<IssueLinkProjection>, Error> {
        let links = self.issue_links_for_request(request_id).await?;
        self.attach_delivery_events(links).await
    }

    pub async fn issue_link_projections_for_campaign(
        &self,
        campaign_id: &str,
    ) -> Result<Vec<IssueLinkProjection>, Error> {
        let links = self.issue_links_for_campaign(campaign_id).await?;
        self.attach_delivery_events(links).await
    }

    async fn attach_delivery_events(
        &self,
        links: Vec<IssueLinkRecord>,
    ) -> Result<Vec<IssueLinkProjection>, Error> {
        let mut projections = Vec::with_capacity(links.len());
        for link in links {
            let events = self.issue_delivery_events_for_link(&link.id).await?;
            projections.push(IssueLinkProjection { link, events });
        }
        Ok(projections)
    }

    pub async fn issue_link(&self, link_id: &str) -> Result<IssueLinkRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        let row = sqlx::query::query("SELECT * FROM issue_links WHERE id=?1")
            .bind(link_id)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(Error::NotFound)?;
        project_issue_link(&mut connection, &row).await
    }

    /// Latest link per issue for the Project board. Core state is derived from
    /// recorded facts here, so Office can render `IN_REVIEW`/`DONE` without
    /// guessing from local queue rows.
    pub async fn issue_link_summaries_for_project(
        &self,
        project_id: &str,
    ) -> Result<std::collections::HashMap<(String, u64), IssueLinkSummary>, Error> {
        let mut connection = self.pool.acquire().await?;
        let rows = sqlx::query::query(
            "SELECT * FROM issue_links WHERE project_id=?1
             ORDER BY created_at DESC,id DESC LIMIT 1000",
        )
        .bind(project_id)
        .fetch_all(&mut *connection)
        .await?;
        let mut summaries = std::collections::HashMap::new();
        for row in rows {
            let link = project_issue_link(&mut connection, &row).await?;
            let key = (
                link.repository_full_name.to_ascii_lowercase(),
                link.issue_number,
            );
            if summaries.contains_key(&key) {
                continue;
            }
            let latest = load_latest_delivery_failure(&mut connection, &link.id).await?;
            summaries.insert(
                key,
                IssueLinkSummary {
                    id: link.id,
                    request_id: link.request_id,
                    campaign_id: link.campaign_id,
                    candidate_id: link.candidate_id,
                    role: link.role,
                    state: link.state,
                    delivery_state: link.delivery_state,
                    resolution: link.resolution,
                    pull_request_number: link.pull_request_number,
                    pull_request_url: link.pull_request_url,
                    merge_sha: link.merge_sha,
                    merged_at: link.merged_at,
                    latest_error_kind: latest.as_ref().map(|(failure, _)| failure.kind.clone()),
                    latest_error_detail: latest.as_ref().map(|(failure, _)| failure.detail.clone()),
                    latest_error_event_id: latest.map(|(_, event_id)| event_id),
                },
            );
        }
        Ok(summaries)
    }

    pub async fn record_issue_delivery_event(
        &self,
        input: NewIssueDeliveryEvent<'_>,
    ) -> Result<(IssueDeliveryEventRecord, bool), Error> {
        validate_delivery_event(&input)?;
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let link = load_issue_link(&mut transaction, input.issue_link_id).await?;
        let existing = sqlx::query::query(
            "SELECT * FROM issue_delivery_events
             WHERE issue_link_id=?1 AND event_key=?2",
        )
        .bind(input.issue_link_id)
        .bind(input.event_key)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(row) = existing {
            let event = event_from_row(&row)?;
            if event.action != input.action
                || event.marker.as_deref() != input.marker
                || event.body.as_deref() != input.body
                || event.label_name.as_deref() != input.label_name
            {
                return Err(Error::Conflict(
                    "issue delivery event idempotency key was reused with different content",
                ));
            }
            transaction.commit().await?;
            return Ok((event, false));
        }
        let now = Utc::now().to_rfc3339();
        let event = IssueDeliveryEventRecord {
            id: Uuid::new_v4().to_string(),
            issue_link_id: link.id.clone(),
            campaign_id: input.campaign_id.map(str::to_owned),
            candidate_id: input.candidate_id.map(str::to_owned),
            action: input.action,
            event_key: input.event_key.into(),
            marker: input.marker.map(str::to_owned),
            body: input.body.map(str::to_owned),
            label_name: input.label_name.map(str::to_owned),
            status: IssueDeliveryEventStatus::Pending,
            attempts: 0,
            external_id: None,
            error_kind: None,
            error_detail: None,
            last_attempt_at: None,
            applied_at: None,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        sqlx::query::query(
            "INSERT INTO issue_delivery_events
             (id,issue_link_id,campaign_id,candidate_id,action,event_key,marker,body,label_name,
              status,attempts,external_id,error_kind,error_detail,last_attempt_at,applied_at,
              created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'PENDING',0,NULL,NULL,NULL,NULL,NULL,?10,?10)",
        )
        .bind(&event.id)
        .bind(&event.issue_link_id)
        .bind(&event.campaign_id)
        .bind(&event.candidate_id)
        .bind(event.action.as_str())
        .bind(&event.event_key)
        .bind(&event.marker)
        .bind(&event.body)
        .bind(&event.label_name)
        .bind(&event.created_at)
        .execute(&mut *transaction)
        .await?;
        update_link_delivery_state(
            &mut transaction,
            &link.id,
            IssueLinkState::NeedsAttention,
            IssueDeliveryState::Pending,
        )
        .await?;
        record_event(
            &mut transaction,
            &link.id,
            "issue_delivery.pending",
            &format!(
                "event={} action={} key={}",
                event.id,
                event.action.as_str(),
                event.event_key
            ),
        )
        .await?;
        transaction.commit().await?;
        Ok((event, true))
    }

    pub async fn issue_delivery_event(
        &self,
        event_id: &str,
    ) -> Result<IssueDeliveryEventRecord, Error> {
        let mut connection = self.pool.acquire().await?;
        let row = sqlx::query::query("SELECT * FROM issue_delivery_events WHERE id=?1")
            .bind(event_id)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(Error::NotFound)?;
        event_from_row(&row)
    }

    pub async fn issue_delivery_events_for_link(
        &self,
        link_id: &str,
    ) -> Result<Vec<IssueDeliveryEventRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        let rows = sqlx::query::query(
            "SELECT * FROM issue_delivery_events
             WHERE issue_link_id=?1 ORDER BY created_at,id",
        )
        .bind(link_id)
        .fetch_all(&mut *connection)
        .await?;
        rows.iter().map(event_from_row).collect()
    }

    pub async fn issue_delivery_events_for_campaign(
        &self,
        campaign_id: &str,
    ) -> Result<Vec<IssueDeliveryEventRecord>, Error> {
        let mut connection = self.pool.acquire().await?;
        let rows = sqlx::query::query(
            "SELECT * FROM issue_delivery_events
             WHERE campaign_id=?1 ORDER BY created_at,id",
        )
        .bind(campaign_id)
        .fetch_all(&mut *connection)
        .await?;
        rows.iter().map(event_from_row).collect()
    }

    pub async fn start_issue_delivery_attempt(
        &self,
        event_id: &str,
    ) -> Result<IssueDeliveryEventRecord, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let event = load_event(&mut transaction, event_id).await?;
        if event.status == IssueDeliveryEventStatus::Applied {
            transaction.commit().await?;
            return Ok(event);
        }
        if event.attempts >= 10 {
            return Err(Error::Conflict("issue delivery attempt limit reached"));
        }
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "UPDATE issue_delivery_events
             SET attempts=attempts+1,last_attempt_at=?1,status='PENDING',updated_at=?1
             WHERE id=?2 AND status<>'APPLIED'",
        )
        .bind(&now)
        .bind(event_id)
        .execute(&mut *transaction)
        .await?;
        let event = load_event(&mut transaction, event_id).await?;
        transaction.commit().await?;
        Ok(event)
    }

    pub async fn complete_issue_delivery_event(
        &self,
        event_id: &str,
        completion: IssueDeliveryEventCompletion<'_>,
    ) -> Result<IssueDeliveryEventRecord, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let event = load_event(&mut transaction, event_id).await?;
        if event.status == IssueDeliveryEventStatus::Applied {
            transaction.commit().await?;
            return Ok(event);
        }
        let now = Utc::now().to_rfc3339();
        let applied_at =
            (completion.status == IssueDeliveryEventStatus::Applied).then(|| now.clone());
        sqlx::query::query(
            "UPDATE issue_delivery_events
             SET status=?1,external_id=?2,error_kind=?3,error_detail=?4,applied_at=?5,
                 updated_at=?6
             WHERE id=?7",
        )
        .bind(completion.status.as_str())
        .bind(completion.external_id)
        .bind(completion.error_kind)
        .bind(completion.error_detail)
        .bind(applied_at)
        .bind(&now)
        .bind(event_id)
        .execute(&mut *transaction)
        .await?;
        if completion.status == IssueDeliveryEventStatus::Applied {
            record_event(
                &mut transaction,
                &event.issue_link_id,
                "issue_delivery.applied",
                &format!(
                    "event={} action={} external={}",
                    event.id,
                    event.action.as_str(),
                    completion.external_id.unwrap_or("")
                ),
            )
            .await?;
        } else {
            record_event(
                &mut transaction,
                &event.issue_link_id,
                "issue_delivery.failed",
                &format!(
                    "event={} status={} kind={}",
                    event.id,
                    completion.status.as_str(),
                    completion.error_kind.unwrap_or("")
                ),
            )
            .await?;
        }
        let event = load_event(&mut transaction, event_id).await?;
        transaction.commit().await?;
        Ok(event)
    }

    pub async fn record_issue_delivery_retry(
        &self,
        event_id: &str,
        actor: &str,
    ) -> Result<IssueDeliveryEventRecord, Error> {
        if actor.trim().is_empty() || actor.len() > 200 {
            return Err(Error::Invalid("issue delivery retry actor"));
        }
        let event = self.issue_delivery_event(event_id).await?;
        let mut transaction = self.pool.begin().await?;
        record_event(
            &mut transaction,
            &event.issue_link_id,
            "issue_delivery.retried",
            &format!("event={} actor={actor}", event.id),
        )
        .await?;
        transaction.commit().await?;
        Ok(event)
    }

    pub async fn mark_issue_link_review(
        &self,
        link_id: &str,
        campaign_id: &str,
        candidate_id: &str,
        pull_request_number: u64,
        pull_request_url: &str,
    ) -> Result<IssueLinkRecord, Error> {
        if pull_request_number == 0 || pull_request_url.trim().is_empty() {
            return Err(Error::Invalid("issue link pull request identity"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let now = Utc::now().to_rfc3339();
        let updated = sqlx::query::query(
            "UPDATE issue_links
             SET campaign_id=?1,candidate_id=?2,pull_request_number=?3,pull_request_url=?4,
                 state='IN_REVIEW',updated_at=?5
             WHERE id=?6 AND (campaign_id IS NULL OR campaign_id=?1)",
        )
        .bind(campaign_id)
        .bind(candidate_id)
        .bind(
            i64::try_from(pull_request_number)
                .map_err(|_| Error::Invalid("pull request number exceeds the supported bound"))?,
        )
        .bind(pull_request_url)
        .bind(&now)
        .bind(link_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(Error::Conflict(
                "issue link is already bound to another Campaign",
            ));
        }
        let link = load_issue_link(&mut transaction, link_id).await?;
        transaction.commit().await?;
        Ok(link)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn mark_issue_link_merged(
        &self,
        link_id: &str,
        campaign_id: &str,
        candidate_id: &str,
        merge_sha: &str,
        merged_at: &str,
        state: IssueLinkState,
        delivery_state: IssueDeliveryState,
        resolution: IssueLinkResolution,
    ) -> Result<IssueLinkRecord, Error> {
        if merge_sha.len() < 7
            || merge_sha.len() > 64
            || !merge_sha.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Invalid("issue link merge SHA"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let now = Utc::now().to_rfc3339();
        let updated = sqlx::query::query(
            "UPDATE issue_links
             SET campaign_id=?1,candidate_id=?2,merge_sha=?3,merged_at=?4,state=?5,
                 delivery_state=?6,resolution=?7,updated_at=?8
             WHERE id=?9 AND (campaign_id IS NULL OR campaign_id=?1)",
        )
        .bind(campaign_id)
        .bind(candidate_id)
        .bind(merge_sha)
        .bind(merged_at)
        .bind(state.as_str())
        .bind(delivery_state.as_str())
        .bind(resolution.as_str())
        .bind(&now)
        .bind(link_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(Error::Conflict(
                "issue link is already bound to another Campaign",
            ));
        }
        let link = load_issue_link(&mut transaction, link_id).await?;
        transaction.commit().await?;
        Ok(link)
    }

    pub async fn set_issue_link_delivery_state(
        &self,
        link_id: &str,
        state: IssueLinkState,
        delivery_state: IssueDeliveryState,
    ) -> Result<IssueLinkRecord, Error> {
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query::query(
            "UPDATE issue_links SET state=?1,delivery_state=?2,updated_at=?3 WHERE id=?4",
        )
        .bind(state.as_str())
        .bind(delivery_state.as_str())
        .bind(Utc::now().to_rfc3339())
        .bind(link_id)
        .execute(&mut *transaction)
        .await?;
        let link = load_issue_link(&mut transaction, link_id).await?;
        transaction.commit().await?;
        Ok(link)
    }

    /// True when every recorded Lead task has a recorded successful candidate.
    /// Direct Campaigns have no Lead tasks and therefore satisfy this fact.
    pub async fn campaign_work_complete(&self, campaign_id: &str) -> Result<bool, Error> {
        let (open,): (i64,) = sqlx::query_as::query_as(
            "SELECT COUNT(*) FROM lead_tasks
             WHERE campaign_id=?1 AND status NOT IN ('SUCCEEDED','COMPLETED')",
        )
        .bind(campaign_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(open == 0)
    }

    /// The frozen WorkGraph unit that owns a candidate, if any. Direct
    /// Campaigns have no Lead task and therefore no implicit unit mapping.
    pub async fn candidate_work_unit(
        &self,
        campaign_id: &str,
        candidate_id: &str,
    ) -> Result<Option<String>, Error> {
        let value: Option<(String,)> = sqlx::query_as::query_as(
            "SELECT unit_id FROM lead_tasks WHERE campaign_id=?1 AND candidate_id=?2
             ORDER BY sequence LIMIT 1",
        )
        .bind(campaign_id)
        .bind(candidate_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(value.map(|(unit,)| unit))
    }
}

fn intake_issue_conflict(code: &'static str, message: &str) -> Error {
    Error::IntakeConflict {
        code,
        message: message.into(),
    }
}

fn validate_delivery_event(input: &NewIssueDeliveryEvent<'_>) -> Result<(), Error> {
    let key = input.event_key;
    if key.is_empty()
        || key.len() > MAX_EVENT_KEY_CHARS
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_' | b'.'))
    {
        return Err(Error::Invalid("issue delivery event key"));
    }
    if input.issue_link_id.trim().is_empty() {
        return Err(Error::Invalid("issue delivery link"));
    }
    match input.action {
        IssueDeliveryAction::Comment => {
            let marker = input.marker.ok_or(Error::Invalid("issue comment marker"))?;
            if !valid_delivery_marker(marker) {
                return Err(Error::Invalid("issue comment marker"));
            }
            let body = input.body.ok_or(Error::Invalid("issue comment body"))?;
            if body.trim().is_empty() || body.len() > 65_536 || !body.contains(marker) {
                return Err(Error::Invalid("issue comment body"));
            }
        }
        IssueDeliveryAction::Label => {
            let label = input.label_name.ok_or(Error::Invalid("issue label name"))?;
            if label.trim().is_empty() || label.len() > 100 || label.contains('\n') {
                return Err(Error::Invalid("issue label name"));
            }
        }
        IssueDeliveryAction::Close | IssueDeliveryAction::Failure => {}
    }
    if input
        .marker
        .is_some_and(|marker| !valid_delivery_marker(marker))
    {
        return Err(Error::Invalid("issue delivery marker"));
    }
    Ok(())
}

async fn load_binding(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> Result<ProjectGitHubBinding, Error> {
    let row = sqlx::query::query(
        "SELECT project_id,owner,repo,repo_full_name,default_branch,repository_github_id,
                bound_at,updated_at
         FROM project_github_bindings WHERE project_id=?1",
    )
    .bind(project_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or_else(|| {
        intake_issue_conflict(
            "ISSUE_REPOSITORY_NOT_BOUND",
            "the Project has no bound GitHub repository for the selected issues",
        )
    })?;
    Ok(ProjectGitHubBinding {
        project_id: row.try_get("project_id")?,
        owner: row.try_get("owner")?,
        repo: row.try_get("repo")?,
        repo_full_name: row.try_get("repo_full_name")?,
        default_branch: row.try_get("default_branch")?,
        repository_github_id: row.try_get("repository_github_id")?,
        bound_at: row.try_get("bound_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

async fn load_issue_link(
    connection: &mut SqliteConnection,
    link_id: &str,
) -> Result<IssueLinkRecord, Error> {
    let row = sqlx::query::query("SELECT * FROM issue_links WHERE id=?1")
        .bind(link_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(Error::NotFound)?;
    link_from_row(&row)
}

async fn project_issue_link(
    connection: &mut SqliteConnection,
    row: &SqliteRow,
) -> Result<IssueLinkRecord, Error> {
    let mut link = link_from_row(row)?;
    let facts = load_link_facts(connection, &link).await?;
    link.state = derive_issue_link_state(crate::issues::IssueStateFacts {
        lead_state: facts.lead_state.as_deref(),
        campaign_status: facts.campaign_status.as_deref(),
        gate_outcome: facts.gate_outcome.as_deref(),
        pull_request_opened: link.pull_request_number.is_some(),
        merge_sha: link.merge_sha.as_deref(),
        delivery_state: link.delivery_state,
        resolution: link.resolution,
    });
    link.latest_error = load_latest_delivery_failure(connection, &link.id)
        .await?
        .map(|(failure, _)| failure);
    Ok(link)
}

async fn load_link_facts(
    connection: &mut SqliteConnection,
    link: &IssueLinkRecord,
) -> Result<LinkFacts, Error> {
    let row = sqlx::query::query(
        "SELECT lr.state,COALESCE(ir.campaign_id,lr.campaign_id) AS campaign_id
         FROM intake_requests ir
         JOIN lead_requests lr ON lr.id=ir.lead_request_id
         WHERE ir.id=?1",
    )
    .bind(&link.request_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(LinkFacts {
            lead_state: None,
            campaign_status: None,
            gate_outcome: None,
        });
    };
    let lead_state: String = row.try_get("state")?;
    let campaign_id: Option<String> = row
        .try_get::<Option<String>, _>("campaign_id")?
        .or_else(|| link.campaign_id.clone());
    let campaign_status = match campaign_id.as_deref() {
        Some(campaign_id) => {
            let status: Option<(String,)> =
                sqlx::query_as::query_as("SELECT status FROM campaigns WHERE id=?1")
                    .bind(campaign_id)
                    .fetch_optional(&mut *connection)
                    .await?;
            status.map(|(status,)| status)
        }
        None => None,
    };
    // Gate facts belong to the exact candidate recorded on the link. A later
    // candidate in the same Campaign must never rewrite an earlier issue's
    // board state.
    let gate_outcome = match (campaign_id.as_deref(), link.candidate_id.as_deref()) {
        (Some(campaign_id), Some(candidate_id)) => {
            let gate: Option<(Option<String>,)> = sqlx::query_as::query_as(
                "SELECT gdc.outcome
                 FROM candidates cd
                 JOIN attempts a ON a.id=cd.attempt_id
                 JOIN runs r ON r.id=a.run_id
                 LEFT JOIN gate_decisions_current gdc ON gdc.candidate_id=cd.id
                 WHERE r.campaign_id=?1 AND cd.id=?2
                 LIMIT 1",
            )
            .bind(campaign_id)
            .bind(candidate_id)
            .fetch_optional(&mut *connection)
            .await?;
            gate.and_then(|(outcome,)| outcome)
        }
        _ => None,
    };
    Ok(LinkFacts {
        lead_state: Some(lead_state),
        campaign_status,
        gate_outcome,
    })
}

async fn load_latest_delivery_failure(
    connection: &mut SqliteConnection,
    link_id: &str,
) -> Result<Option<(IssueDeliveryFailure, String)>, Error> {
    let row = sqlx::query::query(
        "SELECT e.id,e.error_kind,e.error_detail,e.status
         FROM issue_delivery_events e
         WHERE e.issue_link_id=?1 AND e.status IN ('FAILED','BLOCKED')
           AND NOT EXISTS (
             SELECT 1 FROM issue_delivery_events applied
             WHERE applied.issue_link_id=e.issue_link_id
               AND applied.status='APPLIED'
               AND applied.updated_at >= e.updated_at
           )
         ORDER BY e.updated_at DESC,e.id DESC LIMIT 1",
    )
    .bind(link_id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let status: String = row.try_get("status")?;
    Ok(Some((
        IssueDeliveryFailure {
            kind: row
                .try_get::<Option<String>, _>("error_kind")?
                .unwrap_or_else(|| "GITHUB_WRITE_FAILED".into()),
            detail: row
                .try_get::<Option<String>, _>("error_detail")?
                .unwrap_or_else(|| "issue delivery did not complete".into()),
            retryable: status == "FAILED",
        },
        row.try_get("id")?,
    )))
}

async fn update_link_delivery_state(
    connection: &mut SqliteConnection,
    link_id: &str,
    state: IssueLinkState,
    delivery_state: IssueDeliveryState,
) -> Result<(), Error> {
    sqlx::query::query(
        "UPDATE issue_links SET state=?1,delivery_state=?2,updated_at=?3 WHERE id=?4",
    )
    .bind(state.as_str())
    .bind(delivery_state.as_str())
    .bind(Utc::now().to_rfc3339())
    .bind(link_id)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

async fn load_event(
    connection: &mut SqliteConnection,
    event_id: &str,
) -> Result<IssueDeliveryEventRecord, Error> {
    let row = sqlx::query::query("SELECT * FROM issue_delivery_events WHERE id=?1")
        .bind(event_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(Error::NotFound)?;
    event_from_row(&row)
}

fn link_from_row(row: &SqliteRow) -> Result<IssueLinkRecord, Error> {
    let role: String = row.try_get("role")?;
    let state: String = row.try_get("state")?;
    let delivery_state: String = row.try_get("delivery_state")?;
    let resolution: String = row.try_get("resolution")?;
    Ok(IssueLinkRecord {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        request_id: row.try_get("request_id")?,
        campaign_id: row.try_get("campaign_id")?,
        candidate_id: row.try_get("candidate_id")?,
        repository_owner: row.try_get("repository_owner")?,
        repository_name: row.try_get("repository_name")?,
        repository_full_name: row.try_get("repository_full_name")?,
        issue_number: row.try_get::<i64, _>("issue_number")? as u64,
        github_issue_id: row.try_get("github_issue_id")?,
        issue_title: row.try_get("issue_title")?,
        role: IssueRole::parse(&role).ok_or(Error::Invalid("stored issue link role"))?,
        state: IssueLinkState::parse(&state).ok_or(Error::Invalid("stored issue link state"))?,
        delivery_state: IssueDeliveryState::parse(&delivery_state)
            .ok_or(Error::Invalid("stored issue delivery state"))?,
        resolution: IssueLinkResolution::parse(&resolution)
            .ok_or(Error::Invalid("stored issue link resolution"))?,
        work_unit_id: row.try_get("work_unit_id")?,
        pull_request_number: row
            .try_get::<Option<i64>, _>("pull_request_number")?
            .map(|number| number as u64),
        pull_request_url: row.try_get("pull_request_url")?,
        merge_sha: row.try_get("merge_sha")?,
        merged_at: row.try_get("merged_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        latest_error: None,
    })
}

fn event_from_row(row: &SqliteRow) -> Result<IssueDeliveryEventRecord, Error> {
    let action: String = row.try_get("action")?;
    let status: String = row.try_get("status")?;
    Ok(IssueDeliveryEventRecord {
        id: row.try_get("id")?,
        issue_link_id: row.try_get("issue_link_id")?,
        campaign_id: row.try_get("campaign_id")?,
        candidate_id: row.try_get("candidate_id")?,
        action: IssueDeliveryAction::parse(&action)
            .ok_or(Error::Invalid("stored issue delivery action"))?,
        event_key: row.try_get("event_key")?,
        marker: row.try_get("marker")?,
        body: row.try_get("body")?,
        label_name: row.try_get("label_name")?,
        status: IssueDeliveryEventStatus::parse(&status)
            .ok_or(Error::Invalid("stored issue delivery status"))?,
        attempts: u32::try_from(row.try_get::<i64, _>("attempts")?)
            .map_err(|_| Error::Invalid("stored issue delivery attempts"))?,
        external_id: row.try_get("external_id")?,
        error_kind: row.try_get("error_kind")?,
        error_detail: row.try_get("error_detail")?,
        last_attempt_at: row.try_get("last_attempt_at")?,
        applied_at: row.try_get("applied_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}
