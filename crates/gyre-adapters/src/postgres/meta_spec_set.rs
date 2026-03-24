use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_ports::MetaSpecSetRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::meta_spec_sets;

#[derive(Queryable, Selectable)]
#[diesel(table_name = meta_spec_sets)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct MetaSpecSetRow {
    workspace_id: String,
    json: String,
    #[allow(dead_code)]
    updated_at: i64,
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = meta_spec_sets)]
struct UpsertMetaSpecSetRow<'a> {
    workspace_id: &'a str,
    json: &'a str,
    updated_at: i64,
}

#[async_trait]
impl MetaSpecSetRepository for PgStorage {
    async fn get(&self, workspace_id: &Id) -> Result<Option<String>> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<String>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = meta_spec_sets::table
                .find(ws_id.as_str())
                .first::<MetaSpecSetRow>(&mut *conn)
                .optional()
                .context("find meta_spec_set by workspace_id")?;
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
            let row = UpsertMetaSpecSetRow {
                workspace_id: ws_id.as_str(),
                json: json.as_str(),
                updated_at: now,
            };
            diesel::insert_into(meta_spec_sets::table)
                .values(&row)
                .on_conflict(meta_spec_sets::workspace_id)
                .do_update()
                .set(&row)
                .execute(&mut *conn)
                .context("upsert meta_spec_set")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, workspace_id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(meta_spec_sets::table.find(ws_id.as_str()))
                .execute(&mut *conn)
                .context("delete meta_spec_set")?;
            Ok(())
        })
        .await?
    }
}
