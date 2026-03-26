use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_ports::UserWorkspaceStateRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::user_workspace_state;

#[derive(Queryable, Selectable)]
#[allow(dead_code)]
#[diesel(table_name = user_workspace_state)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct UserWorkspaceStateRow {
    user_id: String,
    workspace_id: String,
    last_seen_at: i64,
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = user_workspace_state)]
struct UpsertRow<'a> {
    user_id: &'a str,
    workspace_id: &'a str,
    last_seen_at: i64,
}

#[async_trait]
impl UserWorkspaceStateRepository for SqliteStorage {
    async fn upsert_last_seen(
        &self,
        user_id: &str,
        workspace_id: &str,
        timestamp: i64,
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.to_owned();
        let wid = workspace_id.to_owned();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = UpsertRow {
                user_id: &uid,
                workspace_id: &wid,
                last_seen_at: timestamp,
            };
            diesel::insert_into(user_workspace_state::table)
                .values(&row)
                .on_conflict((
                    user_workspace_state::user_id,
                    user_workspace_state::workspace_id,
                ))
                .do_update()
                .set(user_workspace_state::last_seen_at.eq(timestamp))
                .execute(&mut *conn)
                .context("upsert user_workspace_state")?;
            Ok(())
        })
        .await?
    }

    async fn get_last_seen(&self, user_id: &str, workspace_id: &str) -> Result<Option<i64>> {
        let pool = Arc::clone(&self.pool);
        let uid = user_id.to_owned();
        let wid = workspace_id.to_owned();
        tokio::task::spawn_blocking(move || -> Result<Option<i64>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = user_workspace_state::table
                .filter(user_workspace_state::user_id.eq(&uid))
                .filter(user_workspace_state::workspace_id.eq(&wid))
                .first::<UserWorkspaceStateRow>(&mut *conn)
                .optional()
                .context("get user_workspace_state")?;
            Ok(result.map(|r| r.last_seen_at))
        })
        .await?
    }
}
