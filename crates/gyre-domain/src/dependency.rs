//! Cross-repo dependency graph entities.
//!
//! Tracks dependencies between repositories at the tenant level.
//! Enables blast-radius analysis, breaking-change detection, and version drift tracking.

use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// A directed dependency edge from one repo to another.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DependencyEdge {
    pub id: Id,
    /// The repo that depends on the target.
    pub source_repo_id: Id,
    /// The repo being depended on.
    pub target_repo_id: Id,
    pub dependency_type: DependencyType,
    /// File or artifact in the source repo that declares the dependency (e.g. "Cargo.toml").
    pub source_artifact: String,
    /// Name or path of the artifact in the target repo (e.g. crate name, spec path).
    pub target_artifact: String,
    /// Version the source pins (e.g. "1.2.3", "^2.0"). None for path deps or spec links.
    pub version_pinned: Option<String>,
    /// How many versions the source is behind the target. None if unknown.
    pub version_drift: Option<u32>,
    pub detection_method: DetectionMethod,
    pub status: DependencyStatus,
    pub detected_at: u64,
    pub last_verified_at: u64,
}

impl DependencyEdge {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Id,
        source_repo_id: Id,
        target_repo_id: Id,
        dependency_type: DependencyType,
        source_artifact: impl Into<String>,
        target_artifact: impl Into<String>,
        detection_method: DetectionMethod,
        now: u64,
    ) -> Self {
        Self {
            id,
            source_repo_id,
            target_repo_id,
            dependency_type,
            source_artifact: source_artifact.into(),
            target_artifact: target_artifact.into(),
            version_pinned: None,
            version_drift: None,
            detection_method,
            status: DependencyStatus::Active,
            detected_at: now,
            last_verified_at: now,
        }
    }
}

/// Classification of the dependency relationship.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DependencyType {
    /// Source code dependency (Cargo.toml, package.json, go.mod).
    Code,
    /// Spec-level dependency (specs/manifest.yaml cross-repo links).
    Spec,
    /// API contract dependency (OpenAPI, gRPC proto).
    Api,
    /// Shared schema dependency (protobuf imports, schema references).
    Schema,
    /// Manually declared via API — cannot be auto-detected.
    Manual,
}

/// How the dependency was detected.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DetectionMethod {
    CargoToml,
    PackageJson,
    GoMod,
    ManifestLink,
    OpenApiRef,
    ProtoImport,
    McpToolRef,
    Manual,
}

/// Lifecycle status of the dependency edge.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DependencyStatus {
    /// Dependency is current and healthy.
    Active,
    /// Source pins an old version of target.
    Stale,
    /// Target has published a breaking change source hasn't adopted.
    Breaking,
    /// Target repo was deleted or archived.
    Orphaned,
}
