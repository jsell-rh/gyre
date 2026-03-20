use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::Repository;
use gyre_ports::RepoRepository;

use super::{open_conn, SqliteStorage};

fn row_to_repo(row: &rusqlite::Row<'_>) -> rusqlite::Result<Repository> {
    Ok(Repository {
        id: Id::new(row.get::<_, String>(0)?),
        project_id: Id::new(row.get::<_, String>(1)?),
        name: row.get(2)?,
        path: row.get(3)?,
        default_branch: row.get(4)?,
        created_at: row.get::<_, i64>(5)? as u64,
        is_mirror: false,
        mirror_url: None,
        mirror_interval_secs: None,
        last_mirror_sync: None,
    })
}

#[async_trait]
impl RepoRepository for SqliteStorage {
    async fn create(&self, repo: &Repository) -> Result<()> {
        let path = self.db_path();
        let r = repo.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "INSERT INTO repositories (id, project_id, name, path, default_branch, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    r.id.as_str(),
                    r.project_id.as_str(),
                    r.name,
                    r.path,
                    r.default_branch,
                    r.created_at as i64,
                ],
            )
            .context("insert repository")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Repository>> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Repository>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, project_id, name, path, default_branch, created_at
                 FROM repositories WHERE id = ?1",
            )?;
            let mut rows = stmt.query([id.as_str()])?;
            if let Some(row) = rows.next()? {
                Ok(Some(row_to_repo(row)?))
            } else {
                Ok(None)
            }
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<Repository>> {
        let path = self.db_path();
        tokio::task::spawn_blocking(move || -> Result<Vec<Repository>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, project_id, name, path, default_branch, created_at
                 FROM repositories ORDER BY created_at",
            )?;
            let rows = stmt.query_map([], row_to_repo)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn list_by_project(&self, project_id: &Id) -> Result<Vec<Repository>> {
        let path = self.db_path();
        let project_id = project_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Repository>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, project_id, name, path, default_branch, created_at
                 FROM repositories WHERE project_id = ?1 ORDER BY created_at",
            )?;
            let rows = stmt.query_map([project_id.as_str()], row_to_repo)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn update(&self, repo: &Repository) -> Result<()> {
        let path = self.db_path();
        let r = repo.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "UPDATE repositories SET project_id=?1, name=?2, path=?3, default_branch=?4
                 WHERE id=?5",
                rusqlite::params![
                    r.project_id.as_str(),
                    r.name,
                    r.path,
                    r.default_branch,
                    r.id.as_str(),
                ],
            )
            .context("update repository")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute("DELETE FROM repositories WHERE id=?1", [id.as_str()])
                .context("delete repository")?;
            Ok(())
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteStorage;
    use gyre_domain::Project;
    use gyre_ports::ProjectRepository;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    async fn create_project(s: &SqliteStorage, id: &str) {
        let p = Project::new(Id::new(id), format!("proj-{}", id), 1000);
        ProjectRepository::create(s, &p).await.unwrap();
    }

    fn make_repo(id: &str, project_id: &str) -> Repository {
        Repository::new(
            Id::new(id),
            Id::new(project_id),
            format!("repo-{}", id),
            format!("/repos/{}", id),
            1000,
        )
    }

    #[tokio::test]
    async fn create_and_find() {
        let (_tmp, s) = setup();
        create_project(&s, "p1").await;
        let r = make_repo("r1", "p1");
        RepoRepository::create(&s, &r).await.unwrap();
        let found = RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.name, "repo-r1");
        assert_eq!(found.project_id, Id::new("p1"));
        assert_eq!(found.default_branch, "main");
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
        create_project(&s, "p1").await;
        RepoRepository::create(&s, &make_repo("r1", "p1"))
            .await
            .unwrap();
        RepoRepository::create(&s, &make_repo("r2", "p1"))
            .await
            .unwrap();
        assert_eq!(RepoRepository::list(&s).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn list_by_project() {
        let (_tmp, s) = setup();
        create_project(&s, "p1").await;
        create_project(&s, "p2").await;
        RepoRepository::create(&s, &make_repo("r1", "p1"))
            .await
            .unwrap();
        RepoRepository::create(&s, &make_repo("r2", "p1"))
            .await
            .unwrap();
        RepoRepository::create(&s, &make_repo("r3", "p2"))
            .await
            .unwrap();

        let p1_repos = RepoRepository::list_by_project(&s, &Id::new("p1"))
            .await
            .unwrap();
        assert_eq!(p1_repos.len(), 2);
        let p2_repos = RepoRepository::list_by_project(&s, &Id::new("p2"))
            .await
            .unwrap();
        assert_eq!(p2_repos.len(), 1);
    }

    #[tokio::test]
    async fn update_repository() {
        let (_tmp, s) = setup();
        create_project(&s, "p1").await;
        let mut r = make_repo("r1", "p1");
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
        create_project(&s, "p1").await;
        let r = make_repo("r1", "p1");
        RepoRepository::create(&s, &r).await.unwrap();
        RepoRepository::delete(&s, &r.id).await.unwrap();
        assert!(RepoRepository::find_by_id(&s, &r.id)
            .await
            .unwrap()
            .is_none());
    }
}
