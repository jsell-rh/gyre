use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::Project;
use gyre_ports::ProjectRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::projects;

#[derive(Queryable, Selectable)]
#[diesel(table_name = projects)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct ProjectRow {
    id: String,
    name: String,
    description: Option<String>,
    created_at: i64,
    updated_at: i64,
}

impl From<ProjectRow> for Project {
    fn from(r: ProjectRow) -> Self {
        Project {
            id: Id::new(r.id),
            name: r.name,
            description: r.description,
            created_at: r.created_at as u64,
            updated_at: r.updated_at as u64,
        }
    }
}

#[derive(Insertable, AsChangeset)]
#[diesel(table_name = projects)]
struct ProjectRecord<'a> {
    id: &'a str,
    name: &'a str,
    description: Option<&'a str>,
    created_at: i64,
    updated_at: i64,
}

impl<'a> From<&'a Project> for ProjectRecord<'a> {
    fn from(p: &'a Project) -> Self {
        ProjectRecord {
            id: p.id.as_str(),
            name: &p.name,
            description: p.description.as_deref(),
            created_at: p.created_at as i64,
            updated_at: p.updated_at as i64,
        }
    }
}

#[async_trait]
impl ProjectRepository for SqliteStorage {
    async fn create(&self, project: &Project) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let p = project.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = ProjectRecord::from(&p);
            diesel::insert_into(projects::table)
                .values(&record)
                .on_conflict(projects::id)
                .do_update()
                .set(&record)
                .execute(&mut *conn)
                .context("insert project")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Project>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Project>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = projects::table
                .find(id.as_str())
                .first::<ProjectRow>(&mut *conn)
                .optional()
                .context("find project by id")?;
            Ok(result.map(Project::from))
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<Project>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<Project>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = projects::table
                .order(projects::created_at.asc())
                .load::<ProjectRow>(&mut *conn)
                .context("list projects")?;
            Ok(rows.into_iter().map(Project::from).collect())
        })
        .await?
    }

    async fn update(&self, project: &Project) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let p = project.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(projects::table.find(p.id.as_str()))
                .set((
                    projects::name.eq(&p.name),
                    projects::description.eq(p.description.as_deref()),
                    projects::updated_at.eq(p.updated_at as i64),
                ))
                .execute(&mut *conn)
                .context("update project")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(projects::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete project")?;
            Ok(())
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
