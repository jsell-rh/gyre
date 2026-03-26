use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::PromptTemplate;
use gyre_ports::PromptRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::prompt_templates;

// ── Row types ─────────────────────────────────────────────────────────────────

#[derive(Queryable, Selectable, Clone)]
#[diesel(table_name = prompt_templates)]
#[diesel(check_for_backend(diesel::pg::Pg))]
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
impl PromptRepository for PgStorage {
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

            let existing = prompt_templates::table
                .filter(prompt_templates::workspace_id.eq(&ws_id))
                .filter(prompt_templates::function_key.eq(&fkey))
                .first::<PromptTemplateRow>(&mut *conn)
                .optional()
                .context("check existing workspace override")?;

            if let Some(existing_row) = existing {
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

            let existing = prompt_templates::table
                .filter(prompt_templates::workspace_id.is_null())
                .filter(prompt_templates::function_key.eq(&fkey))
                .first::<PromptTemplateRow>(&mut *conn)
                .optional()
                .context("check existing tenant default")?;

            if let Some(existing_row) = existing {
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
