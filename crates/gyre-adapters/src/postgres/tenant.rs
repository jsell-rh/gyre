use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{BudgetConfig, Tenant};
use gyre_ports::TenantRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::tenants;

#[derive(Queryable, Selectable)]
#[diesel(table_name = tenants)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct TenantRow {
    id: String,
    name: String,
    slug: String,
    oidc_issuer: Option<String>,
    budget: Option<String>,
    max_workspaces: Option<i32>,
    created_at: i64,
}

impl TenantRow {
    fn into_tenant(self) -> Result<Tenant> {
        let budget: Option<BudgetConfig> = self
            .budget
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;
        Ok(Tenant {
            id: Id::new(self.id),
            name: self.name,
            slug: self.slug,
            oidc_issuer: self.oidc_issuer,
            budget,
            max_workspaces: self.max_workspaces.map(|v| v as u32),
            created_at: self.created_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = tenants)]
struct NewTenantRow<'a> {
    id: &'a str,
    name: &'a str,
    slug: &'a str,
    oidc_issuer: Option<&'a str>,
    budget: Option<String>,
    max_workspaces: Option<i32>,
    created_at: i64,
}

#[async_trait]
impl TenantRepository for PgStorage {
    async fn create(&self, tenant: &Tenant) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let t = tenant.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let budget_json = t.budget.as_ref().map(serde_json::to_string).transpose()?;
            let row = NewTenantRow {
                id: t.id.as_str(),
                name: &t.name,
                slug: &t.slug,
                oidc_issuer: t.oidc_issuer.as_deref(),
                budget: budget_json,
                max_workspaces: t.max_workspaces.map(|v| v as i32),
                created_at: t.created_at as i64,
            };
            diesel::insert_into(tenants::table)
                .values(&row)
                .on_conflict(tenants::id)
                .do_update()
                .set((
                    tenants::name.eq(&t.name),
                    tenants::slug.eq(&t.slug),
                    tenants::oidc_issuer.eq(t.oidc_issuer.as_deref()),
                    tenants::budget.eq(row.budget.as_deref()),
                    tenants::max_workspaces.eq(row.max_workspaces),
                ))
                .execute(&mut *conn)
                .context("insert tenant")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<Tenant>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<Tenant>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = tenants::table
                .find(id.as_str())
                .first::<TenantRow>(&mut *conn)
                .optional()
                .context("find tenant by id")?;
            result.map(TenantRow::into_tenant).transpose()
        })
        .await?
    }

    async fn find_by_slug(&self, slug: &str) -> Result<Option<Tenant>> {
        let pool = Arc::clone(&self.pool);
        let slug = slug.to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<Tenant>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = tenants::table
                .filter(tenants::slug.eq(&slug))
                .first::<TenantRow>(&mut *conn)
                .optional()
                .context("find tenant by slug")?;
            result.map(TenantRow::into_tenant).transpose()
        })
        .await?
    }

    async fn list(&self) -> Result<Vec<Tenant>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<Tenant>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = tenants::table
                .order(tenants::created_at.asc())
                .load::<TenantRow>(&mut *conn)
                .context("list tenants")?;
            rows.into_iter().map(TenantRow::into_tenant).collect()
        })
        .await?
    }

    async fn update(&self, tenant: &Tenant) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let t = tenant.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let budget_json = t.budget.as_ref().map(serde_json::to_string).transpose()?;
            diesel::update(tenants::table.find(t.id.as_str()))
                .set((
                    tenants::name.eq(&t.name),
                    tenants::slug.eq(&t.slug),
                    tenants::oidc_issuer.eq(t.oidc_issuer.as_deref()),
                    tenants::budget.eq(budget_json.as_deref()),
                    tenants::max_workspaces.eq(t.max_workspaces.map(|v| v as i32)),
                ))
                .execute(&mut *conn)
                .context("update tenant")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(tenants::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete tenant")?;
            Ok(())
        })
        .await?
    }
}
