use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{BudgetConfig, Persona, PersonaApprovalStatus, PersonaScope, Workspace};
use gyre_ports::{PersonaRepository, WorkspaceRepository};
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::{personas, workspaces};

// ---------------------------------------------------------------------------
// Workspace
// ---------------------------------------------------------------------------

#[derive(Queryable, Selectable)]
#[diesel(table_name = workspaces)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct WorkspaceRow {
    id: String,
    tenant_id: String,
    name: String,
    slug: String,
    description: Option<String>,
    budget: Option<String>,
    max_repos: Option<i32>,
    max_agents_per_repo: Option<i32>,
    created_at: i64,
}

impl WorkspaceRow {
    fn into_workspace(self) -> Result<Workspace> {
        let budget: Option<BudgetConfig> = self
            .budget
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;
        Ok(Workspace {
            id: Id::new(self.id),
            tenant_id: Id::new(self.tenant_id),
            name: self.name,
            slug: self.slug,
            description: self.description,
            budget,
            max_repos: self.max_repos.map(|v| v as u32),
            max_agents_per_repo: self.max_agents_per_repo.map(|v| v as u32),
            created_at: self.created_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = workspaces)]
struct NewWorkspaceRow<'a> {
    id: &'a str,
    tenant_id: &'a str,
    name: &'a str,
    slug: &'a str,
    description: Option<&'a str>,
    budget: Option<String>,
    max_repos: Option<i32>,
    max_agents_per_repo: Option<i32>,
    created_at: i64,
}

#[async_trait]
impl WorkspaceRepository for SqliteStorage {
    async fn create(&self, workspace: &Workspace) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let w = workspace.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let budget_json = w.budget.as_ref().map(serde_json::to_string).transpose()?;
            let row = NewWorkspaceRow {
                id: w.id.as_str(),
                tenant_id: w.tenant_id.as_str(),
                name: &w.name,
                slug: &w.slug,
                description: w.description.as_deref(),
                budget: budget_json,
                max_repos: w.max_repos.map(|v| v as i32),
                max_agents_per_repo: w.max_agents_per_repo.map(|v| v as i32),
                created_at: w.created_at as i64,
            };
            diesel::insert_into(workspaces::table)
                .values(&row)
                .on_conflict(workspaces::id)
                .do_update()
                .set((
                    workspaces::name.eq(&w.name),
                    workspaces::slug.eq(&w.slug),
                    workspaces::description.eq(w.description.as_deref()),
                    workspaces::budget.eq(row.budget.as_deref()),
                    workspaces::max_repos.eq(row.max_repos),
                    workspaces::max_agents_per_repo.eq(row.max_agents_per_repo),
                ))
                .execute(&mut *conn)
                .context("insert workspace")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_slug(&self, tenant_id: &Id, slug: &str) -> Result<Option<Workspace>> {
        let pool = Arc::clone(&self.pool);
        let tid = tenant_id.clone();
        let slug = slug.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Workspace>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = workspaces::table
                .filter(workspaces::tenant_id.eq(tid.as_str()))
                .filter(workspaces::slug.eq(&slug))
                .first::<WorkspaceRow>(&mut *conn)
                .optional()
                .context("find workspace by slug")?;
            result.map(WorkspaceRow::into_workspace).transpose()
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Workspace>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Workspace>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = workspaces::table
                .find(id.as_str())
                .first::<WorkspaceRow>(&mut *conn)
                .optional()
                .context("find workspace by id")?;
            result.map(WorkspaceRow::into_workspace).transpose()
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<Workspace>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<Workspace>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = workspaces::table
                .order(workspaces::created_at.asc())
                .load::<WorkspaceRow>(&mut *conn)
                .context("list workspaces")?;
            rows.into_iter().map(WorkspaceRow::into_workspace).collect()
        })
        .await?
    }

    async fn list_by_tenant(&self, tenant_id: &Id) -> Result<Vec<Workspace>> {
        let pool = Arc::clone(&self.pool);
        let tid = tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Workspace>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = workspaces::table
                .filter(workspaces::tenant_id.eq(tid.as_str()))
                .order(workspaces::created_at.asc())
                .load::<WorkspaceRow>(&mut *conn)
                .context("list workspaces by tenant")?;
            rows.into_iter().map(WorkspaceRow::into_workspace).collect()
        })
        .await?
    }

    async fn update(&self, workspace: &Workspace) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let w = workspace.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let budget_json = w.budget.as_ref().map(serde_json::to_string).transpose()?;
            diesel::update(workspaces::table.find(w.id.as_str()))
                .set((
                    workspaces::name.eq(&w.name),
                    workspaces::slug.eq(&w.slug),
                    workspaces::description.eq(w.description.as_deref()),
                    workspaces::budget.eq(budget_json.as_deref()),
                    workspaces::max_repos.eq(w.max_repos.map(|v| v as i32)),
                    workspaces::max_agents_per_repo.eq(w.max_agents_per_repo.map(|v| v as i32)),
                ))
                .execute(&mut *conn)
                .context("update workspace")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(workspaces::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete workspace")?;
            Ok(())
        })
        .await?
    }
}

// ---------------------------------------------------------------------------
// Persona
// ---------------------------------------------------------------------------

#[derive(Queryable, Selectable)]
#[diesel(table_name = personas)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct PersonaRow {
    id: String,
    name: String,
    slug: String,
    scope: String,
    system_prompt: String,
    capabilities: String,
    protocols: String,
    model: Option<String>,
    temperature: Option<f64>,
    max_tokens: Option<i32>,
    budget: Option<String>,
    created_at: i64,
    version: i32,
    content_hash: String,
    owner: Option<String>,
    approval_status: String,
    approved_by: Option<String>,
    approved_at: Option<i64>,
    updated_at: i64,
}

impl PersonaRow {
    fn into_persona(self) -> Result<Persona> {
        let scope: PersonaScope = serde_json::from_str(&self.scope)?;
        let capabilities: Vec<String> =
            serde_json::from_str(&self.capabilities).unwrap_or_default();
        let protocols: Vec<String> = serde_json::from_str(&self.protocols).unwrap_or_default();
        let budget: Option<BudgetConfig> = self
            .budget
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;
        let approval_status: PersonaApprovalStatus =
            serde_json::from_str(&format!("\"{}\"", self.approval_status)).unwrap_or_default();
        Ok(Persona {
            id: Id::new(self.id),
            name: self.name,
            slug: self.slug,
            scope,
            system_prompt: self.system_prompt,
            capabilities,
            protocols,
            model: self.model,
            temperature: self.temperature,
            max_tokens: self.max_tokens.map(|v| v as u32),
            budget,
            created_at: self.created_at as u64,
            version: self.version as u32,
            content_hash: self.content_hash,
            owner: self.owner,
            approval_status,
            approved_by: self.approved_by,
            approved_at: self.approved_at.map(|v| v as u64),
            updated_at: self.updated_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = personas)]
struct NewPersonaRow {
    id: String,
    name: String,
    slug: String,
    scope: String,
    system_prompt: String,
    capabilities: String,
    protocols: String,
    model: Option<String>,
    temperature: Option<f64>,
    max_tokens: Option<i32>,
    budget: Option<String>,
    created_at: i64,
    version: i32,
    content_hash: String,
    owner: Option<String>,
    approval_status: String,
    approved_by: Option<String>,
    approved_at: Option<i64>,
    updated_at: i64,
}

#[async_trait]
impl PersonaRepository for SqliteStorage {
    async fn create(&self, persona: &Persona) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let p = persona.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let approval_status = serde_json::to_string(&p.approval_status)?
                .trim_matches('"')
                .to_string();
            let row = NewPersonaRow {
                id: p.id.as_str().to_string(),
                name: p.name.clone(),
                slug: p.slug.clone(),
                scope: serde_json::to_string(&p.scope)?,
                system_prompt: p.system_prompt.clone(),
                capabilities: serde_json::to_string(&p.capabilities)?,
                protocols: serde_json::to_string(&p.protocols)?,
                model: p.model.clone(),
                temperature: p.temperature,
                max_tokens: p.max_tokens.map(|v| v as i32),
                budget: p.budget.as_ref().map(serde_json::to_string).transpose()?,
                created_at: p.created_at as i64,
                version: p.version as i32,
                content_hash: p.content_hash.clone(),
                owner: p.owner.clone(),
                approval_status,
                approved_by: p.approved_by.clone(),
                approved_at: p.approved_at.map(|v| v as i64),
                updated_at: p.updated_at as i64,
            };
            diesel::insert_into(personas::table)
                .values(&row)
                .on_conflict(personas::id)
                .do_update()
                .set((
                    personas::name.eq(&row.name),
                    personas::slug.eq(&row.slug),
                    personas::scope.eq(&row.scope),
                    personas::system_prompt.eq(&row.system_prompt),
                    personas::capabilities.eq(&row.capabilities),
                    personas::protocols.eq(&row.protocols),
                    personas::model.eq(&row.model),
                    personas::temperature.eq(row.temperature),
                    personas::max_tokens.eq(row.max_tokens),
                    personas::budget.eq(&row.budget),
                    personas::version.eq(row.version),
                    personas::content_hash.eq(&row.content_hash),
                    personas::owner.eq(&row.owner),
                    personas::approval_status.eq(&row.approval_status),
                    personas::approved_by.eq(&row.approved_by),
                    personas::approved_at.eq(row.approved_at),
                    personas::updated_at.eq(row.updated_at),
                ))
                .execute(&mut *conn)
                .context("insert persona")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Persona>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Persona>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = personas::table
                .find(id.as_str())
                .first::<PersonaRow>(&mut *conn)
                .optional()
                .context("find persona by id")?;
            result.map(PersonaRow::into_persona).transpose()
        })
        .await?
    }

    async fn find_by_slug_and_scope(
        &self,
        slug: &str,
        scope: &PersonaScope,
    ) -> Result<Option<Persona>> {
        let pool = Arc::clone(&self.pool);
        let slug = slug.to_string();
        let scope_json = serde_json::to_string(scope)?;
        tokio::task::spawn_blocking(move || -> Result<Option<Persona>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = personas::table
                .filter(personas::slug.eq(&slug))
                .filter(personas::scope.eq(&scope_json))
                .first::<PersonaRow>(&mut *conn)
                .optional()
                .context("find persona by slug+scope")?;
            result.map(PersonaRow::into_persona).transpose()
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<Persona>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<Persona>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = personas::table
                .order(personas::created_at.asc())
                .load::<PersonaRow>(&mut *conn)
                .context("list personas")?;
            rows.into_iter().map(PersonaRow::into_persona).collect()
        })
        .await?
    }

    async fn list_by_scope(&self, scope: &PersonaScope) -> Result<Vec<Persona>> {
        let pool = Arc::clone(&self.pool);
        let scope_json = serde_json::to_string(scope)?;
        tokio::task::spawn_blocking(move || -> Result<Vec<Persona>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = personas::table
                .filter(personas::scope.eq(&scope_json))
                .order(personas::created_at.asc())
                .load::<PersonaRow>(&mut *conn)
                .context("list personas by scope")?;
            rows.into_iter().map(PersonaRow::into_persona).collect()
        })
        .await?
    }

    async fn update(&self, persona: &Persona) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let p = persona.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let approval_status = serde_json::to_string(&p.approval_status)?
                .trim_matches('"')
                .to_string();
            diesel::update(personas::table.find(p.id.as_str()))
                .set((
                    personas::name.eq(&p.name),
                    personas::slug.eq(&p.slug),
                    personas::scope.eq(serde_json::to_string(&p.scope)?),
                    personas::system_prompt.eq(&p.system_prompt),
                    personas::capabilities.eq(serde_json::to_string(&p.capabilities)?),
                    personas::protocols.eq(serde_json::to_string(&p.protocols)?),
                    personas::model.eq(&p.model),
                    personas::temperature.eq(p.temperature),
                    personas::max_tokens.eq(p.max_tokens.map(|v| v as i32)),
                    personas::budget.eq(p
                        .budget
                        .as_ref()
                        .map(serde_json::to_string)
                        .transpose()?
                        .as_deref()
                        .map(String::from)),
                    personas::version.eq(p.version as i32),
                    personas::content_hash.eq(&p.content_hash),
                    personas::owner.eq(&p.owner),
                    personas::approval_status.eq(&approval_status),
                    personas::approved_by.eq(&p.approved_by),
                    personas::approved_at.eq(p.approved_at.map(|v| v as i64)),
                    personas::updated_at.eq(p.updated_at as i64),
                ))
                .execute(&mut *conn)
                .context("update persona")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(personas::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete persona")?;
            Ok(())
        })
        .await?
    }
}
