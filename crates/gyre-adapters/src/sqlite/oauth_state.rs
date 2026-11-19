//! SQLite adapter for the `OAuthStateRepository` port (TASK-273,
//! admin-panel.md: durable OAuth state for Claude Max enrollment).

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_ports::{OAuthStateRecord, OAuthStateRepository};
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::oauth_states;

#[derive(Queryable, Selectable)]
#[diesel(table_name = oauth_states)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct OAuthStateRow {
    state: String,
    tenant_id: String,
    workspace_id: Option<String>,
    initiated_by: String,
    pkce_verifier: String,
    created_at: i64,
    expires_at: i64,
    consumed: i32,
    provider_config: String,
}

impl OAuthStateRow {
    fn into_record(self) -> OAuthStateRecord {
        OAuthStateRecord {
            state: self.state,
            tenant_id: self.tenant_id,
            workspace_id: self.workspace_id,
            initiated_by: self.initiated_by,
            pkce_verifier: self.pkce_verifier,
            created_at: self.created_at as u64,
            expires_at: self.expires_at as u64,
            consumed: self.consumed != 0,
            provider_config: self.provider_config,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = oauth_states)]
struct NewOAuthStateRow<'a> {
    state: &'a str,
    tenant_id: &'a str,
    workspace_id: Option<&'a str>,
    initiated_by: &'a str,
    pkce_verifier: &'a str,
    created_at: i64,
    expires_at: i64,
    consumed: i32,
    provider_config: &'a str,
}

#[async_trait]
impl OAuthStateRepository for SqliteStorage {
    async fn create(&self, record: &OAuthStateRecord) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let record = record.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewOAuthStateRow {
                state: &record.state,
                tenant_id: &record.tenant_id,
                workspace_id: record.workspace_id.as_deref(),
                initiated_by: &record.initiated_by,
                pkce_verifier: &record.pkce_verifier,
                created_at: record.created_at as i64,
                expires_at: record.expires_at as i64,
                consumed: record.consumed as i32,
                provider_config: &record.provider_config,
            };
            diesel::insert_into(oauth_states::table)
                .values(&row)
                .execute(&mut conn)
                .map_err(|e| {
                    anyhow::anyhow!("oauth state create failed (duplicate state?): {e}")
                })?;
            Ok(())
        })
        .await?
    }

    async fn find_by_state(&self, state: &str) -> Result<Option<OAuthStateRecord>> {
        let pool = Arc::clone(&self.pool);
        let state = state.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<OAuthStateRecord>> {
            let mut conn = pool.get().context("get db connection")?;
            Ok(oauth_states::table
                .filter(oauth_states::state.eq(state))
                .select(OAuthStateRow::as_select())
                .first(&mut conn)
                .optional()?
                .map(OAuthStateRow::into_record))
        })
        .await?
    }

    async fn mark_consumed(&self, state: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let state = state.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let n =
                diesel::update(oauth_states::table.filter(oauth_states::state.eq(state.clone())))
                    .set(oauth_states::consumed.eq(1))
                    .execute(&mut conn)
                    .context("mark oauth state consumed")?;
            anyhow::ensure!(n > 0, "oauth state '{state}' not found");
            Ok(())
        })
        .await?
    }

    async fn delete_expired_before(&self, cutoff_secs: u64) -> Result<u64> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = pool.get().context("get db connection")?;
            let n = diesel::delete(oauth_states::table)
                .filter(oauth_states::expires_at.lt(cutoff_secs as i64))
                .execute(&mut conn)
                .context("delete expired oauth states")?;
            Ok(n as u64)
        })
        .await?
    }
}
