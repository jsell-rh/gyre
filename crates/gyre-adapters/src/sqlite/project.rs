use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::Project;
use gyre_ports::ProjectRepository;

use super::{open_conn, SqliteStorage};

#[async_trait]
impl ProjectRepository for SqliteStorage {
    async fn create(&self, project: &Project) -> Result<()> {
        let path = self.db_path();
        let p = project.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "INSERT INTO projects (id, name, description, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    p.id.as_str(),
                    p.name,
                    p.description,
                    p.created_at as i64,
                    p.updated_at as i64,
                ],
            )
            .context("insert project")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Project>> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Project>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, name, description, created_at, updated_at FROM projects WHERE id = ?1",
            )?;
            let mut rows = stmt.query([id.as_str()])?;
            if let Some(row) = rows.next()? {
                Ok(Some(row_to_project(row)?))
            } else {
                Ok(None)
            }
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<Project>> {
        let path = self.db_path();
        tokio::task::spawn_blocking(move || -> Result<Vec<Project>> {
            let conn = open_conn(&path)?;
            let mut stmt = conn.prepare(
                "SELECT id, name, description, created_at, updated_at FROM projects ORDER BY created_at",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(row_to_project(row).unwrap())
            })?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
        .await?
    }

    async fn update(&self, project: &Project) -> Result<()> {
        let path = self.db_path();
        let p = project.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute(
                "UPDATE projects SET name=?1, description=?2, updated_at=?3 WHERE id=?4",
                rusqlite::params![p.name, p.description, p.updated_at as i64, p.id.as_str()],
            )
            .context("update project")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let path = self.db_path();
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = open_conn(&path)?;
            conn.execute("DELETE FROM projects WHERE id=?1", [id.as_str()])
                .context("delete project")?;
            Ok(())
        })
        .await?
    }
}

fn row_to_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: Id::new(row.get::<_, String>(0)?),
        name: row.get(1)?,
        description: row.get(2)?,
        created_at: row.get::<_, i64>(3)? as u64,
        updated_at: row.get::<_, i64>(4)? as u64,
    })
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

    fn make_project(id: &str, name: &str) -> Project {
        Project::new(Id::new(id), name, 1000)
    }

    #[tokio::test]
    async fn create_and_find() {
        let (_tmp, s) = setup();
        let p = make_project("p1", "Alpha");
        s.create(&p).await.unwrap();
        let found = s.find_by_id(&p.id).await.unwrap().unwrap();
        assert_eq!(found.name, "Alpha");
        assert_eq!(found.id, p.id);
    }

    #[tokio::test]
    async fn find_missing_returns_none() {
        let (_tmp, s) = setup();
        let result = s.find_by_id(&Id::new("no-such")).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn list_projects() {
        let (_tmp, s) = setup();
        s.create(&make_project("p1", "Alpha")).await.unwrap();
        s.create(&make_project("p2", "Beta")).await.unwrap();
        let all = s.list().await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn update_project() {
        let (_tmp, s) = setup();
        let mut p = make_project("p1", "Alpha");
        s.create(&p).await.unwrap();
        p.name = "Renamed".to_string();
        p.description = Some("desc".to_string());
        p.updated_at = 2000;
        s.update(&p).await.unwrap();
        let found = s.find_by_id(&p.id).await.unwrap().unwrap();
        assert_eq!(found.name, "Renamed");
        assert_eq!(found.description.as_deref(), Some("desc"));
        assert_eq!(found.updated_at, 2000);
    }

    #[tokio::test]
    async fn delete_project() {
        let (_tmp, s) = setup();
        let p = make_project("p1", "Alpha");
        s.create(&p).await.unwrap();
        s.delete(&p.id).await.unwrap();
        assert!(s.find_by_id(&p.id).await.unwrap().is_none());
    }
}
