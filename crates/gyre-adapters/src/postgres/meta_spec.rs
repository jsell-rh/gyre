//! PostgreSQL adapter for the meta-spec registry (agent-runtime spec §2).

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::meta_spec::{
    MetaSpec, MetaSpecApprovalStatus, MetaSpecBinding, MetaSpecKind, MetaSpecScope, MetaSpecVersion,
};
use gyre_ports::meta_spec_repository::{
    MetaSpecBindingRepository, MetaSpecFilter, MetaSpecRepository,
};
use std::sync::Arc;

use super::PgStorage;
use crate::schema::{meta_spec_bindings, meta_spec_versions, meta_specs};

// ---------------------------------------------------------------------------
// Row types
// ---------------------------------------------------------------------------

#[derive(Queryable, Selectable, Clone)]
#[diesel(table_name = meta_specs)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct MetaSpecRow {
    id: String,
    kind: String,
    name: String,
    scope: String,
    scope_id: Option<String>,
    prompt: String,
    version: i32,
    content_hash: String,
    required: i32,
    approval_status: String,
    approved_by: Option<String>,
    approved_at: Option<i64>,
    created_by: String,
    created_at: i64,
    updated_at: i64,
}

impl MetaSpecRow {
    fn into_domain(self) -> Result<MetaSpec> {
        Ok(MetaSpec {
            id: Id::new(self.id),
            kind: MetaSpecKind::from_str(&self.kind)
                .ok_or_else(|| anyhow!("unknown meta_spec kind: {}", self.kind))?,
            name: self.name,
            scope: MetaSpecScope::from_str(&self.scope)
                .ok_or_else(|| anyhow!("unknown meta_spec scope: {}", self.scope))?,
            scope_id: self.scope_id,
            prompt: self.prompt,
            version: self.version as u32,
            content_hash: self.content_hash,
            required: self.required != 0,
            approval_status: MetaSpecApprovalStatus::from_str(&self.approval_status)
                .ok_or_else(|| anyhow!("unknown approval_status: {}", self.approval_status))?,
            approved_by: self.approved_by,
            approved_at: self.approved_at.map(|t| t as u64),
            created_by: self.created_by,
            created_at: self.created_at as u64,
            updated_at: self.updated_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = meta_specs)]
struct InsertMetaSpecRow<'a> {
    id: &'a str,
    kind: &'a str,
    name: &'a str,
    scope: &'a str,
    scope_id: Option<&'a str>,
    prompt: &'a str,
    version: i32,
    content_hash: &'a str,
    required: i32,
    approval_status: &'a str,
    approved_by: Option<&'a str>,
    approved_at: Option<i64>,
    created_by: &'a str,
    created_at: i64,
    updated_at: i64,
}

#[derive(Queryable, Selectable, Clone)]
#[diesel(table_name = meta_spec_versions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct MetaSpecVersionRow {
    id: String,
    meta_spec_id: String,
    version: i32,
    prompt: String,
    content_hash: String,
    created_at: i64,
}

impl MetaSpecVersionRow {
    fn into_domain(self) -> MetaSpecVersion {
        MetaSpecVersion {
            id: Id::new(self.id),
            meta_spec_id: Id::new(self.meta_spec_id),
            version: self.version as u32,
            prompt: self.prompt,
            content_hash: self.content_hash,
            created_at: self.created_at as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = meta_spec_versions)]
struct InsertVersionRow<'a> {
    id: &'a str,
    meta_spec_id: &'a str,
    version: i32,
    prompt: &'a str,
    content_hash: &'a str,
    created_at: i64,
}

#[derive(Queryable, Selectable, Clone)]
#[diesel(table_name = meta_spec_bindings)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct MetaSpecBindingRow {
    id: String,
    spec_id: String,
    meta_spec_id: String,
    pinned_version: i32,
    created_at: i64,
}

impl MetaSpecBindingRow {
    fn into_domain(self) -> MetaSpecBinding {
        MetaSpecBinding {
            id: Id::new(self.id),
            spec_id: self.spec_id,
            meta_spec_id: Id::new(self.meta_spec_id),
            pinned_version: self.pinned_version as u32,
            created_at: self.created_at as u64,
        }
    }
}

#[derive(Insertable)]
#[diesel(table_name = meta_spec_bindings)]
struct InsertBindingRow<'a> {
    id: &'a str,
    spec_id: &'a str,
    meta_spec_id: &'a str,
    pinned_version: i32,
    created_at: i64,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

// ---------------------------------------------------------------------------
// MetaSpecRepository impl
// ---------------------------------------------------------------------------

#[async_trait]
impl MetaSpecRepository for PgStorage {
    async fn create(&self, meta_spec: &MetaSpec) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let row_id = meta_spec.id.as_str().to_string();
        let kind = meta_spec.kind.as_str().to_string();
        let name = meta_spec.name.clone();
        let scope = meta_spec.scope.as_str().to_string();
        let scope_id = meta_spec.scope_id.clone();
        let prompt = meta_spec.prompt.clone();
        let version = meta_spec.version as i32;
        let content_hash = meta_spec.content_hash.clone();
        let required = meta_spec.required as i32;
        let approval_status = meta_spec.approval_status.as_str().to_string();
        let approved_by = meta_spec.approved_by.clone();
        let approved_at = meta_spec.approved_at.map(|t| t as i64);
        let created_by = meta_spec.created_by.clone();
        let created_at = meta_spec.created_at as i64;
        let updated_at = meta_spec.updated_at as i64;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = InsertMetaSpecRow {
                id: &row_id,
                kind: &kind,
                name: &name,
                scope: &scope,
                scope_id: scope_id.as_deref(),
                prompt: &prompt,
                version,
                content_hash: &content_hash,
                required,
                approval_status: &approval_status,
                approved_by: approved_by.as_deref(),
                approved_at,
                created_by: &created_by,
                created_at,
                updated_at,
            };
            diesel::insert_into(meta_specs::table)
                .values(&row)
                .execute(&mut *conn)
                .context("insert meta_spec")?;
            Ok(())
        })
        .await?
    }

    async fn get_by_id(&self, id: &Id) -> Result<Option<MetaSpec>> {
        let pool = Arc::clone(&self.pool);
        let id_str = id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Option<MetaSpec>> {
            let mut conn = pool.get().context("get db connection")?;
            let row = meta_specs::table
                .filter(meta_specs::id.eq(&id_str))
                .first::<MetaSpecRow>(&mut *conn)
                .optional()
                .context("get meta_spec by id")?;
            row.map(|r| r.into_domain()).transpose()
        })
        .await?
    }

    async fn list(&self, filter: &MetaSpecFilter) -> Result<Vec<MetaSpec>> {
        let pool = Arc::clone(&self.pool);
        let scope_filter = filter.scope.as_ref().map(|s| s.as_str().to_string());
        let scope_id_filter = filter.scope_id.clone();
        let kind_filter = filter.kind.as_ref().map(|k| k.as_str().to_string());
        let required_filter = filter.required;
        tokio::task::spawn_blocking(move || -> Result<Vec<MetaSpec>> {
            let mut conn = pool.get().context("get db connection")?;
            let mut query = meta_specs::table.into_boxed();
            if let Some(scope) = &scope_filter {
                query = query.filter(meta_specs::scope.eq(scope));
            }
            if let Some(scope_id) = &scope_id_filter {
                query = query.filter(meta_specs::scope_id.eq(scope_id));
            }
            if let Some(kind) = &kind_filter {
                query = query.filter(meta_specs::kind.eq(kind));
            }
            if let Some(required) = required_filter {
                query = query.filter(meta_specs::required.eq(required as i32));
            }
            let rows = query
                .load::<MetaSpecRow>(&mut *conn)
                .context("list meta_specs")?;
            rows.into_iter().map(|r| r.into_domain()).collect()
        })
        .await?
    }

    async fn update(&self, meta_spec: &MetaSpec) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let ms_id = meta_spec.id.as_str().to_string();
        let kind = meta_spec.kind.as_str().to_string();
        let name = meta_spec.name.clone();
        let scope = meta_spec.scope.as_str().to_string();
        let scope_id = meta_spec.scope_id.clone();
        let prompt = meta_spec.prompt.clone();
        let version = meta_spec.version as i32;
        let content_hash = meta_spec.content_hash.clone();
        let required = meta_spec.required as i32;
        let approval_status = meta_spec.approval_status.as_str().to_string();
        let approved_by = meta_spec.approved_by.clone();
        let approved_at = meta_spec.approved_at.map(|t| t as i64);
        let updated_at = meta_spec.updated_at as i64;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let now = now_secs();

            // Archive current version.
            let current = meta_specs::table
                .filter(meta_specs::id.eq(&ms_id))
                .first::<MetaSpecRow>(&mut *conn)
                .optional()
                .context("fetch current meta_spec")?;

            if let Some(current_row) = current {
                let version_id = uuid::Uuid::new_v4().to_string();
                let ver_row = InsertVersionRow {
                    id: &version_id,
                    meta_spec_id: &current_row.id,
                    version: current_row.version,
                    prompt: &current_row.prompt,
                    content_hash: &current_row.content_hash,
                    created_at: now,
                };
                diesel::insert_into(meta_spec_versions::table)
                    .values(&ver_row)
                    .execute(&mut *conn)
                    .context("archive meta_spec version")?;
            }

            diesel::update(meta_specs::table.filter(meta_specs::id.eq(&ms_id)))
                .set((
                    meta_specs::kind.eq(&kind),
                    meta_specs::name.eq(&name),
                    meta_specs::scope.eq(&scope),
                    meta_specs::scope_id.eq(&scope_id),
                    meta_specs::prompt.eq(&prompt),
                    meta_specs::version.eq(version),
                    meta_specs::content_hash.eq(&content_hash),
                    meta_specs::required.eq(required),
                    meta_specs::approval_status.eq(&approval_status),
                    meta_specs::approved_by.eq(&approved_by),
                    meta_specs::approved_at.eq(&approved_at),
                    meta_specs::updated_at.eq(updated_at),
                ))
                .execute(&mut *conn)
                .context("update meta_spec")?;
            Ok(())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id_str = id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let binding_count = meta_spec_bindings::table
                .filter(meta_spec_bindings::meta_spec_id.eq(&id_str))
                .count()
                .get_result::<i64>(&mut *conn)
                .context("count bindings")?;
            if binding_count > 0 {
                return Err(anyhow!(
                    "cannot delete meta_spec '{id_str}': {binding_count} binding(s) reference it"
                ));
            }
            diesel::delete(meta_specs::table.filter(meta_specs::id.eq(&id_str)))
                .execute(&mut *conn)
                .context("delete meta_spec")?;
            Ok(())
        })
        .await?
    }

    async fn list_versions(&self, meta_spec_id: &Id) -> Result<Vec<MetaSpecVersion>> {
        let pool = Arc::clone(&self.pool);
        let ms_id = meta_spec_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<MetaSpecVersion>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = meta_spec_versions::table
                .filter(meta_spec_versions::meta_spec_id.eq(&ms_id))
                .order(meta_spec_versions::version.asc())
                .load::<MetaSpecVersionRow>(&mut *conn)
                .context("list meta_spec_versions")?;
            Ok(rows.into_iter().map(|r| r.into_domain()).collect())
        })
        .await?
    }

    async fn get_version(
        &self,
        meta_spec_id: &Id,
        version: u32,
    ) -> Result<Option<MetaSpecVersion>> {
        let pool = Arc::clone(&self.pool);
        let ms_id = meta_spec_id.as_str().to_string();
        let ver = version as i32;
        tokio::task::spawn_blocking(move || -> Result<Option<MetaSpecVersion>> {
            let mut conn = pool.get().context("get db connection")?;
            // Check the archive table first.
            let row = meta_spec_versions::table
                .filter(meta_spec_versions::meta_spec_id.eq(&ms_id))
                .filter(meta_spec_versions::version.eq(ver))
                .first::<MetaSpecVersionRow>(&mut *conn)
                .optional()
                .context("get meta_spec_version")?;
            if let Some(r) = row {
                return Ok(Some(r.into_domain()));
            }
            // Fall back to the live row when version == current version.
            let live = meta_specs::table
                .filter(meta_specs::id.eq(&ms_id))
                .filter(meta_specs::version.eq(ver))
                .first::<MetaSpecRow>(&mut *conn)
                .optional()
                .context("get live meta_spec for version")?;
            Ok(live.map(|r| MetaSpecVersion {
                id: Id::new(r.id.clone()),
                meta_spec_id: Id::new(r.id),
                version: r.version as u32,
                prompt: r.prompt,
                content_hash: r.content_hash,
                created_at: r.updated_at as u64,
            }))
        })
        .await?
    }
}

// ---------------------------------------------------------------------------
// MetaSpecBindingRepository impl
// ---------------------------------------------------------------------------

#[async_trait]
impl MetaSpecBindingRepository for PgStorage {
    async fn create(&self, binding: &MetaSpecBinding) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let b_id = binding.id.as_str().to_string();
        let spec_id = binding.spec_id.clone();
        let ms_id = binding.meta_spec_id.as_str().to_string();
        let pinned = binding.pinned_version as i32;
        let created_at = binding.created_at as i64;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = InsertBindingRow {
                id: &b_id,
                spec_id: &spec_id,
                meta_spec_id: &ms_id,
                pinned_version: pinned,
                created_at,
            };
            diesel::insert_into(meta_spec_bindings::table)
                .values(&row)
                .execute(&mut *conn)
                .context("insert meta_spec_binding")?;
            Ok(())
        })
        .await?
    }

    async fn list_by_spec_id(&self, spec_id: &str) -> Result<Vec<MetaSpecBinding>> {
        let pool = Arc::clone(&self.pool);
        let sid = spec_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<MetaSpecBinding>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = meta_spec_bindings::table
                .filter(meta_spec_bindings::spec_id.eq(&sid))
                .load::<MetaSpecBindingRow>(&mut *conn)
                .context("list meta_spec_bindings")?;
            Ok(rows.into_iter().map(|r| r.into_domain()).collect())
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let id_str = id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::delete(meta_spec_bindings::table.filter(meta_spec_bindings::id.eq(&id_str)))
                .execute(&mut *conn)
                .context("delete meta_spec_binding")?;
            Ok(())
        })
        .await?
    }

    async fn has_bindings_for(&self, meta_spec_id: &Id) -> Result<bool> {
        let pool = Arc::clone(&self.pool);
        let ms_id = meta_spec_id.as_str().to_string();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            let mut conn = pool.get().context("get db connection")?;
            let count = meta_spec_bindings::table
                .filter(meta_spec_bindings::meta_spec_id.eq(&ms_id))
                .count()
                .get_result::<i64>(&mut *conn)
                .context("count bindings")?;
            Ok(count > 0)
        })
        .await?
    }
}
