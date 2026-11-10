use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{RepoAccessOverride, WorkspaceRole};
use gyre_ports::RepoAccessOverrideRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::repo_access_overrides;

#[derive(Queryable, Selectable)]
#[diesel(table_name = repo_access_overrides)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct OverrideRow {
    id: String,
    repo_id: String,
    user_id: String,
    restricted_role: String,
    created_by: String,
    created_at: i64,
}

impl OverrideRow {
    fn into_override(self) -> Result<RepoAccessOverride> {
        let restricted_role = WorkspaceRole::parse_role(&self.restricted_role)
            .ok_or_else(|| anyhow!("unknown workspace role: {}", self.restricted_role))?;
        Ok(RepoAccessOverride {
            id: Id::new(self.id),
            repo_id: Id::new(self.repo_id),
            user_id: Id::new(self.user_id),
            restricted_role,
            created_by: Id::new(self.created_by),
            created_at: self.created_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = repo_access_overrides)]
struct NewOverrideRow<'a> {
    id: &'a str,
    repo_id: &'a str,
    user_id: &'a str,
    restricted_role: &'a str,
    created_by: &'a str,
    created_at: i64,
}

#[async_trait]
impl RepoAccessOverrideRepository for SqliteStorage {
    async fn create(&self, o: &RepoAccessOverride) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let o = o.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewOverrideRow {
                id: o.id.as_str(),
                repo_id: o.repo_id.as_str(),
                user_id: o.user_id.as_str(),
                restricted_role: o.restricted_role.as_str(),
                created_by: o.created_by.as_str(),
                created_at: o.created_at as i64,
            };
            // Plain INSERT: a duplicate (repo_id, user_id) hits the UNIQUE
            // constraint and fails — the port contract requires create to
            // fail on an existing pair rather than silently rewrite it.
            diesel::insert_into(repo_access_overrides::table)
                .values(&row)
                .execute(&mut *conn)
                .context("insert repo access override")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_repo_and_user(
        &self,
        repo_id: &Id,
        user_id: &Id,
    ) -> Result<Option<RepoAccessOverride>> {
        let pool = Arc::clone(&self.pool);
        let rid = repo_id.clone();
        let uid = user_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<RepoAccessOverride>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = repo_access_overrides::table
                .filter(repo_access_overrides::repo_id.eq(rid.as_str()))
                .filter(repo_access_overrides::user_id.eq(uid.as_str()))
                .first::<OverrideRow>(&mut *conn)
                .optional()
                .context("find repo access override")?;
            result.map(OverrideRow::into_override).transpose()
        })
        .await?
    }

    async fn list_by_repo(&self, repo_id: &Id) -> Result<Vec<RepoAccessOverride>> {
        let pool = Arc::clone(&self.pool);
        let rid = repo_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<RepoAccessOverride>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = repo_access_overrides::table
                .filter(repo_access_overrides::repo_id.eq(rid.as_str()))
                .order(repo_access_overrides::created_at.asc())
                .load::<OverrideRow>(&mut *conn)
                .context("list repo access overrides")?;
            rows.into_iter().map(OverrideRow::into_override).collect()
        })
        .await?
    }

    async fn delete(&self, repo_id: &Id, user_id: &Id) -> Result<bool> {
        let pool = Arc::clone(&self.pool);
        let rid = repo_id.clone();
        let uid = user_id.clone();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            let mut conn = pool.get().context("get db connection")?;
            let deleted = diesel::delete(
                repo_access_overrides::table
                    .filter(repo_access_overrides::repo_id.eq(rid.as_str()))
                    .filter(repo_access_overrides::user_id.eq(uid.as_str())),
            )
            .execute(&mut *conn)
            .context("delete repo access override")?;
            Ok(deleted > 0)
        })
        .await?
    }
}
