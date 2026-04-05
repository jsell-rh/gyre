use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_ports::saved_view::{SavedView, SavedViewRepository};
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::saved_views;

#[derive(Queryable, Selectable)]
#[diesel(table_name = saved_views)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct SavedViewRow {
    id: String,
    repo_id: String,
    workspace_id: String,
    tenant_id: String,
    name: String,
    description: Option<String>,
    query_json: String,
    created_by: String,
    created_at: i64,
    updated_at: i64,
    is_system: bool,
}

impl SavedViewRow {
    fn into_view(self) -> SavedView {
        SavedView {
            id: Id::new(self.id),
            repo_id: Id::new(self.repo_id),
            workspace_id: Id::new(self.workspace_id),
            tenant_id: Id::new(self.tenant_id),
            name: self.name,
            description: self.description,
            query_json: self.query_json,
            created_by: self.created_by,
            created_at: self.created_at as u64,
            updated_at: self.updated_at as u64,
            is_system: self.is_system,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = saved_views)]
struct NewSavedViewRow<'a> {
    id: &'a str,
    repo_id: &'a str,
    workspace_id: &'a str,
    tenant_id: &'a str,
    name: &'a str,
    description: Option<&'a str>,
    query_json: &'a str,
    created_by: &'a str,
    created_at: i64,
    updated_at: i64,
    is_system: bool,
}

#[async_trait]
impl SavedViewRepository for SqliteStorage {
    async fn create(&self, view: SavedView) -> Result<SavedView> {
        let pool = Arc::clone(&self.pool);
        let v = view.clone();
        tokio::task::spawn_blocking(move || -> Result<SavedView> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewSavedViewRow {
                id: v.id.as_str(),
                repo_id: v.repo_id.as_str(),
                workspace_id: v.workspace_id.as_str(),
                tenant_id: v.tenant_id.as_str(),
                name: &v.name,
                description: v.description.as_deref(),
                query_json: &v.query_json,
                created_by: &v.created_by,
                created_at: v.created_at as i64,
                updated_at: v.updated_at as i64,
                is_system: v.is_system,
            };
            if v.is_system {
                // System views use INSERT OR IGNORE to handle seeding races gracefully
                diesel::insert_or_ignore_into(saved_views::table)
                    .values(&row)
                    .execute(&mut *conn)
                    .context("insert saved view")?;
            } else {
                // User views use regular INSERT to surface duplicate PK errors
                diesel::insert_into(saved_views::table)
                    .values(&row)
                    .execute(&mut *conn)
                    .context("insert saved view")?;
            }
            Ok(v)
        })
        .await?
    }

    async fn get(&self, id: &Id) -> Result<Option<SavedView>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<SavedView>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = saved_views::table
                .find(id.as_str())
                .first::<SavedViewRow>(&mut *conn)
                .optional()
                .context("find saved view by id")?;
            Ok(result.map(SavedViewRow::into_view))
        })
        .await?
    }

    async fn list_by_repo(&self, repo_id: &Id) -> Result<Vec<SavedView>> {
        let pool = Arc::clone(&self.pool);
        let rid = repo_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<SavedView>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = saved_views::table
                .filter(saved_views::repo_id.eq(rid.as_str()))
                .order(saved_views::created_at.asc())
                .load::<SavedViewRow>(&mut *conn)
                .context("list saved views by repo")?;
            Ok(rows.into_iter().map(SavedViewRow::into_view).collect())
        })
        .await?
    }

    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<SavedView>> {
        let pool = Arc::clone(&self.pool);
        let wid = workspace_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<SavedView>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = saved_views::table
                .filter(saved_views::workspace_id.eq(wid.as_str()))
                .order(saved_views::created_at.asc())
                .load::<SavedViewRow>(&mut *conn)
                .context("list saved views by workspace")?;
            Ok(rows.into_iter().map(SavedViewRow::into_view).collect())
        })
        .await?
    }

    async fn list_by_repo_and_tenant(
        &self,
        repo_id: &Id,
        tenant_id: &Id,
    ) -> Result<Vec<SavedView>> {
        let pool = Arc::clone(&self.pool);
        let rid = repo_id.clone();
        let tid = tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<SavedView>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = saved_views::table
                .filter(saved_views::repo_id.eq(rid.as_str()))
                .filter(saved_views::tenant_id.eq(tid.as_str()))
                .order(saved_views::created_at.asc())
                .load::<SavedViewRow>(&mut *conn)
                .context("list saved views by repo and tenant")?;
            Ok(rows.into_iter().map(SavedViewRow::into_view).collect())
        })
        .await?
    }

    async fn update(&self, view: SavedView) -> Result<SavedView> {
        let pool = Arc::clone(&self.pool);
        let v = view.clone();
        tokio::task::spawn_blocking(move || -> Result<SavedView> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = diesel::update(saved_views::table.find(v.id.as_str()))
                .set((
                    saved_views::name.eq(&v.name),
                    saved_views::description.eq(v.description.as_deref()),
                    saved_views::query_json.eq(&v.query_json),
                    saved_views::updated_at.eq(v.updated_at as i64),
                ))
                .execute(&mut *conn)
                .context("update saved view")?;
            if rows == 0 {
                anyhow::bail!("saved view {} not found", v.id);
            }
            Ok(v)
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(saved_views::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete saved view")?;
            Ok(())
        })
        .await?
    }

    async fn delete_scoped(&self, id: &Id, tenant_id: &Id) -> Result<bool> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        let tenant_id = tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            let mut conn = pool.get().context("get db connection")?;
            let deleted = diesel::delete(
                saved_views::table
                    .find(id.as_str())
                    .filter(saved_views::tenant_id.eq(tenant_id.as_str())),
            )
            .execute(&mut *conn)
            .context("delete saved view scoped")?;
            Ok(deleted > 0)
        })
        .await?
    }
}
