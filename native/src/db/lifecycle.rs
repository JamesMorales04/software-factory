use super::*;

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

impl Database {
    pub fn create_campaign(&self, action_id: &str, key: &str) -> Result<Campaign, Error> {
        let digest = sha256_hex(
            serde_json::to_string(&(action_id, "campaign-v1"))
                .unwrap()
                .as_bytes(),
        );
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction()?;
        if let Some(id) = replay(&tx, key, "create_campaign", &digest)? {
            let value = load_campaign(&tx, &id)?;
            tx.commit()?;
            return Ok(value);
        }
        let exists = tx
            .query_row("SELECT 1 FROM actions WHERE id=?1", [action_id], |_| Ok(()))
            .optional()?
            .is_some();
        if !exists {
            return Err(Error::NotFound);
        }
        let value = Campaign {
            id: Uuid::new_v4().to_string(),
            action_id: action_id.into(),
            status: "BLOCKED_AUTHORIZATION".into(),
            created_at: Utc::now().to_rfc3339(),
        };
        tx.execute(
            "INSERT INTO campaigns VALUES (?1,?2,?3,?4)",
            params![value.id, value.action_id, value.status, value.created_at],
        )?;
        tx.execute(
            "INSERT INTO idempotency VALUES (?1,?2,?3,?4)",
            params![key, "create_campaign", digest, value.id],
        )?;
        record_event(
            &tx,
            &value.id,
            "campaign.created",
            "blocked pending explicit authorization",
        )?;
        tx.commit()?;
        Ok(value)
    }

    pub fn create_run(&self, campaign_id: &str, key: &str) -> Result<Run, Error> {
        let digest = sha256_hex(
            serde_json::to_string(&(campaign_id, "run-v1"))
                .unwrap()
                .as_bytes(),
        );
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction()?;
        if let Some(id) = replay(&tx, key, "create_run", &digest)? {
            let value = load_run(&tx, &id)?;
            tx.commit()?;
            return Ok(value);
        }
        let authorized: bool = tx
            .query_row(
                "SELECT status='AUTHORIZED' FROM campaigns WHERE id=?1",
                [campaign_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        if !authorized {
            return Err(Error::Invalid(
                "campaign lacks explicit execution authorization",
            ));
        }
        let value = Run {
            id: Uuid::new_v4().to_string(),
            campaign_id: campaign_id.into(),
            status: "QUEUED".into(),
            created_at: Utc::now().to_rfc3339(),
        };
        tx.execute(
            "INSERT INTO runs VALUES (?1,?2,?3,?4)",
            params![value.id, value.campaign_id, value.status, value.created_at],
        )?;
        tx.execute(
            "INSERT INTO idempotency VALUES (?1,?2,?3,?4)",
            params![key, "create_run", digest, value.id],
        )?;
        record_event(&tx, &value.id, "run.created", "queued")?;
        tx.commit()?;
        Ok(value)
    }

    pub fn create_attempt(&self, run_id: &str, key: &str) -> Result<Attempt, Error> {
        let digest = sha256_hex(
            serde_json::to_string(&(run_id, "attempt-v1"))
                .unwrap()
                .as_bytes(),
        );
        let mut connection = self.0.lock().expect("database mutex poisoned");
        let tx = connection.transaction()?;
        if let Some(id) = replay(&tx, key, "create_attempt", &digest)? {
            let value = load_attempt(&tx, &id)?;
            tx.commit()?;
            return Ok(value);
        }
        let exists = tx
            .query_row(
                "SELECT 1 FROM runs WHERE id=?1 AND status='QUEUED'",
                [run_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(Error::Invalid("run is not queued"));
        }
        let value = Attempt {
            id: Uuid::new_v4().to_string(),
            run_id: run_id.into(),
            status: "QUEUED".into(),
            created_at: Utc::now().to_rfc3339(),
        };
        tx.execute(
            "INSERT INTO attempts(id,run_id,status,created_at) VALUES (?1,?2,?3,?4)",
            params![value.id, value.run_id, value.status, value.created_at],
        )?;
        tx.execute(
            "INSERT INTO idempotency VALUES (?1,?2,?3,?4)",
            params![key, "create_attempt", digest, value.id],
        )?;
        record_event(
            &tx,
            &value.id,
            "attempt.created",
            "queued; no worker assignment",
        )?;
        tx.commit()?;
        Ok(value)
    }

    pub fn campaigns(&self) -> Result<Vec<Campaign>, Error> {
        let connection = self.0.lock().expect("database mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT id,action_id,status,created_at FROM campaigns ORDER BY created_at,id",
        )?;
        Ok(statement
            .query_map([], |row| {
                Ok(Campaign {
                    id: row.get(0)?,
                    action_id: row.get(1)?,
                    status: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn runs(&self) -> Result<Vec<Run>, Error> {
        let connection = self.0.lock().expect("database mutex poisoned");
        let mut statement = connection
            .prepare("SELECT id,campaign_id,status,created_at FROM runs ORDER BY created_at,id")?;
        Ok(statement
            .query_map([], |r| {
                Ok(Run {
                    id: r.get(0)?,
                    campaign_id: r.get(1)?,
                    status: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn attempts(&self) -> Result<Vec<Attempt>, Error> {
        let connection = self.0.lock().expect("database mutex poisoned");
        let mut statement = connection
            .prepare("SELECT id,run_id,status,created_at FROM attempts ORDER BY created_at,id")?;
        Ok(statement
            .query_map([], |r| {
                Ok(Attempt {
                    id: r.get(0)?,
                    run_id: r.get(1)?,
                    status: r.get(2)?,
                    created_at: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }
}

pub(super) fn load_campaign(connection: &Connection, id: &str) -> Result<Campaign, Error> {
    Ok(connection.query_row(
        "SELECT id,action_id,status,created_at FROM campaigns WHERE id=?1",
        [id],
        |r| {
            Ok(Campaign {
                id: r.get(0)?,
                action_id: r.get(1)?,
                status: r.get(2)?,
                created_at: r.get(3)?,
            })
        },
    )?)
}

pub(super) fn load_run(connection: &Connection, id: &str) -> Result<Run, Error> {
    Ok(connection.query_row(
        "SELECT id,campaign_id,status,created_at FROM runs WHERE id=?1",
        [id],
        |r| {
            Ok(Run {
                id: r.get(0)?,
                campaign_id: r.get(1)?,
                status: r.get(2)?,
                created_at: r.get(3)?,
            })
        },
    )?)
}

pub(super) fn load_attempt(connection: &Connection, id: &str) -> Result<Attempt, Error> {
    Ok(connection.query_row(
        "SELECT id,run_id,status,created_at FROM attempts WHERE id=?1",
        [id],
        |r| {
            Ok(Attempt {
                id: r.get(0)?,
                run_id: r.get(1)?,
                status: r.get(2)?,
                created_at: r.get(3)?,
            })
        },
    )?)
}
