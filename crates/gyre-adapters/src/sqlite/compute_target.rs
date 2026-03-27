use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{ComputeTargetEntity, ComputeTargetType};
use gyre_ports::ComputeTargetRepository;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::{compute_targets, workspaces};

// ── Row types ─────────────────────────────────────────────────────────────────

#[derive(Queryable, Selectable, Clone)]
#[diesel(table_name = compute_targets)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
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
impl ComputeTargetRepository for SqliteStorage {
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use gyre_domain::ComputeTargetType;

    fn storage() -> SqliteStorage {
        SqliteStorage::new(":memory:").expect("open in-memory db")
    }

    fn tenant_id() -> Id {
        Id::new(uuid::Uuid::new_v4().to_string())
    }

    fn make_target(tenant_id: &Id, name: &str, ty: ComputeTargetType) -> ComputeTargetEntity {
        ComputeTargetEntity::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            tenant_id.clone(),
            name,
            ty,
            1000,
        )
    }

    #[tokio::test]
    async fn create_and_get_round_trip() {
        let s = storage();
        let tid = tenant_id();
        let ct = make_target(&tid, "my-container", ComputeTargetType::Container);
        let id = ct.id.clone();
        s.create(&ct).await.expect("create");
        let got = s.get_by_id(&id).await.expect("get").expect("should exist");
        assert_eq!(got.name, "my-container");
        assert_eq!(got.target_type, ComputeTargetType::Container);
        assert!(!got.is_default);
    }

    #[tokio::test]
    async fn list_by_tenant_returns_only_tenant_targets() {
        let s = storage();
        let t1 = tenant_id();
        let t2 = tenant_id();
        s.create(&make_target(&t1, "a", ComputeTargetType::Ssh))
            .await
            .expect("create a");
        s.create(&make_target(&t1, "b", ComputeTargetType::Kubernetes))
            .await
            .expect("create b");
        s.create(&make_target(&t2, "c", ComputeTargetType::Container))
            .await
            .expect("create c");

        let list = s.list_by_tenant(&t1).await.expect("list");
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|ct| ct.tenant_id == t1));
    }

    #[tokio::test]
    async fn update_changes_fields() {
        let s = storage();
        let tid = tenant_id();
        let mut ct = make_target(&tid, "orig", ComputeTargetType::Ssh);
        s.create(&ct).await.expect("create");
        ct.name = "renamed".to_string();
        ct.is_default = true;
        ct.updated_at = 2000;
        s.update(&ct).await.expect("update");
        let got = s.get_by_id(&ct.id).await.expect("get").expect("exists");
        assert_eq!(got.name, "renamed");
        assert!(got.is_default);
    }

    #[tokio::test]
    async fn delete_removes_target() {
        let s = storage();
        let tid = tenant_id();
        let ct = make_target(&tid, "del-me", ComputeTargetType::Container);
        let id = ct.id.clone();
        s.create(&ct).await.expect("create");
        s.delete(&id).await.expect("delete");
        assert!(s.get_by_id(&id).await.expect("get").is_none());
    }

    #[tokio::test]
    async fn get_default_for_tenant() {
        let s = storage();
        let tid = tenant_id();
        let mut ct = make_target(&tid, "def", ComputeTargetType::Kubernetes);
        ct.is_default = true;
        s.create(&ct).await.expect("create");
        s.create(&make_target(&tid, "non-def", ComputeTargetType::Ssh))
            .await
            .expect("create non-default");
        let def = s
            .get_default_for_tenant(&tid)
            .await
            .expect("get default")
            .expect("should have default");
        assert_eq!(def.name, "def");
    }

    #[tokio::test]
    async fn get_default_returns_none_when_none_set() {
        let s = storage();
        let tid = tenant_id();
        s.create(&make_target(&tid, "no-default", ComputeTargetType::Ssh))
            .await
            .expect("create");
        let def = s.get_default_for_tenant(&tid).await.expect("get default");
        assert!(def.is_none());
    }
}
