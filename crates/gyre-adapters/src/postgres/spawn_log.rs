use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_ports::{SpawnLogEntry, SpawnLogRepository};
use std::sync::Arc;
use uuid::Uuid;

use super::PgStorage;
use crate::schema::{revoked_tokens, spawn_log};

#[derive(Queryable, Selectable)]
#[diesel(table_name = spawn_log)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct SpawnLogRow {
    #[allow(dead_code)]
    id: String,
    agent_id: String,
    step: String,
    status: String,
    detail: Option<String>,
    occurred_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = spawn_log)]
struct NewSpawnLogRow<'a> {
    id: &'a str,
    agent_id: &'a str,
    step: &'a str,
    status: &'a str,
    detail: Option<&'a str>,
    occurred_at: i64,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = revoked_tokens)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct RevokedTokenRow {
    #[allow(dead_code)]
    token_hash: String,
    #[allow(dead_code)]
    agent_id: String,
    #[allow(dead_code)]
    revoked_at: i64,
}

#[derive(Insertable)]
#[diesel(table_name = revoked_tokens)]
struct NewRevokedTokenRow<'a> {
    token_hash: &'a str,
    agent_id: &'a str,
    revoked_at: i64,
}

#[async_trait]
impl SpawnLogRepository for PgStorage {
    async fn append_spawn_step(
        &self,
        agent_id: &str,
        step: &str,
        status: &str,
        detail: Option<&str>,
        occurred_at: u64,
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = Uuid::new_v4().to_string();
        let agent_id = agent_id.to_string();
        let step = step.to_string();
        let status = status.to_string();
        let detail = detail.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewSpawnLogRow {
                id: &id,
                agent_id: &agent_id,
                step: &step,
                status: &status,
                detail: detail.as_deref(),
                occurred_at: occurred_at as i64,
            };
            diesel::insert_into(spawn_log::table)
                .values(&row)
                .execute(&mut *conn)
                .context("insert spawn_log entry")?;
            Ok(())
        })
        .await?
    }

    async fn get_spawn_log(&self, agent_id: &str) -> Result<Vec<SpawnLogEntry>> {
        let pool = Arc::clone(&self.pool);
        let agent_id = agent_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<SpawnLogEntry>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = spawn_log::table
                .filter(spawn_log::agent_id.eq(&agent_id))
                .order(spawn_log::occurred_at.asc())
                .load::<SpawnLogRow>(&mut *conn)
                .context("load spawn_log")?;
            Ok(rows
                .into_iter()
                .map(|r| SpawnLogEntry {
                    agent_id: r.agent_id,
                    step: r.step,
                    status: r.status,
                    detail: r.detail,
                    occurred_at: r.occurred_at as u64,
                })
                .collect())
        })
        .await?
    }

    async fn revoke_token(&self, token_hash: &str, agent_id: &str, revoked_at: u64) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let token_hash = token_hash.to_string();
        let agent_id = agent_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewRevokedTokenRow {
                token_hash: &token_hash,
                agent_id: &agent_id,
                revoked_at: revoked_at as i64,
            };
            diesel::insert_into(revoked_tokens::table)
                .values(&row)
                .on_conflict(revoked_tokens::token_hash)
                .do_nothing()
                .execute(&mut *conn)
                .context("insert revoked_token")?;
            Ok(())
        })
        .await?
    }

    async fn is_token_revoked(&self, token_hash: &str) -> Result<bool> {
        let pool = Arc::clone(&self.pool);
        let token_hash = token_hash.to_string();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            let mut conn = pool.get().context("get db connection")?;
            let found = revoked_tokens::table
                .find(&token_hash)
                .first::<RevokedTokenRow>(&mut *conn)
                .optional()
                .context("check revoked_token")?;
            Ok(found.is_some())
        })
        .await?
    }
}
