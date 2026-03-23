use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::Repository;
use gyre_ports::RepoRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::repositories;

#[derive(Queryable, Selectable)]
#[diesel(table_name = repositories)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct RepositoryRow {
    id: String,
    project_id: String,
    name: String,
    path: String,
    default_branch: String,
    created_at: i64,
    is_mirror: i32,
    mirror_url: Option<String>,
    mirror_interval_secs: Option<i64>,
    last_mirror_sync: Option<i64>,
    #[allow(dead_code)]
    tenant_id: String,
    workspace_id: Option<String>,
}

impl From<RepositoryRow> for Repository {
    fn from(r: RepositoryRow) -> Self {
        Repository {
            id: Id::new(r.id),
            project_id: Id::new(r.project_id),
            name: r.name,
            path: r.path,
            default_branch: r.default_branch,
            created_at: r.created_at as u64,
            is_mirror: r.is_mirror != 0,
            mirror_url: r.mirror_url,
            mirror_interval_secs: r.mirror_interval_secs.map(|v| v as u64),
            last_mirror_sync: r.last_mirror_sync.map(|v| v as u64),
            workspace_id: r.workspace_id.map(Id::new),
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = repositories)]
struct NewRepositoryRow<'a> {
    id: &'a str,
    project_id: &'a str,
    name: &'a str,
    path: &'a str,
    default_branch: &'a str,
    created_at: i64,
    is_mirror: i32,
    mirror_url: Option<&'a str>,
    mirror_interval_secs: Option<i64>,
    last_mirror_sync: Option<i64>,
    tenant_id: &'a str,
    workspace_id: Option<&'a str>,
}

#[async_trait]
impl RepoRepository for PgStorage {
    async fn create(&self, repo: &Repository) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let r = repo.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewRepositoryRow {
                id: r.id.as_str(),
                project_id: r.project_id.as_str(),
                name: &r.name,
                path: &r.path,
                default_branch: &r.default_branch,
                created_at: r.created_at as i64,
                is_mirror: r.is_mirror as i32,
                mirror_url: r.mirror_url.as_deref(),
                mirror_interval_secs: r.mirror_interval_secs.map(|v| v as i64),
                last_mirror_sync: r.last_mirror_sync.map(|v| v as i64),
                tenant_id: "default",
                workspace_id: r.workspace_id.as_ref().map(|id| id.as_str()),
            };
            diesel::insert_into(repositories::table)
                .values(&row)
                .on_conflict(repositories::id)
                .do_update()
                .set((
                    repositories::project_id.eq(row.project_id),
                    repositories::name.eq(row.name),
                    repositories::path.eq(row.path),
                    repositories::default_branch.eq(row.default_branch),
                    repositories::is_mirror.eq(row.is_mirror),
                    repositories::mirror_url.eq(row.mirror_url),
                    repositories::mirror_interval_secs.eq(row.mirror_interval_secs),
                    repositories::last_mirror_sync.eq(row.last_mirror_sync),
                    repositories::workspace_id.eq(row.workspace_id),
                ))
                .execute(&mut *conn)
                .context("insert repository")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Repository>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Repository>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = repositories::table
                .find(id.as_str())
                .first::<RepositoryRow>(&mut *conn)
                .optional()
                .context("find repository by id")?;
            Ok(result.map(Repository::from))
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<Repository>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<Repository>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = repositories::table
                .order(repositories::created_at.asc())
                .load::<RepositoryRow>(&mut *conn)
                .context("list repositories")?;
            Ok(rows.into_iter().map(Repository::from).collect())
        })
        .await?
    }

    async fn list_by_project(&self, project_id: &Id) -> Result<Vec<Repository>> {
        let pool = Arc::clone(&self.pool);
        let project_id = project_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Repository>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = repositories::table
                .filter(repositories::project_id.eq(project_id.as_str()))
                .order(repositories::created_at.asc())
                .load::<RepositoryRow>(&mut *conn)
                .context("list repositories by project")?;
            Ok(rows.into_iter().map(Repository::from).collect())
        })
        .await?
    }

    async fn update(&self, repo: &Repository) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let r = repo.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(repositories::table.find(r.id.as_str()))
                .set((
                    repositories::project_id.eq(r.project_id.as_str()),
                    repositories::name.eq(&r.name),
                    repositories::path.eq(&r.path),
                    repositories::default_branch.eq(&r.default_branch),
                    repositories::is_mirror.eq(r.is_mirror as i32),
                    repositories::mirror_url.eq(r.mirror_url.as_deref()),
                    repositories::mirror_interval_secs.eq(r.mirror_interval_secs.map(|v| v as i64)),
                    repositories::last_mirror_sync.eq(r.last_mirror_sync.map(|v| v as i64)),
                    repositories::workspace_id.eq(r.workspace_id.as_ref().map(|id| id.as_str())),
                ))
                .execute(&mut *conn)
                .context("update repository")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(repositories::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete repository")?;
            Ok(())
        })
        .await?
    }
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Repository>> {
        let pool = Arc::clone(&self.pool);
        let workspace_id = workspace_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Repository>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = repositories::table
                .filter(repositories::workspace_id.eq(workspace_id.as_str()))
                .order(repositories::created_at.asc())
                .load::<RepositoryRow>(&mut *conn)
                .context("list repositories by workspace")?;
            Ok(rows.into_iter().map(Repository::from).collect())
        })
        .await?
    }
}
