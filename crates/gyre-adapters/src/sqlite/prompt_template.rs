use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::PromptTemplate;
use gyre_ports::PromptRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::prompt_templates;

// ── Row types ─────────────────────────────────────────────────────────────────

#[derive(Queryable, Selectable, Clone)]
#[diesel(table_name = prompt_templates)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct PromptTemplateRow {
    id: String,
    workspace_id: Option<String>,
    function_key: String,
    content: String,
    created_by: String,
    created_at: i64,
    updated_at: i64,
}

impl PromptTemplateRow {
    fn into_domain(self) -> PromptTemplate {
        PromptTemplate {
            id: Id::new(self.id),
            workspace_id: self.workspace_id.map(Id::new),
            function_key: self.function_key,
            content: self.content,
            created_by: Id::new(self.created_by),
            created_at: self.created_at as u64,
            updated_at: self.updated_at as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = prompt_templates)]
struct InsertPromptTemplateRow<'a> {
    id: &'a str,
    workspace_id: Option<&'a str>,
    function_key: &'a str,
    content: &'a str,
    created_by: &'a str,
    created_at: i64,
    updated_at: i64,
}

// ── Repository impl ───────────────────────────────────────────────────────────

#[async_trait]
impl PromptRepository for SqliteStorage {
    async fn get_effective(
        &self,
        workspace_id: &Id,
        function_key: &str,
    ) -> Result<Option<PromptTemplate>> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        let fkey = function_key.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<PromptTemplate>> {
            let mut conn = pool.get().context("get db connection")?;

            // First: workspace override
            let ws_row = prompt_templates::table
                .filter(prompt_templates::workspace_id.eq(&ws_id))
                .filter(prompt_templates::function_key.eq(&fkey))
                .first::<PromptTemplateRow>(&mut *conn)
                .optional()
                .context("get workspace prompt override")?;

            if let Some(row) = ws_row {
                return Ok(Some(row.into_domain()));
            }

            // Second: tenant default (workspace_id IS NULL)
            let tenant_row = prompt_templates::table
                .filter(prompt_templates::workspace_id.is_null())
                .filter(prompt_templates::function_key.eq(&fkey))
                .first::<PromptTemplateRow>(&mut *conn)
                .optional()
                .context("get tenant default prompt")?;

            Ok(tenant_row.map(|r| r.into_domain()))
        })
        .await?
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<PromptTemplate>> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<PromptTemplate>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = prompt_templates::table
                .filter(prompt_templates::workspace_id.eq(&ws_id))
                .load::<PromptTemplateRow>(&mut *conn)
                .context("list workspace prompt templates")?;
            Ok(rows.into_iter().map(|r| r.into_domain()).collect())
        })
        .await?
    }

    async fn upsert_workspace(
        &self,
        workspace_id: &Id,
        function_key: &str,
        content: &str,
        created_by: &Id,
    ) -> Result<PromptTemplate> {
        let pool = Arc::clone(&self.pool);
        let ws_id = workspace_id.as_str().to_string();
        let fkey = function_key.to_string();
        let content = content.to_string();
        let created_by = created_by.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<PromptTemplate> {
            let mut conn = pool.get().context("get db connection")?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

            // Check if override already exists.
            let existing = prompt_templates::table
                .filter(prompt_templates::workspace_id.eq(&ws_id))
                .filter(prompt_templates::function_key.eq(&fkey))
                .first::<PromptTemplateRow>(&mut *conn)
                .optional()
                .context("check existing workspace override")?;

            if let Some(existing_row) = existing {
                // Update existing.
                diesel::update(
                    prompt_templates::table
                        .filter(prompt_templates::id.eq(&existing_row.id)),
                )
                .set((
                    prompt_templates::content.eq(&content),
                    prompt_templates::updated_at.eq(now),
                ))
                .execute(&mut *conn)
                .context("update workspace prompt override")?;

                let updated = prompt_templates::table
                    .filter(prompt_templates::id.eq(&existing_row.id))
                    .first::<PromptTemplateRow>(&mut *conn)
                    .context("fetch updated workspace prompt")?;
                return Ok(updated.into_domain());
            }

            // Insert new.
            let new_id = uuid::Uuid::new_v4().to_string();
            let row = InsertPromptTemplateRow {
                id: &new_id,
                workspace_id: Some(ws_id.as_str()),
                function_key: &fkey,
                content: &content,
                created_by: &created_by,
                created_at: now,
                updated_at: now,
            };
            diesel::insert_into(prompt_templates::table)
                .values(&row)
                .execute(&mut *conn)
                .context("insert workspace prompt override")?;

            let inserted = prompt_templates::table
                .filter(prompt_templates::id.eq(&new_id))
                .first::<PromptTemplateRow>(&mut *conn)
                .context("fetch inserted workspace prompt")?;
            Ok(inserted.into_domain())
        })
        .await?
    }

    async fn upsert_tenant_default(
        &self,
        function_key: &str,
        content: &str,
        created_by: &Id,
    ) -> Result<PromptTemplate> {
        let pool = Arc::clone(&self.pool);
        let fkey = function_key.to_string();
        let content = content.to_string();
        let created_by = created_by.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<PromptTemplate> {
            let mut conn = pool.get().context("get db connection")?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

            // Check if tenant default already exists.
            let existing = prompt_templates::table
                .filter(prompt_templates::workspace_id.is_null())
                .filter(prompt_templates::function_key.eq(&fkey))
                .first::<PromptTemplateRow>(&mut *conn)
                .optional()
                .context("check existing tenant default")?;

            if let Some(existing_row) = existing {
                // Update existing.
                diesel::update(
                    prompt_templates::table
                        .filter(prompt_templates::id.eq(&existing_row.id)),
                )
                .set((
                    prompt_templates::content.eq(&content),
                    prompt_templates::updated_at.eq(now),
                ))
                .execute(&mut *conn)
                .context("update tenant default prompt")?;

                let updated = prompt_templates::table
                    .filter(prompt_templates::id.eq(&existing_row.id))
                    .first::<PromptTemplateRow>(&mut *conn)
                    .context("fetch updated tenant default")?;
                return Ok(updated.into_domain());
            }

            // Insert new tenant default (workspace_id = NULL).
            let new_id = uuid::Uuid::new_v4().to_string();
            let row = InsertPromptTemplateRow {
                id: &new_id,
                workspace_id: None,
                function_key: &fkey,
                content: &content,
                created_by: &created_by,
                created_at: now,
                updated_at: now,
            };
            diesel::insert_into(prompt_templates::table)
                .values(&row)
                .execute(&mut *conn)
                .context("insert tenant default prompt")?;

            let inserted = prompt_templates::table
                .filter(prompt_templates::id.eq(&new_id))
                .first::<PromptTemplateRow>(&mut *conn)
                .context("fetch inserted tenant default")?;
            Ok(inserted.into_domain())
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
        let fkey = function_key.to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(
                prompt_templates::table
                    .filter(prompt_templates::workspace_id.eq(&ws_id))
                    .filter(prompt_templates::function_key.eq(&fkey)),
            )
            .execute(&mut *conn)
            .context("delete workspace prompt override")?;
            Ok(())
        })
        .await?
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_common::Id;

    fn test_storage() -> SqliteStorage {
        SqliteStorage::new(":memory:").expect("open in-memory db")
    }

    #[tokio::test]
    async fn upsert_workspace_and_get_effective_round_trip() {
        let storage = test_storage();
        let ws_id = Id::new(uuid::Uuid::new_v4().to_string());
        let caller = Id::new(uuid::Uuid::new_v4().to_string());

        storage
            .upsert_workspace(&ws_id, "briefing-ask", "Hello {{question}}", &caller)
            .await
            .expect("upsert");

        let result = storage
            .get_effective(&ws_id, "briefing-ask")
            .await
            .expect("get_effective");

        let tmpl = result.expect("should have effective prompt");
        assert_eq!(tmpl.function_key, "briefing-ask");
        assert_eq!(tmpl.content, "Hello {{question}}");
        assert_eq!(tmpl.workspace_id, Some(ws_id));
    }

    #[tokio::test]
    async fn tenant_default_fallback_when_no_workspace_override() {
        let storage = test_storage();
        let ws_id = Id::new(uuid::Uuid::new_v4().to_string());
        let caller = Id::new(uuid::Uuid::new_v4().to_string());

        // Set only tenant default — no workspace override.
        storage
            .upsert_tenant_default("graph-predict", "Tenant default", &caller)
            .await
            .expect("upsert tenant default");

        let result = storage
            .get_effective(&ws_id, "graph-predict")
            .await
            .expect("get_effective");

        let tmpl = result.expect("should fall back to tenant default");
        assert_eq!(tmpl.function_key, "graph-predict");
        assert_eq!(tmpl.content, "Tenant default");
        assert_eq!(tmpl.workspace_id, None, "tenant default has no workspace_id");
    }

    #[tokio::test]
    async fn workspace_override_takes_precedence_over_tenant_default() {
        let storage = test_storage();
        let ws_id = Id::new(uuid::Uuid::new_v4().to_string());
        let caller = Id::new(uuid::Uuid::new_v4().to_string());

        storage
            .upsert_tenant_default("specs-assist", "Tenant content", &caller)
            .await
            .expect("upsert tenant default");

        storage
            .upsert_workspace(&ws_id, "specs-assist", "Workspace content", &caller)
            .await
            .expect("upsert workspace override");

        let result = storage
            .get_effective(&ws_id, "specs-assist")
            .await
            .expect("get_effective");

        let tmpl = result.expect("should have effective prompt");
        assert_eq!(tmpl.content, "Workspace content");
        assert_eq!(tmpl.workspace_id, Some(ws_id));
    }

    #[tokio::test]
    async fn delete_workspace_override_reverts_to_tenant_default() {
        let storage = test_storage();
        let ws_id = Id::new(uuid::Uuid::new_v4().to_string());
        let caller = Id::new(uuid::Uuid::new_v4().to_string());

        storage
            .upsert_tenant_default("explorer-generate", "Tenant fallback", &caller)
            .await
            .expect("upsert tenant default");

        storage
            .upsert_workspace(&ws_id, "explorer-generate", "WS override", &caller)
            .await
            .expect("upsert workspace override");

        storage
            .delete_workspace_override(&ws_id, "explorer-generate")
            .await
            .expect("delete override");

        let result = storage
            .get_effective(&ws_id, "explorer-generate")
            .await
            .expect("get_effective");

        let tmpl = result.expect("should fall back to tenant default");
        assert_eq!(tmpl.content, "Tenant fallback");
        assert_eq!(tmpl.workspace_id, None);
    }

    #[tokio::test]
    async fn get_effective_returns_none_when_no_template() {
        let storage = test_storage();
        let ws_id = Id::new(uuid::Uuid::new_v4().to_string());
        let result = storage
            .get_effective(&ws_id, "briefing-ask")
            .await
            .expect("get_effective");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn upsert_workspace_updates_existing() {
        let storage = test_storage();
        let ws_id = Id::new(uuid::Uuid::new_v4().to_string());
        let caller = Id::new(uuid::Uuid::new_v4().to_string());

        storage
            .upsert_workspace(&ws_id, "briefing-ask", "v1", &caller)
            .await
            .expect("upsert v1");

        let updated = storage
            .upsert_workspace(&ws_id, "briefing-ask", "v2", &caller)
            .await
            .expect("upsert v2");

        assert_eq!(updated.content, "v2");
    }
}
