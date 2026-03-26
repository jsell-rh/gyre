use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::LlmFunctionConfig;
use gyre_ports::LlmConfigRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::llm_function_configs;

#[derive(Queryable, Selectable)]
#[diesel(table_name = llm_function_configs)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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
impl LlmConfigRepository for SqliteStorage {
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
            // Fetch back the actual row (may have updated an existing id).
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
            // For tenant defaults workspace_id is NULL; SQLite UNIQUE treats NULLs as distinct,
            // so we use a manual SELECT + INSERT/UPDATE pattern.
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

    async fn delete_workspace_override(
        &self,
        workspace_id: &Id,
        function_key: &str,
    ) -> Result<()> {
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
    async fn get_effective_returns_none_when_no_config() {
        let (_tmp, s) = setup();
        let result = LlmConfigRepository::get_effective(
            &s,
            &Id::new("ws-1"),
            "briefing-ask",
        )
        .await
        .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn upsert_workspace_and_get_effective_round_trip() {
        let (_tmp, s) = setup();
        let ws = Id::new("ws-1");
        let by = Id::new("user-1");
        let cfg = LlmConfigRepository::upsert_workspace(
            &s,
            &ws,
            "briefing-ask",
            "gemini-1.5-pro-002",
            Some(4096),
            &by,
        )
        .await
        .unwrap();
        assert_eq!(cfg.model_name, "gemini-1.5-pro-002");
        assert_eq!(cfg.max_tokens, Some(4096));

        let got = LlmConfigRepository::get_effective(&s, &ws, "briefing-ask")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.model_name, "gemini-1.5-pro-002");
    }

    #[tokio::test]
    async fn tenant_default_fallback() {
        let (_tmp, s) = setup();
        let ws = Id::new("ws-1");
        let by = Id::new("user-1");
        // Set tenant default.
        LlmConfigRepository::upsert_tenant_default(
            &s,
            "graph-predict",
            "gemini-2.0-flash-001",
            None,
            &by,
        )
        .await
        .unwrap();
        // Workspace has no override — should return tenant default.
        let got = LlmConfigRepository::get_effective(&s, &ws, "graph-predict")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.model_name, "gemini-2.0-flash-001");
        assert!(got.workspace_id.is_none());
    }

    #[tokio::test]
    async fn workspace_override_takes_precedence_over_tenant_default() {
        let (_tmp, s) = setup();
        let ws = Id::new("ws-1");
        let by = Id::new("user-1");
        LlmConfigRepository::upsert_tenant_default(
            &s,
            "explorer-generate",
            "gemini-2.0-flash-001",
            None,
            &by,
        )
        .await
        .unwrap();
        LlmConfigRepository::upsert_workspace(
            &s,
            &ws,
            "explorer-generate",
            "gemini-1.5-pro-002",
            None,
            &by,
        )
        .await
        .unwrap();
        let got = LlmConfigRepository::get_effective(&s, &ws, "explorer-generate")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.model_name, "gemini-1.5-pro-002");
        assert!(got.workspace_id.is_some());
    }

    #[tokio::test]
    async fn delete_workspace_override_reverts_to_default() {
        let (_tmp, s) = setup();
        let ws = Id::new("ws-1");
        let by = Id::new("user-1");
        LlmConfigRepository::upsert_tenant_default(
            &s,
            "specs-assist",
            "gemini-2.0-flash-001",
            None,
            &by,
        )
        .await
        .unwrap();
        LlmConfigRepository::upsert_workspace(
            &s,
            &ws,
            "specs-assist",
            "gemini-1.5-pro-002",
            None,
            &by,
        )
        .await
        .unwrap();
        LlmConfigRepository::delete_workspace_override(&s, &ws, "specs-assist")
            .await
            .unwrap();
        let got = LlmConfigRepository::get_effective(&s, &ws, "specs-assist")
            .await
            .unwrap()
            .unwrap();
        // Should fall back to tenant default.
        assert_eq!(got.model_name, "gemini-2.0-flash-001");
    }
}
