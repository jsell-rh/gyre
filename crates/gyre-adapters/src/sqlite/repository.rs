use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{RepoStatus, Repository};
use gyre_ports::RepoRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::repositories;

#[derive(Queryable, Selectable)]
#[diesel(table_name = repositories)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct RepositoryRow {
    id: String,
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
    workspace_id: String,
    description: Option<String>,
    status: String,
    updated_at: i64,
}

impl From<RepositoryRow> for Repository {
    fn from(r: RepositoryRow) -> Self {
        use std::str::FromStr;
        Repository {
            id: Id::new(r.id),
            name: r.name,
            path: r.path,
            default_branch: r.default_branch,
            created_at: r.created_at as u64,
            is_mirror: r.is_mirror != 0,
            mirror_url: r.mirror_url,
            mirror_interval_secs: r.mirror_interval_secs.map(|v| v as u64),
            last_mirror_sync: r.last_mirror_sync.map(|v| v as u64),
            workspace_id: Id::new(r.workspace_id),
            description: r.description,
            status: RepoStatus::from_str(&r.status).unwrap_or(RepoStatus::Active),
            updated_at: r.updated_at as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = repositories)]
struct NewRepositoryRow<'a> {
    id: &'a str,
    name: &'a str,
    path: &'a str,
    default_branch: &'a str,
    created_at: i64,
    is_mirror: i32,
    mirror_url: Option<&'a str>,
    mirror_interval_secs: Option<i64>,
    last_mirror_sync: Option<i64>,
    tenant_id: &'a str,
    workspace_id: &'a str,
    description: Option<&'a str>,
    status: &'a str,
    updated_at: i64,
}

#[async_trait]
impl RepoRepository for SqliteStorage {
    async fn create(&self, repo: &Repository) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let r = repo.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let status_str = r.status.to_string();
            let row = NewRepositoryRow {
                id: r.id.as_str(),
                name: &r.name,
                path: &r.path,
                default_branch: &r.default_branch,
                created_at: r.created_at as i64,
                is_mirror: r.is_mirror as i32,
                mirror_url: r.mirror_url.as_deref(),
                mirror_interval_secs: r.mirror_interval_secs.map(|v| v as i64),
                last_mirror_sync: r.last_mirror_sync.map(|v| v as i64),
                tenant_id: "default",
                workspace_id: r.workspace_id.as_str(),
                description: r.description.as_deref(),
                status: &status_str,
                updated_at: r.updated_at as i64,
            };
            diesel::insert_into(repositories::table)
                .values(&row)
                .on_conflict(repositories::id)
                .do_update()
                .set((
                    repositories::name.eq(row.name),
                    repositories::path.eq(row.path),
                    repositories::default_branch.eq(row.default_branch),
                    repositories::is_mirror.eq(row.is_mirror),
                    repositories::mirror_url.eq(row.mirror_url),
                    repositories::mirror_interval_secs.eq(row.mirror_interval_secs),
                    repositories::last_mirror_sync.eq(row.last_mirror_sync),
                    repositories::workspace_id.eq(row.workspace_id),
                    repositories::description.eq(row.description),
                    repositories::status.eq(row.status),
                    repositories::updated_at.eq(row.updated_at),
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
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Repository>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = repositories::table
                .filter(repositories::tenant_id.eq(&tenant))
                .filter(repositories::id.eq(id.as_str()))
                .first::<RepositoryRow>(&mut *conn)
                .optional()
                .context("find repository by id")?;
            Ok(result.map(Repository::from))
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<Repository>> {
        let pool = Arc::clone(&self.pool);
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Repository>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = repositories::table
                .filter(repositories::tenant_id.eq(&tenant))
                .order(repositories::created_at.asc())
                .load::<RepositoryRow>(&mut *conn)
                .context("list repositories")?;
            Ok(rows.into_iter().map(Repository::from).collect())
        })
        .await?
    }

    async fn update(&self, repo: &Repository) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let r = repo.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let status_str = r.status.to_string();
            diesel::update(repositories::table.find(r.id.as_str()))
                .set((
                    repositories::name.eq(&r.name),
                    repositories::path.eq(&r.path),
                    repositories::default_branch.eq(&r.default_branch),
                    repositories::is_mirror.eq(r.is_mirror as i32),
                    repositories::mirror_url.eq(r.mirror_url.as_deref()),
                    repositories::mirror_interval_secs.eq(r.mirror_interval_secs.map(|v| v as i64)),
                    repositories::last_mirror_sync.eq(r.last_mirror_sync.map(|v| v as i64)),
                    repositories::workspace_id.eq(r.workspace_id.as_str()),
                    repositories::description.eq(r.description.as_deref()),
                    repositories::status.eq(&status_str),
                    repositories::updated_at.eq(r.updated_at as i64),
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
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Repository>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = repositories::table
                .filter(repositories::tenant_id.eq(&tenant))
                .filter(repositories::workspace_id.eq(workspace_id.as_str()))
                .order(repositories::created_at.asc())
                .load::<RepositoryRow>(&mut *conn)
                .context("list repositories by workspace")?;
            Ok(rows.into_iter().map(Repository::from).collect())
        })
        .await?
    }

    async fn find_by_name_and_workspace(
        &self,
        workspace_id: &Id,
        name: &str,
    ) -> Result<Option<Repository>> {
        let pool = Arc::clone(&self.pool);
        let workspace_id = workspace_id.clone();
        let name = name.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Repository>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = repositories::table
                .filter(repositories::workspace_id.eq(workspace_id.as_str()))
                .filter(repositories::name.eq(&name))
                .first::<RepositoryRow>(&mut *conn)
                .optional()
                .context("find repository by name and workspace")?;
            Ok(result.map(Repository::from))
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

    fn make_repo(id: &str, workspace_id: &str) -> Repository {
        Repository::new(
            Id::new(id),
            Id::new(workspace_id),
            format!("repo-{}", id),
            format!("/repos/{}", id),
            1000,
        )
    }

    #[tokio::test]
    async fn create_and_find() {
        let (_tmp, s) = setup();
        let r = make_repo("r1", "ws1");
        RepoRepository::create(&s, &r).await.unwrap();
        let found = RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.name, "repo-r1");
        assert_eq!(found.workspace_id, Id::new("ws1"));
        assert_eq!(found.default_branch, "main");
        assert_eq!(found.status, RepoStatus::Active);
        assert!(found.description.is_none());
    }

    #[tokio::test]
    async fn find_missing_returns_none() {
        let (_tmp, s) = setup();
        assert!(RepoRepository::find_by_id(&s, &Id::new("nope"))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn list_repositories() {
        let (_tmp, s) = setup();
        RepoRepository::create(&s, &make_repo("r1", "ws1"))
            .await
            .unwrap();
        RepoRepository::create(&s, &make_repo("r2", "ws1"))
            .await
            .unwrap();
        assert_eq!(RepoRepository::list(&s).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn list_by_workspace() {
        let (_tmp, s) = setup();
        RepoRepository::create(&s, &make_repo("r1", "ws1"))
            .await
            .unwrap();
        RepoRepository::create(&s, &make_repo("r2", "ws1"))
            .await
            .unwrap();
        RepoRepository::create(&s, &make_repo("r3", "ws2"))
            .await
            .unwrap();

        let ws1_repos = RepoRepository::list_by_workspace(&s, &Id::new("ws1"))
            .await
            .unwrap();
        assert_eq!(ws1_repos.len(), 2);
        let ws2_repos = RepoRepository::list_by_workspace(&s, &Id::new("ws2"))
            .await
            .unwrap();
        assert_eq!(ws2_repos.len(), 1);
    }

    #[tokio::test]
    async fn update_repository() {
        let (_tmp, s) = setup();
        let mut r = make_repo("r1", "ws1");
        RepoRepository::create(&s, &r).await.unwrap();
        r.default_branch = "develop".to_string();
        r.path = "/new/path".to_string();
        RepoRepository::update(&s, &r).await.unwrap();
        let found = RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.default_branch, "develop");
        assert_eq!(found.path, "/new/path");
    }

    #[tokio::test]
    async fn delete_repository() {
        let (_tmp, s) = setup();
        let r = make_repo("r1", "ws1");
        RepoRepository::create(&s, &r).await.unwrap();
        RepoRepository::delete(&s, &r.id).await.unwrap();
        assert!(RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn archive_and_unarchive() {
        let (_tmp, s) = setup();
        let mut r = make_repo("r1", "ws1");
        RepoRepository::create(&s, &r).await.unwrap();

        r.archive();
        RepoRepository::update(&s, &r).await.unwrap();

        let found = RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.status, RepoStatus::Archived);

        r.unarchive();
        RepoRepository::update(&s, &r).await.unwrap();

        let found2 = RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found2.status, RepoStatus::Active);
    }

    #[tokio::test]
    async fn description_persists() {
        let (_tmp, s) = setup();
        let mut r = make_repo("r1", "ws1");
        r.description = Some("my description".to_string());
        RepoRepository::create(&s, &r).await.unwrap();

        let found = RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.description.as_deref(), Some("my description"));
    }
}
