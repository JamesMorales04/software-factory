//! Read-only Agent Office participant and provider-tool-event projections.
//!
//! This module never writes Core state. It projects existing durable records
//! (provider sessions, Lead requests/model calls, quality receipts, Worker
//! registrations and provider tool events) into bounded, allowlisted shapes for
//! the operator-authenticated Office API.
//!
//! The projection deliberately does not expose provider directories, prompts,
//! source bodies, command output, arbitrary tool detail or model responses.
//! Free-form provider detail is returned only when it matches a small Core
//! allowlist; everything else is withheld and labelled.

use super::*;

use chrono::Utc;
use sqlx::row::Row;

pub const PARTICIPANT_PAGE_MAX: usize = 200;
pub const PARTICIPANT_OFFSET_MAX: usize = 10_000;
pub const TOOL_EVENT_PAGE_MAX: usize = 200;

const PARTICIPANT_ROLES: &[&str] = &["IMPLEMENTER", "LEAD", "VERIFIER", "REVIEWER", "WORKER"];

const SESSION_STATES: &[&str] = &[
    "INTENT",
    "ACTIVE",
    "CANCEL_REQUESTED",
    "CANCELLED",
    "CLEANED",
    "COMPLETED",
    "FAILED",
    "ORPHANED",
];

const LEAD_STATES: &[&str] = &[
    "SUBMITTED",
    "ASSESSING",
    "PLANNED",
    "FROZEN",
    "READY",
    "RUNNING",
    "BLOCKED",
    "FAILED",
];

const RECEIPT_RESULTS: &[&str] = &["PASS", "FAIL", "CHANGES_REQUESTED", "NOT_TESTED"];

const TOOL_DECISIONS: &[&str] = &["REQUEST", "ALLOW", "DENY", "RESULT", "ERROR"];

const SAFE_TOOL_NAMES: &[&str] = &[
    "write_file",
    "run_command",
    "final",
    "final-explanation",
    "empty-final",
    "apply_patch",
    "read_file",
    "list_files",
    "search",
];

const SAFE_CLEANUP_RECEIPTS: &[&str] = &[
    "session_delete_verified_absent",
    "no_external_session",
    "container_removed_verified_absent",
    "container_cleanup_unconfirmed",
    "sandbox_stopped_cleanup_unconfirmed",
    "unconfined_cleanup_unconfirmed",
];

const SAFE_TOOL_DETAILS: &[&str] = &[
    "tool requested",
    "dynamic tool requested",
    "path authorized",
    "write applied",
    "frozen check authorized",
    "write failed",
    "command failed",
    "provider cancellation was requested",
    "provider deadline exceeded",
    "no file has been written; an empty final response cannot create a candidate",
    "provider wrote one or more files but did not emit a final response; finalizing the applied writes for independent verification",
    "Codex wrote files but did not emit a final response; preserving applied writes",
];

pub fn participant_statuses() -> Vec<&'static str> {
    let mut statuses = Vec::new();
    statuses.extend_from_slice(SESSION_STATES);
    statuses.extend_from_slice(LEAD_STATES);
    statuses.extend_from_slice(RECEIPT_RESULTS);
    statuses.push("REGISTERED");
    statuses.sort_unstable();
    statuses.dedup();
    statuses
}

pub fn is_valid_participant_role(value: &str) -> bool {
    PARTICIPANT_ROLES.contains(&value)
}

pub fn is_valid_participant_status(value: &str) -> bool {
    participant_statuses().contains(&value)
}

#[derive(Debug, Clone, Default)]
pub struct ParticipantQuery {
    pub role: Option<String>,
    pub provider: Option<String>,
    pub status: Option<String>,
    pub campaign_id: Option<String>,
    pub project_id: Option<String>,
    pub participant_id: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParticipantRoute {
    pub provider: String,
    pub model: String,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParticipantUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParticipantSession {
    pub session_record_id: String,
    pub attempt_id: String,
    pub worker_id: String,
    pub generation: i64,
    pub provider: String,
    pub model: String,
    pub provider_mode: String,
    pub state: String,
    pub provider_session_id: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost_micros: u64,
    pub usage_known: bool,
    pub cleanup: String,
    pub cleanup_receipt: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParticipantEvidence {
    pub candidate_id: Option<String>,
    pub candidate_commit: Option<String>,
    pub candidate_tree: Option<String>,
    pub gate_outcome: Option<String>,
    pub gate_decided_at: Option<String>,
    pub verifier_receipt_id: Option<String>,
    pub reviewer_receipt_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParticipantRecord {
    pub participant_id: String,
    pub role: String,
    pub identity_kind: String,
    pub identity_id: String,
    pub principal_id: Option<String>,
    pub project_id: Option<String>,
    pub action_id: Option<String>,
    pub campaign_id: Option<String>,
    pub request_id: Option<String>,
    pub work_unit_id: Option<String>,
    pub run_id: Option<String>,
    pub attempt_id: Option<String>,
    pub attempt_generation: Option<i64>,
    pub worker_id: Option<String>,
    pub requested_route: Option<ParticipantRoute>,
    pub observed_route: Option<ParticipantRoute>,
    pub route_provenance: String,
    pub session: Option<ParticipantSession>,
    pub status: String,
    pub status_source: String,
    pub lease_state: String,
    pub observation: String,
    pub failure_reason: Option<String>,
    pub usage: Option<ParticipantUsage>,
    pub usage_provenance: String,
    pub evidence: Option<ParticipantEvidence>,
    pub model_call_count: Option<u64>,
    pub model_call_state: Option<String>,
    pub observation_note: String,
    pub recorded_at: String,
    pub last_observed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParticipantPageInfo {
    pub limit: usize,
    pub offset: usize,
    pub returned: usize,
    pub has_more: bool,
    pub order: String,
    pub as_of: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParticipantSource {
    pub role: String,
    pub source: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParticipantPage {
    pub participants: Vec<ParticipantRecord>,
    pub page: ParticipantPageInfo,
    pub sources: Vec<ParticipantSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderSessionContext {
    pub participant_id: String,
    pub session_record_id: String,
    pub attempt_id: String,
    pub run_id: String,
    pub campaign_id: String,
    pub worker_id: String,
    pub generation: i64,
    pub provider: String,
    pub model: String,
    pub provider_mode: String,
    pub state: String,
    pub lease_state: String,
    pub cleanup: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderToolEventView {
    pub sequence: i64,
    pub call_ref: String,
    pub tool: String,
    pub tool_digest: String,
    pub decision: String,
    pub detail: Option<String>,
    pub detail_state: String,
    pub recorded_at: String,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderToolEventProjection {
    pub session: ProviderSessionContext,
    pub events: Vec<ProviderToolEventView>,
    pub after_sequence: i64,
    pub limit: usize,
    pub returned: usize,
    pub next_sequence: Option<i64>,
    pub has_more: bool,
    pub discarded: u32,
    pub source: String,
    pub note: String,
}

impl Database {
    pub async fn participants(&self, query: &ParticipantQuery) -> Result<ParticipantPage, Error> {
        let limit = query.limit.clamp(1, PARTICIPANT_PAGE_MAX);
        let offset = query.offset.min(PARTICIPANT_OFFSET_MAX);
        let fetch = offset.saturating_add(limit).saturating_add(1);
        let now_ms = Utc::now().timestamp_millis();
        let mut participants = Vec::new();

        if query
            .role
            .as_deref()
            .is_none_or(|role| role == "IMPLEMENTER")
        {
            participants.extend(self.implementer_participants(query, fetch, now_ms).await?);
        }
        if query.role.as_deref().is_none_or(|role| role == "LEAD") {
            participants.extend(self.lead_participants(query, fetch).await?);
        }
        if query
            .role
            .as_deref()
            .is_none_or(|role| role == "VERIFIER" || role == "REVIEWER")
        {
            participants.extend(self.quality_participants(query, fetch).await?);
        }
        if query.role.as_deref().is_none_or(|role| role == "WORKER") {
            participants.extend(self.worker_participants(query, fetch).await?);
        }

        if let Some(role) = &query.role {
            participants.retain(|participant| &participant.role == role);
        }
        if let Some(provider) = &query.provider {
            participants.retain(|participant| {
                participant
                    .requested_route
                    .as_ref()
                    .or(participant.observed_route.as_ref())
                    .is_some_and(|route| &route.provider == provider)
            });
        }
        if let Some(status) = &query.status {
            participants.retain(|participant| &participant.status == status);
        }
        if let Some(campaign_id) = &query.campaign_id {
            participants.retain(|participant| {
                participant.campaign_id.as_deref() == Some(campaign_id.as_str())
            });
        }
        if let Some(project_id) = &query.project_id {
            participants.retain(|participant| {
                participant.project_id.as_deref() == Some(project_id.as_str())
            });
        }
        if let Some(participant_id) = &query.participant_id {
            participants.retain(|participant| &participant.participant_id == participant_id);
        }

        participants.sort_by(|left, right| {
            right
                .last_observed_at
                .cmp(&left.last_observed_at)
                .then_with(|| left.participant_id.cmp(&right.participant_id))
        });
        let mut seen = std::collections::BTreeSet::new();
        participants.retain(|participant| seen.insert(participant.participant_id.clone()));

        let start = offset.min(participants.len());
        let end = start.saturating_add(limit).min(participants.len());
        let has_more = participants.len() > end;
        let page = participants[start..end].to_vec();
        Ok(ParticipantPage {
            page: ParticipantPageInfo {
                limit,
                offset,
                returned: page.len(),
                has_more,
                order: "last_observed_at DESC, participant_id ASC".into(),
                as_of: Utc::now().to_rfc3339(),
            },
            participants: page,
            sources: vec![
                ParticipantSource {
                    role: "IMPLEMENTER".into(),
                    source: "provider_sessions -> attempts -> runs -> campaigns".into(),
                    note: "One participant per observed provider session generation. External provider liveness is not verified.".into(),
                },
                ParticipantSource {
                    role: "LEAD".into(),
                    source: "lead_requests + model_call_reservations".into(),
                    note: "A Lead request is a recorded planning operation, not a continuously running agent.".into(),
                },
                ParticipantSource {
                    role: "VERIFIER".into(),
                    source: "quality_receipts role=verifier".into(),
                    note: "Independent verification receipt recorded by Core.".into(),
                },
                ParticipantSource {
                    role: "REVIEWER".into(),
                    source: "quality_receipts role=reviewer".into(),
                    note: "Independent review receipt recorded by Core, including deterministic or model provenance.".into(),
                },
                ParticipantSource {
                    role: "WORKER".into(),
                    source: "worker_sessions".into(),
                    note: "Worker registration identity only. Core does not expose heartbeat liveness through this projection.".into(),
                },
            ],
        })
    }

    pub async fn provider_session_context(
        &self,
        session_record_id: &str,
    ) -> Result<ProviderSessionContext, Error> {
        let row = sqlx::query::query(
            "SELECT ps.session_record_id,ps.attempt_id,ps.worker_id,ps.generation,ps.provider,
                    ps.model,ps.provider_mode,ps.state,ps.cleanup_receipt,ps.updated_at,
                    a.run_id,r.campaign_id,l.generation AS lease_generation,
                    l.expires_at_ms
             FROM provider_sessions ps
             JOIN attempts a ON a.id=ps.attempt_id
             JOIN runs r ON r.id=a.run_id
             LEFT JOIN worker_leases l ON l.attempt_id=ps.attempt_id
             WHERE ps.session_record_id=?1",
        )
        .bind(session_record_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        let now_ms = Utc::now().timestamp_millis();
        let generation: i64 = row.try_get("generation")?;
        let lease_generation: Option<i64> = row.try_get("lease_generation")?;
        let expires_at_ms: Option<i64> = row.try_get("expires_at_ms")?;
        let lease_state = match (lease_generation, expires_at_ms) {
            (Some(lease_generation), Some(expires_at_ms))
                if lease_generation == generation && expires_at_ms > now_ms =>
            {
                "LIVE"
            }
            (Some(_), Some(_)) => "EXPIRED",
            _ => "RELEASED",
        };
        let state: String = row.try_get("state")?;
        let cleanup_receipt: Option<String> = row.try_get("cleanup_receipt")?;
        Ok(ProviderSessionContext {
            participant_id: format!("implementer:{session_record_id}"),
            session_record_id: row.try_get("session_record_id")?,
            attempt_id: row.try_get("attempt_id")?,
            run_id: row.try_get("run_id")?,
            campaign_id: row.try_get("campaign_id")?,
            worker_id: row.try_get("worker_id")?,
            generation,
            provider: row.try_get("provider")?,
            model: row.try_get("model")?,
            provider_mode: row.try_get("provider_mode")?,
            lease_state: lease_state.into(),
            cleanup: cleanup_state(&state, cleanup_receipt.as_deref()).into(),
            state,
            updated_at: row.try_get("updated_at")?,
        })
    }

    pub async fn provider_tool_event_projection(
        &self,
        session_record_id: &str,
        after_sequence: i64,
        limit: usize,
    ) -> Result<ProviderToolEventProjection, Error> {
        if after_sequence < 0 {
            return Err(Error::Invalid("tool event cursor"));
        }
        let session = self.provider_session_context(session_record_id).await?;
        let limit = limit.clamp(1, TOOL_EVENT_PAGE_MAX);
        let (legacy_discarded,): (i64,) = sqlx::query_as::query_as(
            "SELECT COUNT(*) FROM provider_tool_events
             WHERE session_record_id=?1 AND sequence<=0",
        )
        .bind(session_record_id)
        .fetch_one(&self.pool)
        .await?;
        let rows = sqlx::query::query(
            "SELECT session_record_id,sequence,call_id,tool,decision,detail,created_at
             FROM provider_tool_events
             WHERE session_record_id=?1 AND sequence>?2
             ORDER BY sequence LIMIT ?3",
        )
        .bind(session_record_id)
        .bind(after_sequence)
        .bind((limit + 1) as i64)
        .fetch_all(&self.pool)
        .await?;
        let has_more = rows.len() > limit;
        let rows = rows.into_iter().take(limit);
        let mut events = Vec::new();
        let discarded = u32::try_from(legacy_discarded).unwrap_or(u32::MAX);
        let mut last_sequence = after_sequence;
        for row in rows {
            let sequence: i64 = row.try_get("sequence")?;
            last_sequence = last_sequence.max(sequence);
            let call_id: String = row.try_get("call_id")?;
            let raw_tool: String = row.try_get("tool")?;
            let decision: String = row.try_get("decision")?;
            let detail: String = row.try_get("detail")?;
            let recorded_at: String = row.try_get("created_at")?;
            if sequence <= after_sequence || sequence <= 0 {
                continue;
            }
            let decision_known = TOOL_DECISIONS.contains(&decision.as_str());
            let tool_known = SAFE_TOOL_NAMES.contains(&raw_tool.as_str());
            let (detail, detail_state) = if !decision_known {
                (None, "REDACTED")
            } else {
                match safe_tool_detail(&detail) {
                    Some(detail) => (Some(detail), "RECORDED"),
                    None if detail.trim().is_empty() => (None, "EMPTY"),
                    None => (None, "WITHHELD"),
                }
            };
            let tool_digest = digest_ref(&raw_tool);
            events.push(ProviderToolEventView {
                sequence,
                call_ref: digest_ref(&call_id),
                tool: if tool_known { raw_tool } else { "OTHER".into() },
                tool_digest,
                decision: if decision_known {
                    decision
                } else {
                    "UNKNOWN".into()
                },
                detail,
                detail_state: detail_state.into(),
                recorded_at,
                provenance: "provider_tool_events".into(),
            });
        }
        let next_sequence = (last_sequence > after_sequence).then_some(last_sequence);
        Ok(ProviderToolEventProjection {
            session,
            after_sequence,
            limit,
            returned: events.len(),
            next_sequence,
            has_more,
            discarded,
            source: "provider_tool_events".into(),
            note: "Bounded Core projection. Free-form provider detail is withheld unless it matches the Core allowlist; call and tool identities are digest references.".into(),
            events,
        })
    }

    async fn implementer_participants(
        &self,
        query: &ParticipantQuery,
        fetch: usize,
        now_ms: i64,
    ) -> Result<Vec<ParticipantRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT ps.session_record_id,ps.attempt_id,ps.worker_id,ps.generation,ps.provider,
                    ps.model,ps.provider_mode,ps.state,ps.provider_session_id,ps.input_tokens,
                    ps.output_tokens,ps.total_tokens,ps.cost_micros,ps.usage_known,
                    ps.cleanup_receipt,ps.error,ps.created_at,ps.updated_at,
                    a.status AS attempt_status,r.id AS run_id,r.campaign_id,c.action_id,
                    ac.project_id,
                    (SELECT lt.unit_id FROM lead_tasks lt WHERE lt.attempt_id=ps.attempt_id
                     ORDER BY lt.sequence LIMIT 1) AS unit_id,
                    cd.id AS candidate_id,cd.commit_sha,cd.tree_sha,cd.implementer_id,
                    g.outcome AS gate_outcome,g.decided_at AS gate_decided_at,
                    l.generation AS lease_generation,l.expires_at_ms,
                    (SELECT qr.receipt_id FROM quality_receipts qr
                     WHERE qr.candidate_id=cd.id AND qr.role='verifier') AS verifier_receipt_id,
                    (SELECT qr.receipt_id FROM quality_receipts qr
                     WHERE qr.candidate_id=cd.id AND qr.role='reviewer') AS reviewer_receipt_id
             FROM provider_sessions ps
             JOIN attempts a ON a.id=ps.attempt_id
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             JOIN actions ac ON ac.id=c.action_id
             LEFT JOIN candidates cd ON cd.attempt_id=ps.attempt_id
             LEFT JOIN gate_decisions g ON g.candidate_id=cd.id
             LEFT JOIN worker_leases l ON l.attempt_id=ps.attempt_id
             WHERE (?1 IS NULL OR r.campaign_id=?1)
               AND (?2 IS NULL OR ac.project_id=?2)
               AND (?3 IS NULL OR ps.provider=?3)
               AND (?4 IS NULL OR ps.state=?4)
               AND (?5 IS NULL OR ?5='implementer:'||ps.session_record_id)
             ORDER BY ps.updated_at DESC,ps.session_record_id ASC
             LIMIT ?6",
        )
        .bind(&query.campaign_id)
        .bind(&query.project_id)
        .bind(&query.provider)
        .bind(&query.status)
        .bind(&query.participant_id)
        .bind(fetch as i64)
        .fetch_all(&self.pool)
        .await?;
        let mut participants = Vec::with_capacity(rows.len());
        for row in rows {
            let session_record_id: String = row.try_get("session_record_id")?;
            if !is_safe_identity(&session_record_id) {
                continue;
            }
            let generation: i64 = row.try_get("generation")?;
            let state: String = row.try_get("state")?;
            let lease_generation: Option<i64> = row.try_get("lease_generation")?;
            let expires_at_ms: Option<i64> = row.try_get("expires_at_ms")?;
            let lease_state = match (lease_generation, expires_at_ms) {
                (Some(lease_generation), Some(expires_at_ms))
                    if lease_generation == generation && expires_at_ms > now_ms =>
                {
                    "LIVE"
                }
                (Some(_), Some(_)) => "EXPIRED",
                _ => "RELEASED",
            };
            let cleanup_receipt: Option<String> = row.try_get("cleanup_receipt")?;
            let cleanup = cleanup_state(&state, cleanup_receipt.as_deref());
            let status_known = SESSION_STATES.contains(&state.as_str());
            let observation = if status_known {
                session_observation(&state, lease_state)
            } else {
                "UNKNOWN"
            };
            let requested_route = ParticipantRoute {
                provider: row.try_get("provider")?,
                model: row.try_get("model")?,
                mode: Some(row.try_get("provider_mode")?),
            };
            let usage_known: bool = row.try_get("usage_known")?;
            let usage = ParticipantUsage {
                input_tokens: to_u64(row.try_get("input_tokens")?)?,
                output_tokens: to_u64(row.try_get("output_tokens")?)?,
                total_tokens: to_u64(row.try_get("total_tokens")?)?,
                cost_micros: to_u64(row.try_get("cost_micros")?)?,
                usage_known,
            };
            let session = ParticipantSession {
                session_record_id: session_record_id.clone(),
                attempt_id: row.try_get("attempt_id")?,
                worker_id: row.try_get("worker_id")?,
                generation,
                provider: requested_route.provider.clone(),
                model: requested_route.model.clone(),
                provider_mode: requested_route.mode.clone().unwrap_or_default(),
                state: state.clone(),
                provider_session_id: safe_identifier(row.try_get("provider_session_id")?, 128),
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
                cost_micros: usage.cost_micros,
                usage_known,
                cleanup: cleanup.into(),
                cleanup_receipt: safe_cleanup_receipt(cleanup_receipt.as_deref()),
                error: bounded_safe(row.try_get("error")?, 500),
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
            };
            participants.push(ParticipantRecord {
                participant_id: format!("implementer:{session_record_id}"),
                role: "IMPLEMENTER".into(),
                identity_kind: "PROVIDER_SESSION".into(),
                identity_id: session_record_id,
                principal_id: bounded_safe(row.try_get("implementer_id")?, 256),
                project_id: optional_text(row.try_get("project_id")?, 256),
                action_id: optional_text(row.try_get("action_id")?, 256),
                campaign_id: optional_text(row.try_get("campaign_id")?, 256),
                request_id: None,
                work_unit_id: optional_text(row.try_get("unit_id")?, 256),
                run_id: optional_text(row.try_get("run_id")?, 256),
                attempt_id: optional_text(row.try_get("attempt_id")?, 256),
                attempt_generation: Some(generation),
                worker_id: optional_text(row.try_get("worker_id")?, 256),
                requested_route: Some(requested_route),
                observed_route: None,
                route_provenance: "PROVIDER_SESSION_BINDING_REQUESTED".into(),
                session: Some(session),
                status: if status_known { state } else { "UNKNOWN".into() },
                status_source: "PROVIDER_SESSION".into(),
                lease_state: lease_state.into(),
                observation: observation.into(),
                failure_reason: bounded_safe(row.try_get("error")?, 500),
                usage: Some(usage),
                usage_provenance: "PROVIDER_SESSION_BINDING".into(),
                evidence: evidence(
                    optional_text(row.try_get("candidate_id")?, 256),
                    optional_text(row.try_get("commit_sha")?, 128),
                    optional_text(row.try_get("tree_sha")?, 128),
                    optional_text(row.try_get("gate_outcome")?, 64),
                    optional_text(row.try_get("gate_decided_at")?, 64),
                    optional_text(row.try_get("verifier_receipt_id")?, 256),
                    optional_text(row.try_get("reviewer_receipt_id")?, 256),
                ),
                model_call_count: None,
                model_call_state: None,
                observation_note: if lease_state == "LIVE" {
                    "Core holds a live attempt lease; external provider state is not independently verified."
                        .into()
                } else if observation == "STALE" {
                    "The recorded session is unresolved but no live lease generation matches it. Review cleanup; this is not proof that the external session stopped."
                        .into()
                } else {
                    "Final recorded provider-session state. Core does not independently verify external termination."
                        .into()
                },
                recorded_at: row.try_get("created_at")?,
                last_observed_at: row.try_get("updated_at")?,
            });
        }
        Ok(participants)
    }

    async fn lead_participants(
        &self,
        query: &ParticipantQuery,
        fetch: usize,
    ) -> Result<Vec<ParticipantRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT lr.id,lr.project_id,lr.action_id,lr.campaign_id,lr.state,
                    lr.failure_reason,lr.created_at,lr.updated_at,
                    mc.reservation_id,mc.principal_id,mc.provider,mc.model,
                    mc.state AS model_call_state,mc.updated_at AS model_call_updated_at,
                    (SELECT COUNT(*) FROM model_call_reservations m
                     WHERE m.role='LEAD' AND m.objective_id=lr.id) AS model_call_count,
                    (SELECT COALESCE(SUM(m.input_tokens),0) FROM model_call_reservations m
                     WHERE m.role='LEAD' AND m.objective_id=lr.id) AS input_tokens,
                    (SELECT COALESCE(SUM(m.output_tokens),0) FROM model_call_reservations m
                     WHERE m.role='LEAD' AND m.objective_id=lr.id) AS output_tokens,
                    (SELECT COALESCE(SUM(m.total_tokens),0) FROM model_call_reservations m
                     WHERE m.role='LEAD' AND m.objective_id=lr.id) AS total_tokens,
                    (SELECT COALESCE(SUM(m.cost_micros),0) FROM model_call_reservations m
                     WHERE m.role='LEAD' AND m.objective_id=lr.id) AS cost_micros,
                    (SELECT MIN(m.usage_known) FROM model_call_reservations m
                     WHERE m.role='LEAD' AND m.objective_id=lr.id) AS usage_known
             FROM lead_requests lr
             LEFT JOIN model_call_reservations mc ON mc.reservation_id=(
               SELECT m.reservation_id FROM model_call_reservations m
               WHERE m.role='LEAD' AND m.objective_id=lr.id
               ORDER BY m.created_at DESC,m.reservation_id DESC LIMIT 1)
             WHERE (?1 IS NULL OR lr.campaign_id=?1)
               AND (?2 IS NULL OR lr.project_id=?2)
               AND (?3 IS NULL OR mc.provider=?3)
               AND (?4 IS NULL OR lr.state=?4)
               AND (?5 IS NULL OR ?5='lead:'||lr.id)
             ORDER BY MAX(lr.updated_at,COALESCE(mc.updated_at,lr.updated_at)) DESC,lr.id ASC
             LIMIT ?6",
        )
        .bind(&query.campaign_id)
        .bind(&query.project_id)
        .bind(&query.provider)
        .bind(&query.status)
        .bind(&query.participant_id)
        .bind(fetch as i64)
        .fetch_all(&self.pool)
        .await?;
        let mut participants = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("id")?;
            if !is_safe_identity(&id) {
                continue;
            }
            let state: String = row.try_get("state")?;
            let status_known = LEAD_STATES.contains(&state.as_str());
            let model_call_count: i64 = row.try_get("model_call_count")?;
            let model_call_state: Option<String> = row.try_get("model_call_state")?;
            let provider: Option<String> = row.try_get("provider")?;
            let model: Option<String> = row.try_get("model")?;
            let requested_route = match (provider, model) {
                (Some(provider), Some(model)) if !provider.is_empty() && !model.is_empty() => {
                    Some(ParticipantRoute {
                        provider,
                        model,
                        mode: None,
                    })
                }
                _ => None,
            };
            let usage = if model_call_count > 0 {
                Some(ParticipantUsage {
                    input_tokens: to_u64(row.try_get("input_tokens")?)?,
                    output_tokens: to_u64(row.try_get("output_tokens")?)?,
                    total_tokens: to_u64(row.try_get("total_tokens")?)?,
                    cost_micros: to_u64(row.try_get("cost_micros")?)?,
                    usage_known: row
                        .try_get::<Option<i64>, _>("usage_known")?
                        .map(|value| value == 1)
                        .unwrap_or(false),
                })
            } else {
                None
            };
            let updated_at: String = row.try_get("updated_at")?;
            let model_call_updated_at: Option<String> = row.try_get("model_call_updated_at")?;
            let last_observed_at = model_call_updated_at
                .filter(|value| value > &updated_at)
                .unwrap_or(updated_at);
            participants.push(ParticipantRecord {
                participant_id: format!("lead:{id}"),
                role: "LEAD".into(),
                identity_kind: "LEAD_REQUEST".into(),
                identity_id: id.clone(),
                principal_id: bounded_safe(row.try_get("principal_id")?, 256),
                project_id: optional_text(row.try_get("project_id")?, 256),
                action_id: optional_text(row.try_get("action_id")?, 256),
                campaign_id: optional_text(row.try_get("campaign_id")?, 256),
                request_id: Some(id),
                work_unit_id: None,
                run_id: None,
                attempt_id: None,
                attempt_generation: None,
                worker_id: None,
                requested_route,
                observed_route: None,
                route_provenance: "LEAD_MODEL_CALL_RESERVATION".into(),
                session: None,
                status: if status_known { state.clone() } else { "UNKNOWN".into() },
                status_source: "LEAD_REQUEST".into(),
                lease_state: "NOT_APPLICABLE".into(),
                observation: if matches!(state.as_str(), "BLOCKED" | "FAILED") {
                    "FINAL".into()
                } else if status_known {
                    "CURRENT".into()
                } else {
                    "UNKNOWN".into()
                },
                failure_reason: bounded_safe(row.try_get("failure_reason")?, 500),
                usage,
                usage_provenance: "LEAD_MODEL_CALLS".into(),
                evidence: None,
                model_call_count: Some(to_u64(model_call_count)?),
                model_call_state,
                observation_note: "A Lead request is a Core-recorded planning or assessment operation. It does not establish a continuously running agent session.".into(),
                recorded_at: row.try_get("created_at")?,
                last_observed_at,
            });
        }
        Ok(participants)
    }

    async fn quality_participants(
        &self,
        query: &ParticipantQuery,
        fetch: usize,
    ) -> Result<Vec<ParticipantRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT qr.receipt_id,qr.candidate_id,qr.role,qr.principal_id,qr.result,
                    qr.evidence,qr.mechanism,qr.model_provider,qr.model_name,qr.input_tokens,
                    qr.output_tokens,qr.total_tokens,qr.cost_micros,qr.usage_known,qr.received_at,
                    cd.attempt_id,cd.commit_sha,cd.tree_sha,
                    a.run_id,r.campaign_id,c.action_id,ac.project_id,
                    (SELECT lt.unit_id FROM lead_tasks lt WHERE lt.attempt_id=cd.attempt_id
                     ORDER BY lt.sequence LIMIT 1) AS unit_id,
                    g.outcome AS gate_outcome,g.decided_at AS gate_decided_at,
                    (SELECT qr2.receipt_id FROM quality_receipts qr2
                     WHERE qr2.candidate_id=cd.id AND qr2.role='verifier') AS verifier_receipt_id,
                    (SELECT qr2.receipt_id FROM quality_receipts qr2
                     WHERE qr2.candidate_id=cd.id AND qr2.role='reviewer') AS reviewer_receipt_id
             FROM quality_receipts qr
             JOIN candidates cd ON cd.id=qr.candidate_id
             JOIN attempts a ON a.id=cd.attempt_id
             JOIN runs r ON r.id=a.run_id
             JOIN campaigns c ON c.id=r.campaign_id
             JOIN actions ac ON ac.id=c.action_id
             LEFT JOIN gate_decisions g ON g.candidate_id=cd.id
             WHERE (?1 IS NULL OR r.campaign_id=?1)
               AND (?2 IS NULL OR ac.project_id=?2)
               AND (?3 IS NULL OR qr.model_provider=?3)
               AND (?4 IS NULL OR qr.result=?4)
               AND (?5 IS NULL OR ?5 IN ('verifier:'||qr.receipt_id,'reviewer:'||qr.receipt_id))
               AND (?7 IS NULL
                    OR (qr.role='verifier' AND ?7='VERIFIER')
                    OR (qr.role='reviewer' AND ?7='REVIEWER'))
             ORDER BY qr.received_at DESC,qr.receipt_id ASC
             LIMIT ?6",
        )
        .bind(&query.campaign_id)
        .bind(&query.project_id)
        .bind(&query.provider)
        .bind(&query.status)
        .bind(&query.participant_id)
        .bind(fetch as i64)
        .bind(&query.role)
        .fetch_all(&self.pool)
        .await?;
        let mut participants = Vec::with_capacity(rows.len());
        for row in rows {
            let receipt_id: String = row.try_get("receipt_id")?;
            if !is_safe_identity(&receipt_id) {
                continue;
            }
            let role: String = row.try_get("role")?;
            if !matches!(role.as_str(), "verifier" | "reviewer") {
                continue;
            }
            let result: String = row.try_get("result")?;
            let status_known = RECEIPT_RESULTS.contains(&result.as_str());
            let mechanism = if row.try_get::<String, _>("mechanism")? == "MODEL" {
                ReviewMechanism::Model
            } else {
                ReviewMechanism::Deterministic
            };
            let model_provider: String = row.try_get("model_provider")?;
            let model_name: String = row.try_get("model_name")?;
            let observed_route = if mechanism == ReviewMechanism::Model
                && !model_provider.is_empty()
                && !model_name.is_empty()
            {
                Some(ParticipantRoute {
                    provider: model_provider,
                    model: model_name,
                    mode: None,
                })
            } else {
                None
            };
            let usage_known: bool = row.try_get("usage_known")?;
            let usage = if mechanism == ReviewMechanism::Model || usage_known {
                Some(ParticipantUsage {
                    input_tokens: to_u64(row.try_get("input_tokens")?)?,
                    output_tokens: to_u64(row.try_get("output_tokens")?)?,
                    total_tokens: to_u64(row.try_get("total_tokens")?)?,
                    cost_micros: to_u64(row.try_get("cost_micros")?)?,
                    usage_known,
                })
            } else {
                None
            };
            participants.push(ParticipantRecord {
                participant_id: format!("{role}:{receipt_id}"),
                role: if role == "verifier" {
                    "VERIFIER".into()
                } else {
                    "REVIEWER".into()
                },
                identity_kind: "QUALITY_RECEIPT".into(),
                identity_id: receipt_id,
                principal_id: bounded_safe(row.try_get("principal_id")?, 256),
                project_id: optional_text(row.try_get("project_id")?, 256),
                action_id: optional_text(row.try_get("action_id")?, 256),
                campaign_id: optional_text(row.try_get("campaign_id")?, 256),
                request_id: None,
                work_unit_id: optional_text(row.try_get("unit_id")?, 256),
                run_id: optional_text(row.try_get("run_id")?, 256),
                attempt_id: optional_text(row.try_get("attempt_id")?, 256),
                attempt_generation: None,
                worker_id: None,
                requested_route: None,
                observed_route,
                route_provenance: if mechanism == ReviewMechanism::Model {
                    "QUALITY_RECEIPT_MODEL_OBSERVED".into()
                } else {
                    "NOT_EXPOSED".into()
                },
                session: None,
                status: if status_known { result } else { "UNKNOWN".into() },
                status_source: "QUALITY_RECEIPT".into(),
                lease_state: "NOT_APPLICABLE".into(),
                observation: "FINAL".into(),
                failure_reason: None,
                usage,
                usage_provenance: if mechanism == ReviewMechanism::Model {
                    "QUALITY_RECEIPT_MODEL".into()
                } else {
                    "NOT_APPLICABLE".into()
                },
                evidence: evidence(
                    Some(row.try_get("candidate_id")?),
                    optional_text(row.try_get("commit_sha")?, 128),
                    optional_text(row.try_get("tree_sha")?, 128),
                    optional_text(row.try_get("gate_outcome")?, 64),
                    optional_text(row.try_get("gate_decided_at")?, 64),
                    optional_text(row.try_get("verifier_receipt_id")?, 256),
                    optional_text(row.try_get("reviewer_receipt_id")?, 256),
                ),
                model_call_count: None,
                model_call_state: None,
                observation_note: if mechanism == ReviewMechanism::Model {
                    "Independent review receipt recorded with model provenance. The recorded provider/model is the receipt's observed review route."
                        .into()
                } else {
                    "Independent deterministic review receipt recorded by Core.".into()
                },
                recorded_at: row.try_get("received_at")?,
                last_observed_at: row.try_get("received_at")?,
            });
        }
        Ok(participants)
    }

    async fn worker_participants(
        &self,
        query: &ParticipantQuery,
        fetch: usize,
    ) -> Result<Vec<ParticipantRecord>, Error> {
        if query.campaign_id.is_some()
            || query.project_id.is_some()
            || query.provider.is_some()
            || query
                .status
                .as_deref()
                .is_some_and(|status| status != "REGISTERED")
        {
            return Ok(Vec::new());
        }
        let rows = sqlx::query::query(
            "SELECT worker_id,updated_at FROM worker_sessions
             WHERE (?1 IS NULL OR ?1='worker:'||worker_id)
             ORDER BY updated_at DESC,worker_id ASC LIMIT ?2",
        )
        .bind(&query.participant_id)
        .bind(fetch as i64)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .filter_map(|row| {
                let worker_id: String = match row.try_get("worker_id") {
                    Ok(value) => value,
                    Err(error) => return Some(Err(error.into())),
                };
                if !is_safe_identity(&worker_id) {
                    return None;
                }
                let updated_at: String = match row.try_get("updated_at") {
                    Ok(value) => value,
                    Err(error) => return Some(Err(error.into())),
                };
                Some(Ok(ParticipantRecord {
                    participant_id: format!("worker:{worker_id}"),
                    role: "WORKER".into(),
                    identity_kind: "WORKER_REGISTRATION".into(),
                    identity_id: worker_id.clone(),
                    principal_id: Some(worker_id.clone()),
                    project_id: None,
                    action_id: None,
                    campaign_id: None,
                    request_id: None,
                    work_unit_id: None,
                    run_id: None,
                    attempt_id: None,
                    attempt_generation: None,
                    worker_id: Some(worker_id),
                    requested_route: None,
                    observed_route: None,
                    route_provenance: "NOT_EXPOSED".into(),
                    session: None,
                    status: "REGISTERED".into(),
                    status_source: "WORKER_SESSION_REGISTRATION".into(),
                    lease_state: "NOT_APPLICABLE".into(),
                    observation: "NOT_EXPOSED".into(),
                    failure_reason: None,
                    usage: None,
                    usage_provenance: "NOT_APPLICABLE".into(),
                    evidence: None,
                    model_call_count: None,
                    model_call_state: None,
                    observation_note: "Worker registration identity only. The timestamp records registration or token rotation, not heartbeat liveness.".into(),
                    recorded_at: updated_at.clone(),
                    last_observed_at: updated_at,
                }))
            })
            .collect::<Result<Vec<_>, Error>>()
    }
}

fn session_observation(state: &str, lease_state: &str) -> &'static str {
    match state {
        "INTENT" | "ACTIVE" | "CANCEL_REQUESTED" | "ORPHANED" => {
            if lease_state == "LIVE" {
                "CURRENT"
            } else {
                "STALE"
            }
        }
        "CANCELLED" | "CLEANED" | "COMPLETED" | "FAILED" => "FINAL",
        _ => "UNKNOWN",
    }
}

fn cleanup_state(state: &str, cleanup_receipt: Option<&str>) -> &'static str {
    if matches!(state, "INTENT" | "ACTIVE") {
        return "PENDING";
    }
    match cleanup_receipt {
        Some(receipt) if SAFE_CLEANUP_RECEIPTS.contains(&receipt) => "RECORDED",
        Some(_) => "UNRECOGNIZED",
        None if matches!(
            state,
            "CANCELLED" | "CLEANED" | "COMPLETED" | "FAILED" | "ORPHANED"
        ) =>
        {
            "MISSING"
        }
        None => "PENDING",
    }
}

fn safe_cleanup_receipt(value: Option<&str>) -> Option<String> {
    let value = value?;
    SAFE_CLEANUP_RECEIPTS
        .contains(&value)
        .then(|| value.to_owned())
}

fn safe_tool_detail(detail: &str) -> Option<String> {
    let value = detail.trim();
    if SAFE_TOOL_DETAILS.contains(&value) {
        return Some(value.to_owned());
    }
    if let Some(exit) = value.strip_prefix("exit=")
        && exit.len() <= 16
        && exit
            .chars()
            .all(|character| character.is_ascii_digit() || character == '-')
    {
        return Some(value.to_owned());
    }
    None
}

fn digest_ref(value: &str) -> String {
    super::sha256_hex(value.as_bytes())[..16].to_owned()
}

fn evidence(
    candidate_id: Option<String>,
    candidate_commit: Option<String>,
    candidate_tree: Option<String>,
    gate_outcome: Option<String>,
    gate_decided_at: Option<String>,
    verifier_receipt_id: Option<String>,
    reviewer_receipt_id: Option<String>,
) -> Option<ParticipantEvidence> {
    if candidate_id.is_none()
        && gate_outcome.is_none()
        && verifier_receipt_id.is_none()
        && reviewer_receipt_id.is_none()
    {
        return None;
    }
    Some(ParticipantEvidence {
        candidate_id,
        candidate_commit,
        candidate_tree,
        gate_outcome,
        gate_decided_at,
        verifier_receipt_id,
        reviewer_receipt_id,
    })
}

fn is_safe_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ':' | '-')
        })
}

fn safe_identifier(value: Option<String>, max: usize) -> Option<String> {
    let value = value?;
    let value = value.trim();
    if value.is_empty()
        || value.len() > max
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | ':' | '-')
        })
    {
        return None;
    }
    Some(value.to_owned())
}

fn optional_text(value: Option<String>, max: usize) -> Option<String> {
    let value = value?;
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    Some(value.chars().take(max).collect())
}

fn bounded_safe(value: Option<String>, max: usize) -> Option<String> {
    let value = value?;
    let redacted = crate::telemetry::redact(&value);
    let normalized: String = redacted
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let collapsed = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    let mut bounded: String = collapsed.chars().take(max).collect();
    if collapsed.chars().count() > max {
        bounded.push('…');
    }
    Some(bounded)
}

fn to_u64(value: i64) -> Result<u64, Error> {
    u64::try_from(value).map_err(|_| Error::Invalid("negative participant counter"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        path: std::path::PathBuf,
        db: Database,
    }

    impl Fixture {
        async fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "factory-participants-{}.sqlite",
                uuid::Uuid::new_v4()
            ));
            let db = Database::open(&path).await.unwrap();
            sqlx::query::query(
                "INSERT INTO projects(id,repository,base_commit,created_at,identity)
                 VALUES ('p1','/repo',?1,'2026-10-27T09:00:00+00:00','')",
            )
            .bind("a".repeat(40))
            .execute(&db.pool)
            .await
            .unwrap();
            for (id, objective) in [("a1", "objective one"), ("a2", "objective two")] {
                sqlx::query::query(
                    "INSERT INTO actions(id,project_id,objective,provider,model,risk,status,
                        objective_sha256,created_at)
                     VALUES (?1,'p1',?2,'opencode','deepseek-v4.1-flash','R0',
                        'AUTHORIZED','hash','2026-10-27T09:00:00+00:00')",
                )
                .bind(id)
                .bind(objective)
                .execute(&db.pool)
                .await
                .unwrap();
            }
            for (id, created) in [
                ("c1", "2026-10-27T09:00:00+00:00"),
                ("c2", "2026-10-27T09:00:01+00:00"),
            ] {
                sqlx::query::query(
                    "INSERT INTO campaigns(id,action_id,status,created_at) VALUES (?1,'a1','AUTHORIZED',?2)",
                )
                .bind(id)
                .bind(created)
                .execute(&db.pool)
                .await
                .unwrap();
            }
            sqlx::query::query(
                "INSERT INTO standing_delegations
                 (id,principal_id,audit_id,project_id,revision,content_hash,content_json,created_at)
                 VALUES ('d1','owner','audit','p1',1,'hash','{}','2026-10-27T09:00:00+00:00')",
            )
            .execute(&db.pool)
            .await
            .unwrap();
            for (campaign, authorization) in [("c1", "auth-1"), ("c2", "auth-2")] {
                sqlx::query::query(
                    "INSERT INTO campaign_authorizations
                     (campaign_id,authorization_id,risk_class,execution_mode,actor,rationale,
                      authorized_paths_json,max_attempts,created_at,provider,model,provider_mode,
                      delegation_id,max_sessions,max_turns,max_input_tokens,max_output_tokens,
                      max_cost_micros,deadline_secs,allowed_tools_json)
                     VALUES (?1,?2,'R0','TRUSTED_LOCAL_PROVIDER','owner','test','[\"src\"]',1,
                      '2026-10-27T09:00:00+00:00','opencode','deepseek-v4.1-flash','build','d1',
                      4,4,100000,100000,100000,600,'[\"write_file\"]')",
                )
                .bind(campaign)
                .bind(authorization)
                .execute(&db.pool)
                .await
                .unwrap();
            }
            sqlx::query::query(
                "INSERT INTO worker_sessions(worker_id,token_sha256,updated_at)
                 VALUES ('w1','hash','2026-10-27T10:04:00+00:00')",
            )
            .execute(&db.pool)
            .await
            .unwrap();
            let now_ms = Utc::now().timestamp_millis();
            for (run, campaign, attempt, attempt_status, lease_generation, lease_expires) in [
                ("r1", "c1", "at1", "LEASED", 2_i64, now_ms + 600_000),
                ("r2", "c2", "at2", "LEASED", 1_i64, now_ms - 60_000),
            ] {
                sqlx::query::query(
                    "INSERT INTO runs(id,campaign_id,status,created_at)
                     VALUES (?1,?2,'RUNNING','2026-10-27T10:00:00+00:00')",
                )
                .bind(run)
                .bind(campaign)
                .execute(&db.pool)
                .await
                .unwrap();
                sqlx::query::query(
                    "INSERT INTO attempts(id,run_id,status,created_at)
                     VALUES (?1,?2,?3,'2026-10-27T10:00:00+00:00')",
                )
                .bind(attempt)
                .bind(run)
                .bind(attempt_status)
                .execute(&db.pool)
                .await
                .unwrap();
                sqlx::query::query(
                    "INSERT INTO worker_leases(attempt_id,worker_id,generation,expires_at_ms)
                     VALUES (?1,'w1',?2,?3)",
                )
                .bind(attempt)
                .bind(lease_generation)
                .bind(lease_expires)
                .execute(&db.pool)
                .await
                .unwrap();
            }
            for (attempt, generation, state, updated, cleanup, error) in [
                (
                    "at1",
                    2_i64,
                    "ACTIVE",
                    "2026-10-27T10:05:00+00:00",
                    Some("session_delete_verified_absent"),
                    "",
                ),
                (
                    "at2",
                    2_i64,
                    "ACTIVE",
                    "2026-10-27T10:01:00+00:00",
                    None,
                    "provider uncertain",
                ),
            ] {
                sqlx::query::query(
                    "INSERT INTO provider_sessions
                     (session_record_id,attempt_id,worker_id,generation,provider,model,provider_mode,
                      state,provider_session_id,provider_directory,turn_id,input_tokens,output_tokens,
                      total_tokens,cost_micros,usage_known,cleanup_receipt,error,created_at,updated_at)
                     VALUES (?1,?2,'w1',?3,'opencode','deepseek-v4.1-flash','build',?4,'ses-1',
                      '/private/provider-dir','turn-1',100,50,150,25,1,?5,?6,
                      '2026-10-27T10:00:00+00:00',?7)",
                )
                .bind(format!("s-{attempt}"))
                .bind(attempt)
                .bind(generation)
                .bind(state)
                .bind(cleanup)
                .bind(error)
                .bind(updated)
                .execute(&db.pool)
                .await
                .unwrap();
            }
            for (request, campaign, state, failure) in [
                ("lr1", "c1", "READY", None::<&str>),
                ("lr2", "c2", "BLOCKED", Some("waiting for owner")),
            ] {
                sqlx::query::query(
                    "INSERT INTO lead_requests
                     (id,project_id,action_id,delegation_id,objective,request_hash,
                      project_config_hash,state,failure_reason,campaign_id,created_at,updated_at)
                     VALUES (?1,'p1',?5,'d1','objective','hash','hash',?2,?3,?4,
                      '2026-10-27T09:30:00+00:00','2026-10-27T09:31:00+00:00')",
                )
                .bind(request)
                .bind(state)
                .bind(failure)
                .bind(campaign)
                .bind(if request == "lr1" { "a1" } else { "a2" })
                .execute(&db.pool)
                .await
                .unwrap();
            }
            sqlx::query::query(
                "INSERT INTO model_call_reservations
                 (reservation_id,role,principal_id,project_id,objective_id,scope_id,provider,model,
                  purpose,request_sha256,max_input_tokens,max_output_tokens,max_total_tokens,
                  max_cost_micros,input_tokens,output_tokens,total_tokens,cost_micros,usage_known,
                  state,expires_at_ms,created_at,updated_at)
                 VALUES ('mc1','LEAD','lead-1','p1','lr1','lr1','opencode','deepseek-v4.1-flash',
                  'lead','hash',1000,1000,2000,100,10,5,15,3,1,'SETTLED',?1,
                  '2026-10-27T09:30:30+00:00','2026-10-27T09:32:00+00:00')",
            )
            .bind(now_ms + 600_000)
            .execute(&db.pool)
            .await
            .unwrap();
            sqlx::query::query(
                "INSERT INTO candidates
                 (id,attempt_id,action_spec_sha256,commit_sha,tree_sha,implementer_id,created_at)
                 VALUES ('cd1','at1','hash',?1,?2,'implementer-1',
                  '2026-10-27T10:06:00+00:00')",
            )
            .bind("b".repeat(40))
            .bind("c".repeat(40))
            .execute(&db.pool)
            .await
            .unwrap();
            sqlx::query::query(
                "INSERT INTO gate_decisions(candidate_id,outcome,reasons_json,decided_at)
                 VALUES ('cd1','ACCEPTED','[]','2026-10-27T10:09:00+00:00')",
            )
            .execute(&db.pool)
            .await
            .unwrap();
            for (receipt, role, result, mechanism, provider, model) in [
                ("v1", "verifier", "PASS", "DETERMINISTIC", "", ""),
                (
                    "rv1",
                    "reviewer",
                    "PASS",
                    "MODEL",
                    "opencode",
                    "review-model",
                ),
            ] {
                sqlx::query::query(
                    "INSERT INTO quality_receipts
                     (receipt_id,candidate_id,role,principal_id,action_spec_sha256,commit_sha,
                      tree_sha,result,evidence,received_at,mechanism,model_provider,model_name,
                      input_tokens,output_tokens,total_tokens,cost_micros,usage_known)
                     VALUES (?1,'cd1',?2,?2,'hash',?3,?4,?5,'EXECUTED',
                      '2026-10-27T10:08:00+00:00',?6,?7,?8,20,10,30,6,1)",
                )
                .bind(receipt)
                .bind(role)
                .bind("b".repeat(40))
                .bind("c".repeat(40))
                .bind(result)
                .bind(mechanism)
                .bind(provider)
                .bind(model)
                .execute(&db.pool)
                .await
                .unwrap();
            }
            for (task, attempt, unit, sequence) in [
                ("task-1", "at1", "unit-a", 0_i64),
                ("task-2", "at2", "unit-b", 1_i64),
            ] {
                sqlx::query::query(
                    "INSERT INTO lead_tasks
                     (task_id,campaign_id,request_id,unit_id,sequence,status,base_commit,spec_json,
                      spec_sha256,depends_on_json,run_id,attempt_id,created_at,updated_at)
                     VALUES (?1,?2,?3,?4,?5,'RUNNING',?6,'{}','hash','[]',
                      ?7,?8,'2026-10-27T09:00:00+00:00','2026-10-27T10:00:00+00:00')",
                )
                .bind(task)
                .bind(if attempt == "at1" { "c1" } else { "c2" })
                .bind(if attempt == "at1" { "lr1" } else { "lr2" })
                .bind(unit)
                .bind(sequence)
                .bind("a".repeat(40))
                .bind(if attempt == "at1" { "r1" } else { "r2" })
                .bind(attempt)
                .execute(&db.pool)
                .await
                .unwrap();
            }
            Self { path, db }
        }

        async fn close(self) {
            self.db.close().await;
            let _ = std::fs::remove_file(self.path);
        }
    }

    #[tokio::test]
    async fn participant_projection_binds_real_records_without_synthesizing_sessions() {
        let fixture = Fixture::new().await;
        let page = fixture
            .db
            .participants(&ParticipantQuery {
                limit: 50,
                ..Default::default()
            })
            .await
            .unwrap();
        let by_id: std::collections::BTreeMap<_, _> = page
            .participants
            .iter()
            .map(|participant| (participant.participant_id.as_str(), participant))
            .collect();
        let implementer = by_id["implementer:s-at1"];
        assert_eq!(implementer.role, "IMPLEMENTER");
        assert_eq!(implementer.campaign_id.as_deref(), Some("c1"));
        assert_eq!(implementer.project_id.as_deref(), Some("p1"));
        assert_eq!(implementer.work_unit_id.as_deref(), Some("unit-a"));
        assert_eq!(implementer.run_id.as_deref(), Some("r1"));
        assert_eq!(implementer.attempt_id.as_deref(), Some("at1"));
        assert_eq!(implementer.attempt_generation, Some(2));
        assert_eq!(implementer.worker_id.as_deref(), Some("w1"));
        assert_eq!(implementer.status, "ACTIVE");
        assert_eq!(implementer.lease_state, "LIVE");
        assert_eq!(implementer.observation, "CURRENT");
        assert_eq!(
            implementer.requested_route.as_ref().unwrap().provider,
            "opencode"
        );
        assert!(implementer.observed_route.is_none());
        let session = implementer.session.as_ref().unwrap();
        assert_eq!(session.cleanup, "PENDING");
        assert_eq!(session.provider_session_id.as_deref(), Some("ses-1"));
        assert_eq!(implementer.usage.as_ref().unwrap().total_tokens, 150);
        let evidence = implementer.evidence.as_ref().unwrap();
        assert_eq!(evidence.candidate_id.as_deref(), Some("cd1"));
        assert_eq!(evidence.gate_outcome.as_deref(), Some("ACCEPTED"));
        assert_eq!(evidence.verifier_receipt_id.as_deref(), Some("v1"));
        assert_eq!(evidence.reviewer_receipt_id.as_deref(), Some("rv1"));

        let stale = by_id["implementer:s-at2"];
        assert_eq!(stale.status, "ACTIVE");
        assert_eq!(stale.lease_state, "EXPIRED");
        assert_eq!(stale.observation, "STALE");
        assert!(stale.evidence.is_none());
        assert_eq!(stale.session.as_ref().unwrap().cleanup, "PENDING");
        assert_eq!(stale.failure_reason.as_deref(), Some("provider uncertain"));

        let lead = by_id["lead:lr1"];
        assert_eq!(lead.role, "LEAD");
        assert!(lead.session.is_none());
        assert_eq!(lead.status, "READY");
        assert_eq!(lead.model_call_count, Some(1));
        assert_eq!(lead.requested_route.as_ref().unwrap().provider, "opencode");
        assert_eq!(lead.usage.as_ref().unwrap().total_tokens, 15);

        let verifier = by_id["verifier:v1"];
        assert_eq!(verifier.role, "VERIFIER");
        assert_eq!(verifier.status, "PASS");
        assert!(verifier.observed_route.is_none());
        assert_eq!(verifier.route_provenance, "NOT_EXPOSED");
        assert_eq!(
            verifier.evidence.as_ref().unwrap().gate_outcome.as_deref(),
            Some("ACCEPTED")
        );

        let reviewer = by_id["reviewer:rv1"];
        assert_eq!(reviewer.role, "REVIEWER");
        assert_eq!(
            reviewer.observed_route.as_ref().unwrap().provider,
            "opencode"
        );
        assert_eq!(reviewer.usage.as_ref().unwrap().total_tokens, 30);

        let worker = by_id["worker:w1"];
        assert_eq!(worker.role, "WORKER");
        assert_eq!(worker.status, "REGISTERED");
        assert_eq!(worker.observation, "NOT_EXPOSED");
        assert!(worker.session.is_none());
        fixture.close().await;
    }

    #[tokio::test]
    async fn participant_projection_filters_are_exact_and_campaign_scoped() {
        let fixture = Fixture::new().await;
        let campaign = fixture
            .db
            .participants(&ParticipantQuery {
                campaign_id: Some("c1".into()),
                limit: 50,
                ..Default::default()
            })
            .await
            .unwrap();
        let ids: Vec<_> = campaign
            .participants
            .iter()
            .map(|participant| participant.participant_id.as_str())
            .collect();
        assert!(ids.contains(&"implementer:s-at1"));
        assert!(ids.contains(&"lead:lr1"));
        assert!(ids.contains(&"verifier:v1"));
        assert!(ids.contains(&"reviewer:rv1"));
        assert!(
            !ids.iter()
                .any(|id| id.contains("at2") || id.contains("lr2"))
        );
        assert!(!ids.iter().any(|id| id.starts_with("worker:")));

        let cross = fixture
            .db
            .participants(&ParticipantQuery {
                campaign_id: Some("c2".into()),
                participant_id: Some("implementer:s-at1".into()),
                limit: 50,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(cross.participants.is_empty());

        let implementers = fixture
            .db
            .participants(&ParticipantQuery {
                role: Some("IMPLEMENTER".into()),
                limit: 50,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            implementers
                .participants
                .iter()
                .all(|participant| participant.role == "IMPLEMENTER")
        );

        let active = fixture
            .db
            .participants(&ParticipantQuery {
                status: Some("ACTIVE".into()),
                limit: 50,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(active.participants.len(), 2);

        let provider = fixture
            .db
            .participants(&ParticipantQuery {
                provider: Some("opencode".into()),
                limit: 50,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(provider.participants.iter().all(|participant| {
            participant
                .requested_route
                .as_ref()
                .or(participant.observed_route.as_ref())
                .is_some_and(|route| route.provider == "opencode")
        }));
        assert!(
            !provider
                .participants
                .iter()
                .any(|participant| participant.role == "WORKER")
        );
        fixture.close().await;
    }

    #[tokio::test]
    async fn participant_projection_paginates_deterministically_without_duplicates() {
        let fixture = Fixture::new().await;
        let mut collected = Vec::new();
        for offset in [0_usize, 2, 4] {
            let page = fixture
                .db
                .participants(&ParticipantQuery {
                    limit: 2,
                    offset,
                    ..Default::default()
                })
                .await
                .unwrap();
            assert!(page.page.returned <= 2);
            collected.extend(
                page.participants
                    .into_iter()
                    .map(|participant| participant.participant_id),
            );
        }
        let unique: std::collections::BTreeSet<_> = collected.iter().collect();
        assert_eq!(unique.len(), collected.len());

        let first = fixture
            .db
            .participants(&ParticipantQuery {
                limit: 100,
                ..Default::default()
            })
            .await
            .unwrap();
        let ordered: Vec<_> = first
            .participants
            .iter()
            .map(|participant| participant.participant_id.clone())
            .collect();
        for id in collected {
            assert!(ordered.contains(&id));
        }
        fixture.close().await;
    }

    #[tokio::test]
    async fn tool_event_projection_is_bounded_monotonic_and_redacted() {
        let fixture = Fixture::new().await;
        for (call, tool, decision, detail) in [
            ("call-1", "write_file", "REQUEST", "tool requested"),
            ("call-1", "write_file", "ALLOW", "path authorized"),
            (
                "call-1",
                "final",
                "RESULT",
                "source body: fn secret() { api_key=super-secret }",
            ),
            (
                "call-2",
                "unexpected_tool",
                "ERROR",
                "invalid provider action: Bearer abcdefghijklmnop",
            ),
            (
                "call-3",
                "run_command",
                "DENY",
                "denied because /private/path is outside allowed paths",
            ),
        ] {
            fixture
                .db
                .record_provider_tool_event(&ProviderToolEventInput {
                    session_record_id: "s-at1".into(),
                    worker_id: "w1".into(),
                    generation: 2,
                    call_id: call.into(),
                    tool: tool.into(),
                    decision: decision.into(),
                    detail: detail.into(),
                })
                .await
                .unwrap();
        }
        let first = fixture
            .db
            .provider_tool_event_projection("s-at1", 0, 2)
            .await
            .unwrap();
        assert_eq!(first.returned, 2);
        assert!(first.has_more);
        assert_eq!(first.next_sequence, Some(2));
        assert_eq!(first.events[0].sequence, 1);
        assert_eq!(first.events[0].decision, "REQUEST");
        assert_eq!(first.events[0].detail.as_deref(), Some("tool requested"));
        assert_eq!(first.events[0].detail_state, "RECORDED");
        assert_eq!(first.events[0].call_ref, first.events[1].call_ref);
        assert_eq!(first.session.participant_id, "implementer:s-at1");
        assert_eq!(first.session.lease_state, "LIVE");

        let rest = fixture
            .db
            .provider_tool_event_projection("s-at1", 2, 10)
            .await
            .unwrap();
        assert_eq!(rest.returned, 3);
        assert!(!rest.has_more);
        assert_eq!(rest.events[0].sequence, 3);
        assert_eq!(rest.events[0].detail_state, "WITHHELD");
        assert!(rest.events[0].detail.is_none());
        assert_eq!(rest.events[1].tool, "OTHER");
        assert!(!rest.events[1].tool_digest.is_empty());
        assert_eq!(rest.events[1].detail_state, "WITHHELD");
        assert_eq!(rest.events[2].detail_state, "WITHHELD");
        let serialized = serde_json::to_string(&rest).unwrap();
        assert!(!serialized.contains("super-secret"));
        assert!(!serialized.contains("abcdefghijklmnop"));
        assert!(!serialized.contains("/private/path"));
        assert!(!serialized.contains("source body"));

        let empty = fixture
            .db
            .provider_tool_event_projection("s-at1", 5, 10)
            .await
            .unwrap();
        assert!(empty.events.is_empty());
        assert!(empty.next_sequence.is_none());
        assert!(!empty.has_more);

        let missing = fixture
            .db
            .provider_tool_event_projection("missing", 0, 10)
            .await;
        assert!(matches!(missing, Err(Error::NotFound)));
        fixture.close().await;
    }

    #[tokio::test]
    async fn tool_event_projection_fails_closed_for_malformed_legacy_rows() {
        let fixture = Fixture::new().await;
        let mut connection = fixture.db.pool.acquire().await.unwrap();
        sqlx::query::query("PRAGMA ignore_check_constraints=ON")
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::query::query(
            "INSERT INTO provider_tool_events
             (session_record_id,sequence,call_id,tool,decision,detail,created_at)
             VALUES ('s-at1',1,'call-x','leaky_tool','LEAK','raw source body','2026-10-27T10:00:00+00:00')",
        )
        .execute(&mut *connection)
        .await
        .unwrap();
        sqlx::query::query(
            "INSERT INTO provider_tool_events
             (session_record_id,sequence,call_id,tool,decision,detail,created_at)
             VALUES ('s-at1',-1,'call-negative','write_file','RESULT','negative sequence','2026-10-27T09:59:00+00:00')",
        )
        .execute(&mut *connection)
        .await
        .unwrap();
        drop(connection);
        let projection = fixture
            .db
            .provider_tool_event_projection("s-at1", 0, 10)
            .await
            .unwrap();
        assert_eq!(projection.returned, 1);
        assert_eq!(projection.discarded, 1);
        assert_eq!(projection.events[0].decision, "UNKNOWN");
        assert_eq!(projection.events[0].tool, "OTHER");
        assert_eq!(projection.events[0].detail_state, "REDACTED");
        assert!(projection.events[0].detail.is_none());
        let serialized = serde_json::to_string(&projection).unwrap();
        assert!(!serialized.contains("raw source body"));
        assert!(!serialized.contains("leaky_tool"));
        fixture.close().await;
    }
}
