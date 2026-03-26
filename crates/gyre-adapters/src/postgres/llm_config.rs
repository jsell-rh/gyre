use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::LlmFunctionConfig;
use gyre_ports::LlmConfigRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::llm_function_configs;

#[derive(Queryable, Selectable)]
#[diesel(table_name = llm_function_configs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct LlmFunctionConfigRow {
    id: String,
    workspace_id: Option<String>,
    function_key: String,
    model_name: String,
    max_tokens: Option<i32>,
    updated_by: String,
    updated_at: i64,
}

impl From<LlmFunctionConfigRow> for LlmFunctionConfig {
    fn from(r: LlmFunctionConfigRow) -> Self {
        LlmFunctionConfig {
            id: Id::new(r.id),
            workspace_id: r.workspace_id.map(Id::new),
            function_key: r.function_key,
            model_name: r.model_name,
            max_tokens: r.max_tokens.map(|v| v as u32),
            updated_by: Id::new(r.updated_by),
            updated_at: r.updated_at as u64,
        }
    }
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = llm_function_configs)]
struct UpsertRow<'a> {
    id: &'a str,
    workspace_id: Option<&'a str>,
    function_key: &'a str,
    model_name: &'a str,
    max_tokens: Option<i32>,
    updated_by: &'a str,
    updated_at: i64,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[async_trait]
impl LlmConfigRepository for PgStorage {
    async fn get_effective(
        &self,
        workspace_id: &Id,
        function_key: &str,
    ) -> Result<Option<LlmFunctionConfig>> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        let fk = function_key.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<LlmFunctionConfig>> {
            let mut conn = pool.get().context("get db connection")?;
            // Try workspace override first.
            let ws_row = llm_function_configs::table
                .filter(llm_function_configs::workspace_id.eq(&ws_id))
                .filter(llm_function_configs::function_key.eq(&fk))
                .first::<LlmFunctionConfigRow>(&mut *conn)
                .optional()
                .context("query workspace llm config")?;
            if let Some(row) = ws_row {
                return Ok(Some(row.into()));
            }
            // Fall back to tenant default (workspace_id IS NULL).
            let default_row = llm_function_configs::table
                .filter(llm_function_configs::workspace_id.is_null())
                .filter(llm_function_configs::function_key.eq(&fk))
                .first::<LlmFunctionConfigRow>(&mut *conn)
                .optional()
                .context("query tenant default llm config")?;
            Ok(default_row.map(Into::into))
        })
        .await?
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<LlmFunctionConfig>> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<LlmFunctionConfig>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = llm_function_configs::table
                .filter(llm_function_configs::workspace_id.eq(&ws_id))
                .load::<LlmFunctionConfigRow>(&mut *conn)
                .context("list workspace llm configs")?;
            Ok(rows.into_iter().map(Into::into).collect())
        })
        .await?
    }

    async fn upsert_workspace(
        &self,
        workspace_id: &Id,
        function_key: &str,
        model_name: &str,
        max_tokens: Option<u32>,
        updated_by: &Id,
    ) -> Result<LlmFunctionConfig> {
        let pool = Arc::clone(&self.pool);
        let id = Id::new(uuid::Uuid::new_v4().to_string());
        let ws_id = workspace_id.as_str().to_string();
        let fk = function_key.to_string();
        let mn = model_name.to_string();
        let ub = updated_by.as_str().to_string();
        let id_str = id.as_str().to_string();
        let now = now_secs();
        tokio::task::spawn_blocking(move || -> Result<LlmFunctionConfig> {
            let mut conn = pool.get().context("get db connection")?;
            let row = UpsertRow {
                id: &id_str,
                workspace_id: Some(ws_id.as_str()),
                function_key: fk.as_str(),
                model_name: mn.as_str(),
                max_tokens: max_tokens.map(|v| v as i32),
                updated_by: ub.as_str(),
                updated_at: now,
            };
            diesel::insert_into(llm_function_configs::table)
                .values(&row)
                .on_conflict((
                    llm_function_configs::workspace_id,
                    llm_function_configs::function_key,
                ))
                .do_update()
                .set((
                    llm_function_configs::model_name.eq(mn.as_str()),
                    llm_function_configs::max_tokens.eq(max_tokens.map(|v| v as i32)),
                    llm_function_configs::updated_by.eq(ub.as_str()),
                    llm_function_configs::updated_at.eq(now),
                ))
                .execute(&mut *conn)
                .context("upsert workspace llm config")?;
            let saved = llm_function_configs::table
                .filter(llm_function_configs::workspace_id.eq(ws_id.as_str()))
                .filter(llm_function_configs::function_key.eq(fk.as_str()))
                .first::<LlmFunctionConfigRow>(&mut *conn)
                .context("fetch upserted workspace llm config")?;
            Ok(saved.into())
        })
        .await?
    }

    async fn upsert_tenant_default(
        &self,
        function_key: &str,
        model_name: &str,
        max_tokens: Option<u32>,
        updated_by: &Id,
    ) -> Result<LlmFunctionConfig> {
        let pool = Arc::clone(&self.pool);
        let id = Id::new(uuid::Uuid::new_v4().to_string());
        let fk = function_key.to_string();
        let mn = model_name.to_string();
        let ub = updated_by.as_str().to_string();
        let id_str = id.as_str().to_string();
        let now = now_secs();
        tokio::task::spawn_blocking(move || -> Result<LlmFunctionConfig> {
            let mut conn = pool.get().context("get db connection")?;
            // For tenant defaults workspace_id is NULL; NULL uniqueness in Postgres is handled
            // the same as SQLite for composite unique constraints: each NULL is distinct.
            // We use SELECT + INSERT/UPDATE pattern for reliability.
            let existing = llm_function_configs::table
                .filter(llm_function_configs::workspace_id.is_null())
                .filter(llm_function_configs::function_key.eq(fk.as_str()))
                .first::<LlmFunctionConfigRow>(&mut *conn)
                .optional()
                .context("check existing tenant default")?;
            if let Some(existing_row) = existing {
                diesel::update(llm_function_configs::table.find(existing_row.id.as_str()))
                    .set((
                        llm_function_configs::model_name.eq(mn.as_str()),
                        llm_function_configs::max_tokens.eq(max_tokens.map(|v| v as i32)),
                        llm_function_configs::updated_by.eq(ub.as_str()),
                        llm_function_configs::updated_at.eq(now),
                    ))
                    .execute(&mut *conn)
                    .context("update tenant default llm config")?;
                let saved = llm_function_configs::table
                    .find(existing_row.id.as_str())
                    .first::<LlmFunctionConfigRow>(&mut *conn)
                    .context("fetch updated tenant default")?;
                Ok(saved.into())
            } else {
                let row = UpsertRow {
                    id: &id_str,
                    workspace_id: None,
                    function_key: fk.as_str(),
                    model_name: mn.as_str(),
                    max_tokens: max_tokens.map(|v| v as i32),
                    updated_by: ub.as_str(),
                    updated_at: now,
                };
                diesel::insert_into(llm_function_configs::table)
                    .values(&row)
                    .execute(&mut *conn)
                    .context("insert tenant default llm config")?;
                let saved = llm_function_configs::table
                    .find(id_str.as_str())
                    .first::<LlmFunctionConfigRow>(&mut *conn)
                    .context("fetch inserted tenant default")?;
                Ok(saved.into())
            }
        })
        .await?
    }

    async fn delete_workspace_override(&self, workspace_id: &Id, function_key: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        let fk = function_key.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(
                llm_function_configs::table
                    .filter(llm_function_configs::workspace_id.eq(&ws_id))
                    .filter(llm_function_configs::function_key.eq(&fk)),
            )
            .execute(&mut *conn)
            .context("delete workspace llm config override")?;
            Ok(())
        })
        .await?
    }

    async fn list_tenant_defaults(&self) -> Result<Vec<LlmFunctionConfig>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<LlmFunctionConfig>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = llm_function_configs::table
                .filter(llm_function_configs::workspace_id.is_null())
                .load::<LlmFunctionConfigRow>(&mut *conn)
                .context("list tenant default llm configs")?;
            Ok(rows.into_iter().map(Into::into).collect())
        })
        .await?
    }
}
