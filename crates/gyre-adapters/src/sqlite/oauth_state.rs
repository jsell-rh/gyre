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

    async fn mark_consumed(&self, state: &str) -> Result<bool> {
        let pool = Arc::clone(&self.pool);
        let state = state.to_string();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            let mut conn = pool.get().context("get db connection")?;
            // Atomic single-use consumption: the conditional
            // `consumed = 0` predicate makes the flip itself the arbiter —
            // exactly one of any number of concurrent callers gets
            // `n > 0`; the rest see `false` (replay).
            let n = diesel::update(
                oauth_states::table
                    .filter(oauth_states::state.eq(state.clone()))
                    .filter(oauth_states::consumed.eq(0)),
            )
            .set(oauth_states::consumed.eq(1))
            .execute(&mut conn)
            .context("mark oauth state consumed")?;
            Ok(n > 0)
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
    use super::*;
    use gyre_ports::OAuthStateRepository as _;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    fn row(state: &str, expires_at: u64, consumed: bool) -> OAuthStateRecord {
        OAuthStateRecord {
            state: state.to_string(),
            tenant_id: "tenant-1".to_string(),
            workspace_id: Some("ws-1".to_string()),
            initiated_by: "user-admin".to_string(),
            pkce_verifier: "verifier-abc".to_string(),
            created_at: expires_at - 600,
            expires_at,
            consumed,
            provider_config: "{}".to_string(),
        }
    }

    #[tokio::test]
    async fn create_find_roundtrip_and_duplicate_fails() {
        let (_tmp, s) = setup();
        s.create(&row("st-1", 10_000, false)).await.unwrap();

        let found = s
            .find_by_state("st-1")
            .await
            .unwrap()
            .expect("row must exist");
        assert_eq!(found.tenant_id, "tenant-1");
        assert_eq!(found.workspace_id.as_deref(), Some("ws-1"));
        assert_eq!(found.initiated_by, "user-admin");
        assert_eq!(found.pkce_verifier, "verifier-abc");
        assert_eq!(found.expires_at, 10_000);
        assert!(!found.consumed);

        // Port contract: duplicate state values are rejected.
        let dup = s.create(&row("st-1", 20_000, false)).await;
        assert!(dup.is_err(), "duplicate state create must fail");

        assert!(s.find_by_state("st-missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn mark_consumed_is_atomic_single_use() {
        let (_tmp, s) = setup();
        s.create(&row("st-1", 10_000, false)).await.unwrap();

        // Single-use consumption: the first caller wins...
        assert!(s.mark_consumed("st-1").await.unwrap());
        assert!(s.find_by_state("st-1").await.unwrap().unwrap().consumed);

        // ...and every later caller loses — the flip is a conditional
        // UPDATE (consumed = 0 predicate), so concurrent replays cannot
        // both win even without a prior read.
        assert!(
            !s.mark_consumed("st-1").await.unwrap(),
            "a consumed state must not be consumable again"
        );

        // Unknown state: no row flipped, not an error.
        assert!(!s.mark_consumed("st-missing").await.unwrap());
    }

    #[tokio::test]
    async fn delete_expired_before_removes_only_expired_and_survives_rebuild() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        {
            let s = SqliteStorage::new(&path).unwrap();
            s.create(&row("st-expired", 1_000, false)).await.unwrap();
            s.create(&row("st-valid", 90_000, false)).await.unwrap();
            // Expired AND consumed — still an expired row, still deleted.
            s.create(&row("st-consumed", 900, true)).await.unwrap();

            let deleted = s.delete_expired_before(10_000).await.unwrap();
            assert_eq!(deleted, 2, "only the expired rows are deleted");
            assert!(s.find_by_state("st-valid").await.unwrap().is_some());
        }
        // Restart durability: the valid row survives a storage rebuild.
        let s2 = SqliteStorage::new(&path).unwrap();
        assert!(s2.find_by_state("st-valid").await.unwrap().is_some());
        assert!(s2.find_by_state("st-expired").await.unwrap().is_none());
    }
}
