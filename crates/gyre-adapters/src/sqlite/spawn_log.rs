use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_ports::{SpawnLogEntry, SpawnLogRepository};
use std::sync::Arc;
use uuid::Uuid;

use super::SqliteStorage;
use crate::schema::{revoked_tokens, spawn_log};

#[derive(Queryable, Selectable)]
#[diesel(table_name = spawn_log)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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
impl SpawnLogRepository for SqliteStorage {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    #[tokio::test]
    async fn append_and_get_spawn_log() {
        let (_tmp, s) = setup();
        SpawnLogRepository::append_spawn_step(&s, "agent-1", "create_agent", "ok", None, 1000)
            .await
            .unwrap();
        SpawnLogRepository::append_spawn_step(
            &s,
            "agent-1",
            "mint_token",
            "ok",
            Some("token issued"),
            1001,
        )
        .await
        .unwrap();
        let log = SpawnLogRepository::get_spawn_log(&s, "agent-1")
            .await
            .unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].step, "create_agent");
        assert_eq!(log[1].step, "mint_token");
        assert_eq!(log[1].detail.as_deref(), Some("token issued"));
    }

    #[tokio::test]
    async fn spawn_log_isolated_by_agent() {
        let (_tmp, s) = setup();
        SpawnLogRepository::append_spawn_step(&s, "agent-1", "step1", "ok", None, 1000)
            .await
            .unwrap();
        SpawnLogRepository::append_spawn_step(&s, "agent-2", "step1", "ok", None, 1000)
            .await
            .unwrap();
        let log1 = SpawnLogRepository::get_spawn_log(&s, "agent-1")
            .await
            .unwrap();
        let log2 = SpawnLogRepository::get_spawn_log(&s, "agent-2")
            .await
            .unwrap();
        assert_eq!(log1.len(), 1);
        assert_eq!(log2.len(), 1);
    }

    #[tokio::test]
    async fn revoke_and_check_token() {
        let (_tmp, s) = setup();
        let hash = "deadbeefdeadbeef";
        assert!(!SpawnLogRepository::is_token_revoked(&s, hash)
            .await
            .unwrap());
        SpawnLogRepository::revoke_token(&s, hash, "agent-1", 2000)
            .await
            .unwrap();
        assert!(SpawnLogRepository::is_token_revoked(&s, hash)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn revoke_idempotent() {
        let (_tmp, s) = setup();
        let hash = "aabbcc";
        SpawnLogRepository::revoke_token(&s, hash, "agent-1", 2000)
            .await
            .unwrap();
        // Second call should not error (ON CONFLICT DO NOTHING)
        SpawnLogRepository::revoke_token(&s, hash, "agent-1", 2001)
            .await
            .unwrap();
        assert!(SpawnLogRepository::is_token_revoked(&s, hash)
            .await
            .unwrap());
    }
}
