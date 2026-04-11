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
    /// Current version of the target repo. None if unknown.
    pub target_version_current: Option<String>,
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
            target_version_current: None,
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
    PyprojectToml,
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

// ── Breaking change detection & enforcement ─────────────────────────────────

/// A record of a detected breaking change in a target repo that affects dependents.
///
/// Created when a push to a repo is detected as a breaking change (semver major
/// bump via conventional commit, or API contract change). One record per
/// affected dependency edge.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BreakingChange {
    pub id: Id,
    /// The dependency edge that is affected.
    pub dependency_edge_id: Id,
    /// The repo that introduced the breaking change (target of the dep edge).
    pub source_repo_id: Id,
    /// The commit SHA that introduced the breaking change.
    pub commit_sha: String,
    /// Human-readable description of the breaking change.
    pub description: String,
    /// Unix timestamp when the breaking change was detected.
    pub detected_at: u64,
    /// Whether a dependent repo has acknowledged the breaking change.
    pub acknowledged: bool,
    /// Who acknowledged the breaking change.
    pub acknowledged_by: Option<String>,
    /// Unix timestamp when it was acknowledged.
    pub acknowledged_at: Option<u64>,
}

impl BreakingChange {
    pub fn new(
        id: Id,
        dependency_edge_id: Id,
        source_repo_id: Id,
        commit_sha: impl Into<String>,
        description: impl Into<String>,
        detected_at: u64,
    ) -> Self {
        Self {
            id,
            dependency_edge_id,
            source_repo_id,
            commit_sha: commit_sha.into(),
            description: description.into(),
            detected_at,
            acknowledged: false,
            acknowledged_by: None,
            acknowledged_at: None,
        }
    }
}

/// Per-workspace dependency enforcement policy.
///
/// Controls how the forge responds to breaking changes, version drift,
/// and stale dependencies across the workspace's repos.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DependencyPolicy {
    /// What happens when a breaking change is detected: block, warn, or notify.
    pub breaking_change_behavior: BreakingChangeBehavior,
    /// Flag repos more than this many versions behind. 0 = disabled.
    pub max_version_drift: u32,
    /// Flag dependencies not updated in this many days. 0 = disabled.
    pub stale_dependency_alert_days: u32,
    /// Whether to require cascade tests before merging breaking changes.
    pub require_cascade_tests: bool,
    /// Whether to auto-create tasks for dependency updates.
    pub auto_create_update_tasks: bool,
}

impl Default for DependencyPolicy {
    fn default() -> Self {
        Self {
            breaking_change_behavior: BreakingChangeBehavior::Warn,
            max_version_drift: 3,
            stale_dependency_alert_days: 30,
            require_cascade_tests: true,
            auto_create_update_tasks: true,
        }
    }
}

/// Enforcement behavior for breaking changes in the dependency graph.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BreakingChangeBehavior {
    /// Breaking change cannot merge until all dependents acknowledge.
    Block,
    /// Breaking change merges with warnings. Tasks auto-created.
    Warn,
    /// Breaking change merges silently. Dependent orchestrators notified.
    Notify,
}
