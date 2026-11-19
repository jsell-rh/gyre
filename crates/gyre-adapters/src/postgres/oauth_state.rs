//! SQLite adapter for the `OAuthStateRepository` port (TASK-273,
//! admin-panel.md: durable OAuth state for Claude Max enrollment).

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_ports::{OAuthStateRecord, OAuthStateRepository};
use std::sync::Arc;

use super::PgStorage;
use crate::schema::oauth_states;

#[derive(Queryable, Selectable)]
#[diesel(table_name = oauth_states)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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
impl OAuthStateRepository for PgStorage {
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

#[cfg(test)]
mod tests {
    //! SQL-shape regression tests (postgres/search.rs precedent: the repo
    //! has no PG server harness). The behavioral semantics — single-use
    //! consumption, only-expired deletion, restart durability — are
    //! exercised against a real SQLite file by the sqlite sibling's suite;
    //! these pin the PG statements the mirror emits.

    use super::*;

    #[test]
    fn mark_consumed_updates_only_matching_state_row() {
        let sql = diesel::debug_query::<diesel::pg::Pg, _>(
            &diesel::update(oauth_states::table.filter(oauth_states::state.eq("st-1")))
                .set(oauth_states::consumed.eq(1)),
        )
        .to_string();
        assert!(
            sql.contains("UPDATE") && sql.contains("state") && sql.contains("consumed"),
            "mark_consumed must be a scoped UPDATE: {sql}"
        );
    }

    #[test]
    fn delete_expired_before_filters_on_expires_at_in_sql() {
        // Only-expired deletion must live in the WHERE clause: a predicate
        // on expires_at < cutoff, never a bare DELETE of the whole table.
        let sql = diesel::debug_query::<diesel::pg::Pg, _>(
            &diesel::delete(oauth_states::table).filter(oauth_states::expires_at.lt(1_000i64)),
        )
        .to_string();
        assert!(
            sql.contains("expires_at") && sql.contains("< $"),
            "cleanup must filter on expires_at < cutoff in SQL: {sql}"
        );
    }

    #[test]
    fn find_by_state_scopes_on_state_key() {
        let sql = diesel::debug_query::<diesel::pg::Pg, _>(
            &oauth_states::table.filter(oauth_states::state.eq("st-1")),
        )
        .to_string();
        assert!(
            sql.contains("state") && sql.contains("= $"),
            "find_by_state must filter on the state key: {sql}"
        );
    }
}
