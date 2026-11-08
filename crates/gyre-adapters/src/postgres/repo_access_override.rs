use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{RepoAccessOverride, WorkspaceRole};
use gyre_ports::RepoAccessOverrideRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::repo_access_overrides;

#[derive(Queryable, Selectable)]
#[diesel(table_name = repo_access_overrides)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct OverrideRow {
    id: String,
    repo_id: String,
    user_id: String,
    role: String,
    created_by: String,
    created_at: i64,
    updated_at: i64,
}

impl OverrideRow {
    fn into_override(self) -> Result<RepoAccessOverride> {
        let role = WorkspaceRole::parse_role(&self.role)
            .ok_or_else(|| anyhow!("unknown workspace role: {}", self.role))?;
        Ok(RepoAccessOverride {
            id: Id::new(self.id),
            repo_id: Id::new(self.repo_id),
            user_id: Id::new(self.user_id),
            role,
            created_by: Id::new(self.created_by),
            created_at: self.created_at as u64,
            updated_at: self.updated_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = repo_access_overrides)]
struct NewOverrideRow<'a> {
    id: &'a str,
    repo_id: &'a str,
    user_id: &'a str,
    role: &'a str,
    created_by: &'a str,
    created_at: i64,
    updated_at: i64,
}

#[async_trait]
impl RepoAccessOverrideRepository for PgStorage {
    async fn create(&self, override_: &RepoAccessOverride) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let o = override_.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewOverrideRow {
                id: o.id.as_str(),
                repo_id: o.repo_id.as_str(),
                user_id: o.user_id.as_str(),
                role: o.role.as_str(),
                created_by: o.created_by.as_str(),
                created_at: o.created_at as i64,
                updated_at: o.updated_at as i64,
            };
            // UNIQUE(repo_id, user_id) rejects duplicates; map the Diesel
            // error to `anyhow!` text the caller can branch on.
            diesel::insert_into(repo_access_overrides::table)
                .values(&row)
                .execute(&mut *conn)
                .map_err(|e| {
                    if e.to_string().contains("unique") || e.to_string().contains("duplicate") {
                        anyhow!(
                            "repo access override for repo {} user {} already exists",
                            o.repo_id,
                            o.user_id
                        )
                    } else {
                        anyhow!("insert repo access override: {e}")
                    }
                })?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<RepoAccessOverride>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<RepoAccessOverride>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = repo_access_overrides::table
                .find(id.as_str())
                .first::<OverrideRow>(&mut *conn)
                .optional()
                .context("find repo access override by id")?;
            result.map(OverrideRow::into_override).transpose()
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
                .context("find repo access override by repo and user")?;
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
                .context("list repo access overrides by repo")?;
            rows.into_iter().map(OverrideRow::into_override).collect()
        })
        .await?
    }

    async fn update_role(&self, id: &Id, role: WorkspaceRole) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(repo_access_overrides::table.find(id.as_str()))
                .set((
                    repo_access_overrides::role.eq(role.as_str()),
                    repo_access_overrides::updated_at.eq(std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64),
                ))
                .execute(&mut *conn)
                .context("update repo access override role")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(repo_access_overrides::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete repo access override")?;
            Ok(())
        })
        .await?
    }
}
