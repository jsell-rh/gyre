use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::Id;
use gyre_domain::{DependencyEdge, DependencyStatus, DependencyType, DetectionMethod};
use gyre_ports::DependencyRepository;
use std::sync::Arc;

use super::PgStorage;
use crate::schema::dependency_edges;

fn dep_type_to_str(t: &DependencyType) -> &'static str {
    match t {
        DependencyType::Code => "code",
        DependencyType::Spec => "spec",
        DependencyType::Api => "api",
        DependencyType::Schema => "schema",
        DependencyType::Manual => "manual",
    }
}

fn str_to_dep_type(s: &str) -> Result<DependencyType> {
    match s {
        "code" => Ok(DependencyType::Code),
        "spec" => Ok(DependencyType::Spec),
        "api" => Ok(DependencyType::Api),
        "schema" => Ok(DependencyType::Schema),
        "manual" => Ok(DependencyType::Manual),
        other => Err(anyhow!("unknown dependency type: {}", other)),
    }
}

fn detection_to_str(d: &DetectionMethod) -> &'static str {
    match d {
        DetectionMethod::CargoToml => "cargo_toml",
        DetectionMethod::PackageJson => "package_json",
        DetectionMethod::GoMod => "go_mod",
        DetectionMethod::ManifestLink => "manifest_link",
        DetectionMethod::OpenApiRef => "open_api_ref",
        DetectionMethod::ProtoImport => "proto_import",
        DetectionMethod::McpToolRef => "mcp_tool_ref",
        DetectionMethod::Manual => "manual",
    }
}

fn str_to_detection(s: &str) -> Result<DetectionMethod> {
    match s {
        "cargo_toml" => Ok(DetectionMethod::CargoToml),
        "package_json" => Ok(DetectionMethod::PackageJson),
        "go_mod" => Ok(DetectionMethod::GoMod),
        "manifest_link" => Ok(DetectionMethod::ManifestLink),
        "open_api_ref" => Ok(DetectionMethod::OpenApiRef),
        "proto_import" => Ok(DetectionMethod::ProtoImport),
        "mcp_tool_ref" => Ok(DetectionMethod::McpToolRef),
        "manual" => Ok(DetectionMethod::Manual),
        other => Err(anyhow!("unknown detection method: {}", other)),
    }
}

fn status_to_str(s: &DependencyStatus) -> &'static str {
    match s {
        DependencyStatus::Active => "active",
        DependencyStatus::Stale => "stale",
        DependencyStatus::Breaking => "breaking",
        DependencyStatus::Orphaned => "orphaned",
    }
}

fn str_to_status(s: &str) -> Result<DependencyStatus> {
    match s {
        "active" => Ok(DependencyStatus::Active),
        "stale" => Ok(DependencyStatus::Stale),
        "breaking" => Ok(DependencyStatus::Breaking),
        "orphaned" => Ok(DependencyStatus::Orphaned),
        other => Err(anyhow!("unknown dependency status: {}", other)),
    }
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = dependency_edges)]
#[diesel(check_for_backend(diesel::pg::Pg))]
struct DependencyEdgeRow {
    id: String,
    source_repo_id: String,
    target_repo_id: String,
    dependency_type: String,
    source_artifact: String,
    target_artifact: String,
    version_pinned: Option<String>,
    target_version_current: Option<String>,
    version_drift: Option<i32>,
    detection_method: String,
    status: String,
    detected_at: i64,
    last_verified_at: i64,
}

impl DependencyEdgeRow {
    fn into_edge(self) -> Result<DependencyEdge> {
        Ok(DependencyEdge {
            id: Id::new(self.id),
            source_repo_id: Id::new(self.source_repo_id),
            target_repo_id: Id::new(self.target_repo_id),
            dependency_type: str_to_dep_type(&self.dependency_type)?,
            source_artifact: self.source_artifact,
            target_artifact: self.target_artifact,
            version_pinned: self.version_pinned,
            target_version_current: self.target_version_current,
            version_drift: self.version_drift.map(|v| v as u32),
            detection_method: str_to_detection(&self.detection_method)?,
            status: str_to_status(&self.status)?,
            detected_at: self.detected_at as u64,
            last_verified_at: self.last_verified_at as u64,
        })
    }
}

#[derive(Insertable)]
#[diesel(table_name = dependency_edges)]
struct NewDependencyEdgeRow<'a> {
    id: &'a str,
    source_repo_id: &'a str,
    target_repo_id: &'a str,
    dependency_type: &'a str,
    source_artifact: &'a str,
    target_artifact: &'a str,
    version_pinned: Option<&'a str>,
    target_version_current: Option<&'a str>,
    version_drift: Option<i32>,
    detection_method: &'a str,
    status: &'a str,
    detected_at: i64,
    last_verified_at: i64,
}

#[async_trait]
impl DependencyRepository for PgStorage {
    async fn save(&self, edge: &DependencyEdge) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let e = edge.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            let row = NewDependencyEdgeRow {
                id: e.id.as_str(),
                source_repo_id: e.source_repo_id.as_str(),
                target_repo_id: e.target_repo_id.as_str(),
                dependency_type: dep_type_to_str(&e.dependency_type),
                source_artifact: &e.source_artifact,
                target_artifact: &e.target_artifact,
                version_pinned: e.version_pinned.as_deref(),
                target_version_current: e.target_version_current.as_deref(),
                version_drift: e.version_drift.map(|v| v as i32),
                detection_method: detection_to_str(&e.detection_method),
                status: status_to_str(&e.status),
                detected_at: e.detected_at as i64,
                last_verified_at: e.last_verified_at as i64,
            };
            diesel::insert_into(dependency_edges::table)
                .values(&row)
                .on_conflict(dependency_edges::id)
                .do_update()
                .set((
                    dependency_edges::dependency_type.eq(row.dependency_type),
                    dependency_edges::source_artifact.eq(row.source_artifact),
                    dependency_edges::target_artifact.eq(row.target_artifact),
                    dependency_edges::version_pinned.eq(row.version_pinned),
                    dependency_edges::target_version_current.eq(row.target_version_current),
                    dependency_edges::version_drift.eq(row.version_drift),
                    dependency_edges::detection_method.eq(row.detection_method),
                    dependency_edges::status.eq(row.status),
                    dependency_edges::last_verified_at.eq(row.last_verified_at),
                ))
                .execute(&mut *conn)
                .context("save dependency edge")?;
            Ok(())
        })
        .await?
    }

    async fn find_by_id(&self, id: &Id) -> Result<Option<DependencyEdge>> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<Option<DependencyEdge>> {
            let mut conn = pool.get().context("get db connection")?;
            let result = dependency_edges::table
                .find(id.as_str())
                .first::<DependencyEdgeRow>(&mut *conn)
                .optional()
                .context("find dependency edge by id")?;
            result.map(DependencyEdgeRow::into_edge).transpose()
        })
        .await?
    }

    async fn list_by_repo(&self, repo_id: &Id) -> Result<Vec<DependencyEdge>> {
        let pool = Arc::clone(&self.pool);
        let rid = repo_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<DependencyEdge>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = dependency_edges::table
                .filter(dependency_edges::source_repo_id.eq(rid.as_str()))
                .load::<DependencyEdgeRow>(&mut *conn)
                .context("list dependency edges by repo")?;
            rows.into_iter().map(DependencyEdgeRow::into_edge).collect()
        })
        .await?
    }

    async fn list_dependents(&self, repo_id: &Id) -> Result<Vec<DependencyEdge>> {
        let pool = Arc::clone(&self.pool);
        let rid = repo_id.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<DependencyEdge>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = dependency_edges::table
                .filter(dependency_edges::target_repo_id.eq(rid.as_str()))
                .load::<DependencyEdgeRow>(&mut *conn)
                .context("list dependents")?;
            rows.into_iter().map(DependencyEdgeRow::into_edge).collect()
        })
        .await?
    }

    async fn list_all(&self) -> Result<Vec<DependencyEdge>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || -> Result<Vec<DependencyEdge>> {
            let mut conn = pool.get().context("get db connection")?;
            let rows = dependency_edges::table
                .load::<DependencyEdgeRow>(&mut *conn)
                .context("list all dependency edges")?;
            rows.into_iter().map(DependencyEdgeRow::into_edge).collect()
        })
        .await?
    }

    async fn delete(&self, id: &Id) -> Result<bool> {
        let pool = Arc::clone(&self.pool);
        let id = id.clone();
        tokio::task::spawn_blocking(move || -> Result<bool> {
            let mut conn = pool.get().context("get db connection")?;
            let count = diesel::delete(dependency_edges::table.find(id.as_str()))
                .execute(&mut *conn)
                .context("delete dependency edge")?;
            Ok(count > 0)
        })
        .await?
    }
}
