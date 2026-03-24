use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_ports::PushGateRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::repo_push_gates;

#[derive(Queryable, Selectable)]
#[diesel(table_name = repo_push_gates)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct PushGateRow {
    #[allow(dead_code)]
    repo_id: String,
    gate_names: String,
}

#[derive(Insertable)]
#[diesel(table_name = repo_push_gates)]
struct NewPushGateRow<'a> {
    repo_id: &'a str,
    gate_names: &'a str,
}

#[async_trait]
impl PushGateRepository for SqliteStorage {
    async fn get_for_repo(&self, repo_id: &str) -> Result<Vec<String>> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<String>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = repo_push_gates::table
                .find(&repo_id)
                .first::<PushGateRow>(&mut *conn)
                .optional()
                .context("find push gates for repo")?;
            match row {
                None => Ok(vec![]),
                Some(r) => {
                    let gates: Vec<String> =
                        serde_json::from_str(&r.gate_names).unwrap_or_default();
                    Ok(gates)
                }
            }
        })
        .await?
    }

    async fn set_for_repo(&self, repo_id: &str, gates: Vec<String>) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let repo_id = repo_id.to_string();
        let gate_names = serde_json::to_string(&gates).unwrap_or_else(|_| "[]".to_string());
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewPushGateRow {
                repo_id: &repo_id,
                gate_names: &gate_names,
            };
            diesel::insert_into(repo_push_gates::table)
                .values(&row)
                .on_conflict(repo_push_gates::repo_id)
                .do_update()
                .set(repo_push_gates::gate_names.eq(&gate_names))
                .execute(&mut *conn)
                .context("upsert push gates")?;
            Ok(())
        })
        .await?
    }
}
