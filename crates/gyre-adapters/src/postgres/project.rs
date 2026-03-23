use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::Project;
use gyre_ports::ProjectRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::projects;

#[derive(Queryable, Selectable)]
#[diesel(table_name = projects)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct ProjectRow {
    id: String,
    name: String,
    description: Option<String>,
    created_at: i64,
    updated_at: i64,
    #[allow(dead_code)]
    tenant_id: String,
    workspace_id: Option<String>,
}

impl From<ProjectRow> for Project {
    fn from(r: ProjectRow) -> Self {
        Project {
            id: Id::new(r.id),
            name: r.name,
            description: r.description,
            created_at: r.created_at as u64,
            updated_at: r.updated_at as u64,
            workspace_id: r.workspace_id.map(Id::new),
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
    tenant_id: &'a str,
    workspace_id: Option<&'a str>,
}

#[async_trait]
impl ProjectRepository for PgStorage {
    async fn create(&self, project: &Project) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let p = project.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let record = ProjectRecord {
                id: p.id.as_str(),
                name: &p.name,
                description: p.description.as_deref(),
                created_at: p.created_at as i64,
                updated_at: p.updated_at as i64,
                tenant_id: &tenant,
                workspace_id: p.workspace_id.as_ref().map(|id| id.as_str()),
            };
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
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Project>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = projects::table
                .find(id.as_str())
                .filter(projects::tenant_id.eq(&tenant))
                .first::<ProjectRow>(&mut *conn)
                .optional()
                .context("find project by id")?;
            Ok(result.map(Project::from))
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<Project>> {
        let pool = Arc::clone(&self.pool);
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Project>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = projects::table
                .filter(projects::tenant_id.eq(&tenant))
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
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(
                projects::table
                    .find(p.id.as_str())
                    .filter(projects::tenant_id.eq(&tenant)),
            )
            .set((
                projects::name.eq(&p.name),
                projects::description.eq(p.description.as_deref()),
                projects::updated_at.eq(p.updated_at as i64),
                projects::workspace_id.eq(p.workspace_id.as_ref().map(|id| id.as_str())),
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
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(
                projects::table
                    .find(id.as_str())
                    .filter(projects::tenant_id.eq(&tenant)),
            )
            .execute(&mut *conn)
            .context("delete project")?;
            Ok(())
        })
        .await?
    }
    async fn list_by_workspace(&self, workspace_id: &Id) -> Result<Vec<Project>> {
        let pool = Arc::clone(&self.pool);
        let workspace_id = workspace_id.clone();
        let tenant = self.tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Project>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = projects::table
                .filter(projects::tenant_id.eq(&tenant))
                .filter(projects::workspace_id.eq(workspace_id.as_str()))
                .order(projects::created_at.asc())
                .load::<ProjectRow>(&mut *conn)
                .context("list projects by workspace")?;
            Ok(rows.into_iter().map(Project::from).collect())
        })
        .await?
    }
}
