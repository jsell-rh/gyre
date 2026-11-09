use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_ports::RolloutPolicyRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::rollout_policies;

#[derive(Queryable, Selectable)]
#[diesel(table_name = rollout_policies)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct RolloutPolicyRow {
    #[allow(dead_code)]
    workspace_id: String,
    json: String,
    #[allow(dead_code)]
    updated_at: i64,
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = rollout_policies)]
struct UpsertRolloutPolicyRow<'a> {
    workspace_id: &'a str,
    json: &'a str,
    updated_at: i64,
}

#[async_trait]
impl RolloutPolicyRepository for PgStorage {
    async fn get(&self, workspace_id: &Id) -> Result<Option<String>> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<String>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = rollout_policies::table
                .find(ws_id.as_str())
                .first::<RolloutPolicyRow>(&mut *conn)
                .optional()
                .context("find rollout_policy by workspace_id")?;
            Ok(result.map(|r| r.json))
        })
        .await?
    }

    async fn upsert(&self, workspace_id: &Id, json: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        let json = json.to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = UpsertRolloutPolicyRow {
                workspace_id: ws_id.as_str(),
                json: json.as_str(),
                updated_at: now,
            };
            diesel::insert_into(rollout_policies::table)
                .values(&row)
                .on_conflict(rollout_policies::workspace_id)
                .do_update()
                .set(&row)
                .execute(&mut *conn)
                .context("upsert rollout_policy")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, workspace_id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(rollout_policies::table.find(ws_id.as_str()))
                .execute(&mut *conn)
                .context("delete rollout_policy")?;
            Ok(())
        })
        .await?
    }
}
