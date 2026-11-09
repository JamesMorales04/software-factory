//! Durable KnowledgePacks, temporary skills, adoptions, uses and promotion.
//!
//! Context7 content is untrusted evidence. The deterministic gap gate, read-only
//! tool gate, license review, weekly review and owner decision are enforced here
//! and in the Core endpoints, never by model output.

use super::*;
use chrono::{Duration, Utc};
use sqlx::row::Row;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticKnowledgePackRecord {
    pub id: String,
    pub request_id: String,
    pub project_id: String,
    pub library: String,
    pub library_id: String,
    pub query: String,
    pub pack_json: String,
    pub pack_sha256: String,
    pub provider: String,
    pub source_url: String,
    pub fetched_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewAgenticKnowledgePack<'a> {
    pub request_id: &'a str,
    pub project_id: &'a str,
    pub pack: &'a crate::knowledge::KnowledgePack,
    pub provider: &'a str,
    pub source_url: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticSkillRecord {
    pub id: String,
    pub project_id: String,
    pub request_id: Option<String>,
    pub name: String,
    pub scope: String,
    pub status: String,
    pub manifest_json: String,
    pub manifest_sha256: String,
    pub provenance: String,
    pub license: String,
    pub license_reviewed_at: Option<String>,
    pub compatibility_json: String,
    pub success_count: u32,
    pub failure_count: u32,
    pub last_used_at: Option<String>,
    pub promoted_at: Option<String>,
    pub last_reviewed_at: Option<String>,
    pub next_review_at: Option<String>,
    pub revoked_at: Option<String>,
    pub owner_approved_by: Option<String>,
    pub expires_at: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NewAgenticSkill<'a> {
    pub project_id: &'a str,
    pub request_id: &'a str,
    pub manifest: &'a crate::knowledge::SkillManifest,
    pub manifest_sha256: &'a str,
    pub status: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticSkillUseRecord {
    pub id: String,
    pub skill_id: String,
    pub request_id: String,
    pub outcome: String,
    pub evidence: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgenticSkillAdoptionRecord {
    pub request_id: String,
    pub skill_id: String,
    pub role: String,
    pub adopted_at: String,
}

fn pack_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<AgenticKnowledgePackRecord, Error> {
    Ok(AgenticKnowledgePackRecord {
        id: row.try_get("id")?,
        request_id: row.try_get("request_id")?,
        project_id: row.try_get("project_id")?,
        library: row.try_get("library")?,
        library_id: row.try_get("library_id")?,
        query: row.try_get("query")?,
        pack_json: row.try_get("pack_json")?,
        pack_sha256: row.try_get("pack_sha256")?,
        provider: row.try_get("provider")?,
        source_url: row.try_get("source_url")?,
        fetched_at: row.try_get("fetched_at")?,
        created_at: row.try_get("created_at")?,
    })
}

fn skill_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<AgenticSkillRecord, Error> {
    Ok(AgenticSkillRecord {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        request_id: row.try_get("request_id")?,
        name: row.try_get("name")?,
        scope: row.try_get("scope")?,
        status: row.try_get("status")?,
        manifest_json: row.try_get("manifest_json")?,
        manifest_sha256: row.try_get("manifest_sha256")?,
        provenance: row.try_get("provenance")?,
        license: row.try_get("license")?,
        license_reviewed_at: row.try_get("license_reviewed_at")?,
        compatibility_json: row.try_get("compatibility_json")?,
        success_count: u32::try_from(row.try_get::<i64, _>("success_count")?)
            .map_err(|_| Error::Invalid("skill success count"))?,
        failure_count: u32::try_from(row.try_get::<i64, _>("failure_count")?)
            .map_err(|_| Error::Invalid("skill failure count"))?,
        last_used_at: row.try_get("last_used_at")?,
        promoted_at: row.try_get("promoted_at")?,
        last_reviewed_at: row.try_get("last_reviewed_at")?,
        next_review_at: row.try_get("next_review_at")?,
        revoked_at: row.try_get("revoked_at")?,
        owner_approved_by: row.try_get("owner_approved_by")?,
        expires_at: row.try_get("expires_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn skill_use_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<AgenticSkillUseRecord, Error> {
    Ok(AgenticSkillUseRecord {
        id: row.try_get("id")?,
        skill_id: row.try_get("skill_id")?,
        request_id: row.try_get("request_id")?,
        outcome: row.try_get("outcome")?,
        evidence: row.try_get("evidence")?,
        created_at: row.try_get("created_at")?,
    })
}

fn adoption_from_row(row: &sqlx_sqlite::SqliteRow) -> Result<AgenticSkillAdoptionRecord, Error> {
    Ok(AgenticSkillAdoptionRecord {
        request_id: row.try_get("request_id")?,
        skill_id: row.try_get("skill_id")?,
        role: row.try_get("role")?,
        adopted_at: row.try_get("adopted_at")?,
    })
}

fn review_due(next_review_at: Option<&str>, now: &str) -> bool {
    next_review_at.is_some_and(|value| value <= now)
}

impl Database {
    pub async fn save_agentic_knowledge_pack(
        &self,
        input: &NewAgenticKnowledgePack<'_>,
    ) -> Result<AgenticKnowledgePackRecord, Error> {
        let pack_json = serde_json::to_string(input.pack)
            .map_err(|_| Error::Invalid("knowledge pack encoding"))?;
        if input.pack.pack_sha256.len() != 64
            || input.provider.trim().is_empty()
            || !input.source_url.starts_with("https://")
        {
            return Err(Error::Invalid("knowledge pack identity"));
        }
        let _write = self.write_lock.lock().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO agentic_knowledge_packs
             (id,request_id,project_id,library,library_id,query,pack_json,pack_sha256,
              provider,source_url,fetched_at,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(request_id,library_id) DO UPDATE SET
               library=excluded.library,
               query=excluded.query,
               pack_json=excluded.pack_json,
               pack_sha256=excluded.pack_sha256,
               provider=excluded.provider,
               source_url=excluded.source_url,
               fetched_at=excluded.fetched_at,
               created_at=excluded.created_at",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(input.request_id)
        .bind(input.project_id)
        .bind(&input.pack.library)
        .bind(&input.pack.library_id)
        .bind(&input.pack.query)
        .bind(&pack_json)
        .bind(&input.pack.pack_sha256)
        .bind(input.provider)
        .bind(input.source_url)
        .bind(&input.pack.fetched_at)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.agentic_knowledge_pack_for_library(input.request_id, &input.pack.library_id)
            .await
    }

    pub async fn agentic_knowledge_pack(
        &self,
        request_id: &str,
    ) -> Result<AgenticKnowledgePackRecord, Error> {
        let row = sqlx::query::query(
            "SELECT id,request_id,project_id,library,library_id,query,pack_json,pack_sha256,
                    provider,source_url,fetched_at,created_at
             FROM agentic_knowledge_packs WHERE request_id=?1
             ORDER BY created_at DESC,id DESC LIMIT 1",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        pack_from_row(&row)
    }

    pub async fn agentic_knowledge_pack_for_library(
        &self,
        request_id: &str,
        library_id: &str,
    ) -> Result<AgenticKnowledgePackRecord, Error> {
        let row = sqlx::query::query(
            "SELECT id,request_id,project_id,library,library_id,query,pack_json,pack_sha256,
                    provider,source_url,fetched_at,created_at
             FROM agentic_knowledge_packs WHERE request_id=?1 AND library_id=?2",
        )
        .bind(request_id)
        .bind(library_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        pack_from_row(&row)
    }

    pub async fn create_agentic_skill(
        &self,
        input: &NewAgenticSkill<'_>,
    ) -> Result<AgenticSkillRecord, Error> {
        if !matches!(input.status, "CANDIDATE" | "ACTIVE") {
            return Err(Error::Invalid("new skill status"));
        }
        let manifest_json = serde_json::to_string(input.manifest)
            .map_err(|_| Error::Invalid("skill manifest encoding"))?;
        let compatibility_json = serde_json::to_string(&input.manifest.compatibility)
            .map_err(|_| Error::Invalid("skill compatibility encoding"))?;
        if input.manifest_sha256.len() != 64 {
            return Err(Error::Invalid("skill manifest digest"));
        }
        let _write = self.write_lock.lock().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query::query(
            "INSERT INTO agentic_skills
             (id,project_id,request_id,name,scope,status,manifest_json,manifest_sha256,
              provenance,license,license_reviewed_at,compatibility_json,success_count,
              failure_count,last_used_at,promoted_at,last_reviewed_at,next_review_at,
              revoked_at,owner_approved_by,expires_at,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,NULL,?11,0,0,NULL,NULL,NULL,NULL,
                     NULL,NULL,?12,?13,?13)
             ON CONFLICT(request_id,name) WHERE request_id IS NOT NULL DO NOTHING",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(input.project_id)
        .bind(input.request_id)
        .bind(&input.manifest.name)
        .bind(input.manifest.scope.as_str())
        .bind(input.status)
        .bind(&manifest_json)
        .bind(input.manifest_sha256)
        .bind(&input.manifest.provenance)
        .bind(&input.manifest.license)
        .bind(&compatibility_json)
        .bind(&input.manifest.expires_at)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.agentic_skill_for_request_name(input.request_id, &input.manifest.name)
            .await
    }

    async fn agentic_skill_for_request_name(
        &self,
        request_id: &str,
        name: &str,
    ) -> Result<AgenticSkillRecord, Error> {
        let row = sqlx::query::query(
            "SELECT id,project_id,request_id,name,scope,status,manifest_json,manifest_sha256,
                    provenance,license,license_reviewed_at,compatibility_json,success_count,
                    failure_count,last_used_at,promoted_at,last_reviewed_at,next_review_at,
                    revoked_at,owner_approved_by,expires_at,created_at,updated_at
             FROM agentic_skills WHERE request_id=?1 AND name=?2",
        )
        .bind(request_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        skill_from_row(&row)
    }

    pub async fn agentic_skill(&self, skill_id: &str) -> Result<AgenticSkillRecord, Error> {
        let row = sqlx::query::query(
            "SELECT id,project_id,request_id,name,scope,status,manifest_json,manifest_sha256,
                    provenance,license,license_reviewed_at,compatibility_json,success_count,
                    failure_count,last_used_at,promoted_at,last_reviewed_at,next_review_at,
                    revoked_at,owner_approved_by,expires_at,created_at,updated_at
             FROM agentic_skills WHERE id=?1",
        )
        .bind(skill_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        skill_from_row(&row)
    }

    pub async fn agentic_skills(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<AgenticSkillRecord>, Error> {
        let _ = self.expire_agentic_skills().await;
        let rows = if let Some(project_id) = project_id {
            sqlx::query::query(
                "SELECT id,project_id,request_id,name,scope,status,manifest_json,manifest_sha256,
                        provenance,license,license_reviewed_at,compatibility_json,success_count,
                        failure_count,last_used_at,promoted_at,last_reviewed_at,next_review_at,
                        revoked_at,owner_approved_by,expires_at,created_at,updated_at
                 FROM agentic_skills WHERE project_id=?1
                 ORDER BY created_at DESC,id DESC",
            )
            .bind(project_id)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query::query(
                "SELECT id,project_id,request_id,name,scope,status,manifest_json,manifest_sha256,
                        provenance,license,license_reviewed_at,compatibility_json,success_count,
                        failure_count,last_used_at,promoted_at,last_reviewed_at,next_review_at,
                        revoked_at,owner_approved_by,expires_at,created_at,updated_at
                 FROM agentic_skills ORDER BY created_at DESC,id DESC",
            )
            .fetch_all(&self.pool)
            .await?
        };
        rows.iter().map(skill_from_row).collect()
    }

    pub async fn expire_agentic_skills(&self) -> Result<(), Error> {
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "UPDATE agentic_skills SET status='EXPIRED',updated_at=?1
             WHERE scope='RUN' AND status IN ('ACTIVE','PROMOTION_READY')
               AND expires_at <= ?1",
        )
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn adopt_agentic_skill(
        &self,
        request_id: &str,
        skill_id: &str,
        role: &str,
    ) -> Result<AgenticSkillAdoptionRecord, Error> {
        if !matches!(role, "implementer" | "reviewer" | "verifier") {
            return Err(Error::Invalid("skill adoption role"));
        }
        let skill = self.agentic_skill(skill_id).await?;
        let now = Utc::now().to_rfc3339();
        if !matches!(skill.status.as_str(), "ACTIVE" | "PROMOTION_READY") {
            return Err(Error::Conflict("skill is not active"));
        }
        if skill.revoked_at.is_some() {
            return Err(Error::Conflict("skill is revoked"));
        }
        if skill.scope == "RUN" && skill.expires_at <= now {
            return Err(Error::Conflict("temporary skill has expired"));
        }
        if review_due(skill.next_review_at.as_deref(), &now) {
            return Err(Error::Conflict("skill weekly review is overdue"));
        }
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "INSERT INTO agentic_skill_adoptions (request_id,skill_id,role,adopted_at)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(request_id,skill_id,role) DO UPDATE SET adopted_at=excluded.adopted_at",
        )
        .bind(request_id)
        .bind(skill_id)
        .bind(role)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        let row = sqlx::query::query(
            "SELECT request_id,skill_id,role,adopted_at FROM agentic_skill_adoptions
             WHERE request_id=?1 AND skill_id=?2 AND role=?3",
        )
        .bind(request_id)
        .bind(skill_id)
        .bind(role)
        .fetch_one(&self.pool)
        .await?;
        adoption_from_row(&row)
    }

    pub async fn agentic_skill_adoptions(
        &self,
        request_id: &str,
    ) -> Result<Vec<AgenticSkillAdoptionRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT request_id,skill_id,role,adopted_at FROM agentic_skill_adoptions
             WHERE request_id=?1 ORDER BY adopted_at ASC",
        )
        .bind(request_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(adoption_from_row).collect()
    }

    /// Skills adopted by a request that is attached to this Campaign. Core
    /// includes only active, unexpired and reviewed skills in the assignment.
    pub async fn adopted_skills_for_campaign(
        &self,
        campaign_id: &str,
    ) -> Result<Vec<AssignmentSkill>, Error> {
        let now = Utc::now().to_rfc3339();
        let rows = sqlx::query::query(
            "SELECT s.id,s.name,s.manifest_json,s.scope,s.status,s.next_review_at,s.expires_at
             FROM agentic_skill_adoptions a
             JOIN agentic_skills s ON s.id=a.skill_id
             JOIN intake_requests ir ON ir.id=a.request_id
             WHERE ir.campaign_id=?1 AND a.role='implementer'
               AND s.status IN ('ACTIVE','PROMOTION_READY')
             ORDER BY s.created_at ASC,s.id ASC",
        )
        .bind(campaign_id)
        .fetch_all(&self.pool)
        .await?;
        let mut skills = Vec::new();
        for row in rows {
            let scope: String = row.try_get("scope")?;
            let expires_at: String = row.try_get("expires_at")?;
            let next_review_at: Option<String> = row.try_get("next_review_at")?;
            if scope == "RUN" && expires_at <= now {
                continue;
            }
            if review_due(next_review_at.as_deref(), &now) {
                continue;
            }
            let manifest_json: String = row.try_get("manifest_json")?;
            let manifest = serde_json::from_str::<crate::knowledge::SkillManifest>(&manifest_json)
                .map_err(|_| Error::Invalid("stored skill manifest"))?;
            skills.push(AssignmentSkill {
                id: row.try_get("id")?,
                name: row.try_get("name")?,
                instructions: manifest.instructions,
                tools: manifest.tools,
            });
        }
        Ok(skills)
    }

    pub async fn record_agentic_skill_use(
        &self,
        skill_id: &str,
        request_id: &str,
        outcome: &str,
        evidence: &str,
    ) -> Result<AgenticSkillRecord, Error> {
        if !matches!(outcome, "SUCCESS" | "FAILURE") {
            return Err(Error::Invalid("skill use outcome"));
        }
        if evidence.trim().is_empty() || evidence.len() > 2_000 {
            return Err(Error::Invalid("skill use evidence"));
        }
        let _write = self.write_lock.lock().await;
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query::query(
            "SELECT id,project_id,request_id,name,scope,status,manifest_json,manifest_sha256,
                    provenance,license,license_reviewed_at,compatibility_json,success_count,
                    failure_count,last_used_at,promoted_at,last_reviewed_at,next_review_at,
                    revoked_at,owner_approved_by,expires_at,created_at,updated_at
             FROM agentic_skills WHERE id=?1",
        )
        .bind(skill_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(Error::NotFound)?;
        let skill = skill_from_row(&row)?;
        let now = Utc::now().to_rfc3339();
        if !matches!(skill.status.as_str(), "ACTIVE" | "PROMOTION_READY") {
            return Err(Error::Conflict("skill is not active"));
        }
        if skill.scope == "RUN" && skill.expires_at <= now {
            return Err(Error::Conflict("temporary skill has expired"));
        }
        if review_due(skill.next_review_at.as_deref(), &now) {
            return Err(Error::Conflict("skill weekly review is overdue"));
        }
        sqlx::query::query(
            "INSERT INTO agentic_skill_uses (id,skill_id,request_id,outcome,evidence,created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(skill_id)
        .bind(request_id)
        .bind(outcome)
        .bind(evidence)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        let success_count = skill.success_count + if outcome == "SUCCESS" { 1 } else { 0 };
        let failure_count = skill.failure_count + if outcome == "FAILURE" { 1 } else { 0 };
        let mut scope = skill.scope.clone();
        let mut status = skill.status.clone();
        let mut promoted_at = skill.promoted_at.clone();
        let mut next_review_at = skill.next_review_at.clone();
        if success_count >= 2 && scope == "RUN" {
            if skill.license_reviewed_at.is_some() {
                scope = "PROJECT".into();
                status = "ACTIVE".into();
                promoted_at = Some(now.clone());
                next_review_at = Some((Utc::now() + Duration::days(7)).to_rfc3339());
            } else {
                status = "PROMOTION_READY".into();
            }
        }
        sqlx::query::query(
            "UPDATE agentic_skills
             SET success_count=?1,failure_count=?2,last_used_at=?3,scope=?4,status=?5,
                 promoted_at=?6,next_review_at=?7,updated_at=?3
             WHERE id=?8",
        )
        .bind(i64::from(success_count))
        .bind(i64::from(failure_count))
        .bind(&now)
        .bind(&scope)
        .bind(&status)
        .bind(&promoted_at)
        .bind(&next_review_at)
        .bind(skill_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        self.agentic_skill(skill_id).await
    }

    pub async fn agentic_skill_uses(
        &self,
        skill_id: &str,
    ) -> Result<Vec<AgenticSkillUseRecord>, Error> {
        let rows = sqlx::query::query(
            "SELECT id,skill_id,request_id,outcome,evidence,created_at
             FROM agentic_skill_uses WHERE skill_id=?1 ORDER BY created_at DESC,id DESC",
        )
        .bind(skill_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(skill_use_from_row).collect()
    }

    pub async fn review_agentic_skill_license(
        &self,
        skill_id: &str,
        reviewer: &str,
        license: &str,
    ) -> Result<AgenticSkillRecord, Error> {
        if reviewer.trim().is_empty() || license.trim().is_empty() || license.len() > 120 {
            return Err(Error::Invalid("skill license review"));
        }
        let _write = self.write_lock.lock().await;
        let skill = self.agentic_skill(skill_id).await?;
        if matches!(skill.status.as_str(), "REVOKED" | "EXPIRED") {
            return Err(Error::Conflict("skill is not active"));
        }
        let now = Utc::now().to_rfc3339();
        let mut scope = skill.scope.clone();
        let status = if skill.status == "PROMOTION_READY" {
            "ACTIVE".to_string()
        } else {
            skill.status.clone()
        };
        let mut promoted_at = skill.promoted_at.clone();
        let mut next_review_at = skill.next_review_at.clone();
        if skill.success_count >= 2 && scope == "RUN" {
            scope = "PROJECT".into();
            promoted_at = Some(now.clone());
            next_review_at = Some((Utc::now() + Duration::days(7)).to_rfc3339());
        }
        sqlx::query::query(
            "UPDATE agentic_skills
             SET license=?1,license_reviewed_at=?2,scope=?3,status=?4,promoted_at=?5,
                 next_review_at=?6,updated_at=?2
             WHERE id=?7",
        )
        .bind(license)
        .bind(&now)
        .bind(&scope)
        .bind(&status)
        .bind(&promoted_at)
        .bind(&next_review_at)
        .bind(skill_id)
        .execute(&self.pool)
        .await?;
        self.agentic_skill(skill_id).await
    }

    pub async fn promote_agentic_skill(
        &self,
        skill_id: &str,
        target_scope: &str,
        owner_approved_by: &str,
        confirm: bool,
    ) -> Result<AgenticSkillRecord, Error> {
        if !confirm || !matches!(target_scope, "PROJECT" | "GLOBAL") {
            return Err(Error::Invalid("skill promotion confirmation"));
        }
        let skill = self.agentic_skill(skill_id).await?;
        let now = Utc::now().to_rfc3339();
        let review_due = review_due(skill.next_review_at.as_deref(), &now);
        let license_reviewed = skill.license_reviewed_at.is_some();
        match target_scope {
            "PROJECT" => {
                if skill.scope != "RUN" {
                    return Err(Error::Conflict("only a RUN skill promotes to PROJECT"));
                }
                crate::knowledge::project_promotion_ready(
                    skill.success_count,
                    license_reviewed,
                    review_due,
                )
                .map_err(Error::Validation)?;
            }
            "GLOBAL" => {
                if skill.scope != "PROJECT" {
                    return Err(Error::Conflict("only a PROJECT skill promotes to GLOBAL"));
                }
                if owner_approved_by.trim().is_empty() {
                    return Err(Error::Invalid("global skill owner decision"));
                }
                crate::knowledge::global_promotion_ready(
                    skill.success_count,
                    license_reviewed,
                    review_due,
                    true,
                )
                .map_err(Error::Validation)?;
            }
            _ => return Err(Error::Invalid("skill promotion scope")),
        }
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "UPDATE agentic_skills
             SET scope=?1,status='ACTIVE',promoted_at=?2,last_reviewed_at=?2,
                 next_review_at=?3,owner_approved_by=?4,updated_at=?2
             WHERE id=?5",
        )
        .bind(target_scope)
        .bind(&now)
        .bind((Utc::now() + Duration::days(7)).to_rfc3339())
        .bind(if target_scope == "GLOBAL" {
            Some(owner_approved_by)
        } else {
            None
        })
        .bind(skill_id)
        .execute(&self.pool)
        .await?;
        self.agentic_skill(skill_id).await
    }

    pub async fn review_agentic_skill(
        &self,
        skill_id: &str,
        reviewer: &str,
    ) -> Result<AgenticSkillRecord, Error> {
        if reviewer.trim().is_empty() {
            return Err(Error::Invalid("skill reviewer"));
        }
        let skill = self.agentic_skill(skill_id).await?;
        if matches!(skill.status.as_str(), "REVOKED" | "EXPIRED") {
            return Err(Error::Conflict("skill is not reviewable"));
        }
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "UPDATE agentic_skills
             SET last_reviewed_at=?1,next_review_at=?2,status=CASE
               WHEN status='PROMOTION_READY' THEN 'PROMOTION_READY' ELSE 'ACTIVE' END,
               updated_at=?1
             WHERE id=?3",
        )
        .bind(&now)
        .bind((Utc::now() + Duration::days(7)).to_rfc3339())
        .bind(skill_id)
        .execute(&self.pool)
        .await?;
        self.agentic_skill(skill_id).await
    }

    pub async fn revoke_agentic_skill(
        &self,
        skill_id: &str,
        reason: &str,
    ) -> Result<AgenticSkillRecord, Error> {
        if reason.trim().is_empty() || reason.len() > 500 {
            return Err(Error::Invalid("skill revocation reason"));
        }
        let now = Utc::now().to_rfc3339();
        let _write = self.write_lock.lock().await;
        sqlx::query::query(
            "UPDATE agentic_skills SET status='REVOKED',revoked_at=?1,updated_at=?1 WHERE id=?2",
        )
        .bind(&now)
        .bind(skill_id)
        .execute(&self.pool)
        .await?;
        self.agentic_skill(skill_id).await
    }
}
