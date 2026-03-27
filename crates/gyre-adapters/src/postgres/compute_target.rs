use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{ComputeTargetEntity, ComputeTargetType};
use gyre_ports::ComputeTargetRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::{compute_targets, workspaces};

// ── Row types ─────────────────────────────────────────────────────────────────

#[derive(Queryable, Selectable, Clone)]
#[diesel(table_name = compute_targets)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct ComputeTargetRow {
    id: String,
    tenant_id: String,
    name: String,
    target_type: String,
    config: String,
    is_default: i32,
    created_at: i64,
    updated_at: i64,
}

impl ComputeTargetRow {
    fn into_domain(self) -> Result<ComputeTargetEntity> {
        let target_type = ComputeTargetType::from_db_str(&self.target_type)
            .ok_or_else(|| anyhow::anyhow!("unknown target_type: {}", self.target_type))?;
        let config: serde_json::Value = serde_json::from_str(&self.config)
            .unwrap_or(serde_json::Value::Object(Default::default()));
        Ok(ComputeTargetEntity {
            id: Id::new(self.id),
            tenant_id: Id::new(self.tenant_id),
            name: self.name,
            target_type,
            config,
            is_default: self.is_default != 0,
            created_at: self.created_at as u64,
            updated_at: self.updated_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = compute_targets)]
struct InsertComputeTargetRow<'a> {
    id: &'a str,
    tenant_id: &'a str,
    name: &'a str,
    target_type: &'a str,
    config: String,
    is_default: i32,
    created_at: i64,
    updated_at: i64,
}

// ── Repository impl ───────────────────────────────────────────────────────────

#[async_trait]
impl ComputeTargetRepository for PgStorage {
    async fn create(&self, target: &ComputeTargetEntity) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let t = target.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let config_json = serde_json::to_string(&t.config).context("serialize config")?;
            let target_type = t.target_type.to_string();
            let row = InsertComputeTargetRow {
                id: t.id.as_str(),
                tenant_id: t.tenant_id.as_str(),
                name: &t.name,
                target_type: &target_type,
                config: config_json,
                is_default: if t.is_default { 1 } else { 0 },
                created_at: t.created_at as i64,
                updated_at: t.updated_at as i64,
            };
            diesel::insert_into(compute_targets::table)
                .values(&row)
                .execute(&mut *conn)
                .context("insert compute target")?;
            Ok(())
        })
        .await?
    }

    async fn get_by_id(&self, id: &Id) -> Result<Option<ComputeTargetEntity>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<ComputeTargetEntity>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = compute_targets::table
                .find(id.as_str())
                .first::<ComputeTargetRow>(&mut *conn)
                .optional()
                .context("get compute target by id")?;
            result.map(ComputeTargetRow::into_domain).transpose()
        })
        .await?
    }

    async fn list_by_tenant(&self, tenant_id: &Id) -> Result<Vec<ComputeTargetEntity>> {
        let pool = Arc::clone(&self.pool);
        let tid = tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<ComputeTargetEntity>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = compute_targets::table
                .filter(compute_targets::tenant_id.eq(tid.as_str()))
                .order(compute_targets::name.asc())
                .load::<ComputeTargetRow>(&mut *conn)
                .context("list compute targets by tenant")?;
            rows.into_iter()
                .map(ComputeTargetRow::into_domain)
                .collect()
        })
        .await?
    }

    async fn update(&self, target: &ComputeTargetEntity) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let t = target.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let config_json = serde_json::to_string(&t.config).context("serialize config")?;
            let target_type = t.target_type.to_string();
            diesel::update(compute_targets::table.find(t.id.as_str()))
                .set((
                    compute_targets::name.eq(&t.name),
                    compute_targets::target_type.eq(&target_type),
                    compute_targets::config.eq(&config_json),
                    compute_targets::is_default.eq(if t.is_default { 1 } else { 0 }),
                    compute_targets::updated_at.eq(t.updated_at as i64),
                ))
                .execute(&mut *conn)
                .context("update compute target")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(compute_targets::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete compute target")?;
            Ok(())
        })
        .await?
    }

    async fn get_default_for_tenant(&self, tenant_id: &Id) -> Result<Option<ComputeTargetEntity>> {
        let pool = Arc::clone(&self.pool);
        let tid = tenant_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<ComputeTargetEntity>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = compute_targets::table
                .filter(compute_targets::tenant_id.eq(tid.as_str()))
                .filter(compute_targets::is_default.eq(1))
                .first::<ComputeTargetRow>(&mut *conn)
                .optional()
                .context("get default compute target")?;
            result.map(ComputeTargetRow::into_domain).transpose()
        })
        .await?
    }

    async fn has_workspace_references(&self, id: &Id) -> Result<bool> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            let mut conn = pool.get().context("get db connection")?;
            let count: i64 = workspaces::table
                .filter(workspaces::compute_target_id.eq(id.as_str()))
                .count()
                .get_result(&mut *conn)
                .context("count workspace references")?;
            Ok(count > 0)
        })
        .await?
    }
}
