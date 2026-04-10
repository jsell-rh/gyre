//! Cross-repo dependency graph API (M22.4).
//!
//! Endpoints:
//!   GET    /api/v1/repos/{id}/dependencies            — outgoing deps (this repo depends on)
//!   GET    /api/v1/repos/{id}/dependents              — incoming deps (repos that depend on this)
//!   POST   /api/v1/repos/{id}/dependencies            — add manual dependency
//!   DELETE /api/v1/repos/{id}/dependencies/{dep_id}  — remove a dependency (manual only)
//!   GET    /api/v1/dependencies/graph                 — full tenant-wide graph
//!   GET    /api/v1/repos/{id}/blast-radius            — transitive dependents (BFS)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{DependencyEdge, DependencyStatus, DependencyType, DetectionMethod};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use crate::AppState;

use super::{error::ApiError, new_id, now_secs};

// ── Request / Response types ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AddDependencyRequest {
    pub target_repo_id: String,
    #[serde(default = "default_dep_type")]
    pub dependency_type: DependencyType,
    #[serde(default)]
    pub source_artifact: Option<String>,
    #[serde(default)]
    pub target_artifact: Option<String>,
}

fn default_dep_type() -> DependencyType {
    DependencyType::Manual
}

#[derive(Serialize)]
pub struct DependencyEdgeResponse {
    pub id: String,
    pub source_repo_id: String,
    pub target_repo_id: String,
    pub dependency_type: DependencyType,
    pub source_artifact: String,
    pub target_artifact: String,
    pub version_pinned: Option<String>,
    pub target_version_current: Option<String>,
    pub version_drift: Option<u32>,
    pub detection_method: DetectionMethod,
    pub status: DependencyStatus,
    pub detected_at: u64,
    pub last_verified_at: u64,
}

impl From<DependencyEdge> for DependencyEdgeResponse {
    fn from(e: DependencyEdge) -> Self {
        Self {
            id: e.id.to_string(),
            source_repo_id: e.source_repo_id.to_string(),
            target_repo_id: e.target_repo_id.to_string(),
            dependency_type: e.dependency_type,
            source_artifact: e.source_artifact,
            target_artifact: e.target_artifact,
            version_pinned: e.version_pinned,
            target_version_current: e.target_version_current,
            version_drift: e.version_drift,
            detection_method: e.detection_method,
            status: e.status,
            detected_at: e.detected_at,
            last_verified_at: e.last_verified_at,
        }
    }
}

#[derive(Serialize)]
pub struct GraphNode {
    pub repo_id: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct GraphEdgeResponse {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub edge_type: DependencyType,
    pub status: DependencyStatus,
}

#[derive(Serialize)]
pub struct GraphResponse {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdgeResponse>,
}

#[derive(Serialize)]
pub struct BlastRadiusResponse {
    pub repo_id: String,
    pub direct_dependents: Vec<String>,
    pub transitive_dependents: Vec<String>,
    pub total: usize,
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// GET /api/v1/repos/{id}/dependencies
/// Returns all outgoing dependency edges from this repo.
pub async fn list_dependencies(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<DependencyEdgeResponse>>, ApiError> {
    let repo_id = Id::new(&id);
    state
        .repos
        .find_by_id(&repo_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("repo {id} not found")))?;

    let edges = state
        .dependencies
        .list_by_repo(&repo_id)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(edges.into_iter().map(Into::into).collect()))
}

/// GET /api/v1/repos/{id}/dependents
/// Returns all incoming dependency edges pointing at this repo.
pub async fn list_dependents(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<DependencyEdgeResponse>>, ApiError> {
    let repo_id = Id::new(&id);
    state
        .repos
        .find_by_id(&repo_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("repo {id} not found")))?;

    let edges = state
        .dependencies
        .list_dependents(&repo_id)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(edges.into_iter().map(Into::into).collect()))
}

/// POST /api/v1/repos/{id}/dependencies
/// Manually declare a dependency from this repo to another.
pub async fn add_dependency(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<AddDependencyRequest>,
) -> Result<(StatusCode, Json<DependencyEdgeResponse>), ApiError> {
    let source_repo_id = Id::new(&id);
    let target_repo_id = Id::new(&req.target_repo_id);

    state
        .repos
        .find_by_id(&source_repo_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("source repo {id} not found")))?;

    state
        .repos
        .find_by_id(&target_repo_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| {
            ApiError::NotFound(format!("target repo {} not found", req.target_repo_id))
        })?;

    if source_repo_id.as_str() == target_repo_id.as_str() {
        return Err(ApiError::InvalidInput(
            "a repo cannot depend on itself".to_string(),
        ));
    }

    let now = now_secs();
    let edge = DependencyEdge::new(
        new_id(),
        source_repo_id,
        target_repo_id,
        req.dependency_type,
        req.source_artifact.unwrap_or_default(),
        req.target_artifact.unwrap_or_default(),
        DetectionMethod::Manual,
        now,
    );

    state
        .dependencies
        .save(&edge)
        .await
        .map_err(ApiError::Internal)?;

    Ok((StatusCode::CREATED, Json(edge.into())))
}

/// DELETE /api/v1/repos/{id}/dependencies/{dep_id}
/// Remove a dependency edge. Only manually-declared edges can be deleted via API.
pub async fn delete_dependency(
    State(state): State<Arc<AppState>>,
    Path((repo_id, dep_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let edge_id = Id::new(&dep_id);
    let edge = state
        .dependencies
        .find_by_id(&edge_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("dependency {dep_id} not found")))?;

    if edge.source_repo_id.as_str() != repo_id {
        return Err(ApiError::NotFound(format!("dependency {dep_id} not found")));
    }

    if edge.detection_method != DetectionMethod::Manual {
        return Err(ApiError::InvalidInput(
            "only manually-declared dependencies can be deleted via API; \
             auto-detected edges are managed by the forge"
                .to_string(),
        ));
    }

    state
        .dependencies
        .delete(&edge_id)
        .await
        .map_err(ApiError::Internal)?;

    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/dependencies/graph
/// Returns the full tenant-wide dependency graph.
pub async fn get_graph(
    State(state): State<Arc<AppState>>,
) -> Result<Json<GraphResponse>, ApiError> {
    let all_edges = state
        .dependencies
        .list_all()
        .await
        .map_err(ApiError::Internal)?;

    let mut repo_ids: HashSet<String> = HashSet::new();
    for edge in &all_edges {
        repo_ids.insert(edge.source_repo_id.to_string());
        repo_ids.insert(edge.target_repo_id.to_string());
    }

    let mut nodes = Vec::new();
    for rid in repo_ids {
        let name = state
            .repos
            .find_by_id(&Id::new(&rid))
            .await
            .ok()
            .flatten()
            .map(|r| r.name)
            .unwrap_or_else(|| rid.clone());
        nodes.push(GraphNode { repo_id: rid, name });
    }
    nodes.sort_by(|a, b| a.repo_id.cmp(&b.repo_id));

    let edges = all_edges
        .into_iter()
        .map(|e| GraphEdgeResponse {
            id: e.id.to_string(),
            source: e.source_repo_id.to_string(),
            target: e.target_repo_id.to_string(),
            edge_type: e.dependency_type,
            status: e.status,
        })
        .collect();

    Ok(Json(GraphResponse { nodes, edges }))
}

/// GET /api/v1/workspaces/{id}/dependency-graph
/// Returns the dependency graph scoped to repos within a workspace.
/// Includes edges where at least one endpoint (source or target) is in the workspace.
pub async fn get_workspace_dependency_graph(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<GraphResponse>, ApiError> {
    let workspace_id = Id::new(&id);

    state
        .workspaces
        .find_by_id(&workspace_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {id} not found")))?;

    let ws_repos = state
        .repos
        .list_by_workspace(&workspace_id)
        .await
        .map_err(ApiError::Internal)?;

    let ws_repo_ids: HashSet<String> = ws_repos.iter().map(|r| r.id.to_string()).collect();

    let all_edges = state
        .dependencies
        .list_all()
        .await
        .map_err(ApiError::Internal)?;

    // Keep edges where at least one endpoint is in the workspace.
    let filtered_edges: Vec<DependencyEdge> = all_edges
        .into_iter()
        .filter(|e| {
            ws_repo_ids.contains(&e.source_repo_id.to_string())
                || ws_repo_ids.contains(&e.target_repo_id.to_string())
        })
        .collect();

    // Collect all repo IDs referenced by filtered edges.
    let mut repo_ids: HashSet<String> = HashSet::new();
    for edge in &filtered_edges {
        repo_ids.insert(edge.source_repo_id.to_string());
        repo_ids.insert(edge.target_repo_id.to_string());
    }

    let mut nodes = Vec::new();
    for rid in repo_ids {
        let name = state
            .repos
            .find_by_id(&Id::new(&rid))
            .await
            .ok()
            .flatten()
            .map(|r| r.name)
            .unwrap_or_else(|| rid.clone());
        nodes.push(GraphNode { repo_id: rid, name });
    }
    nodes.sort_by(|a, b| a.repo_id.cmp(&b.repo_id));

    let edges = filtered_edges
        .into_iter()
        .map(|e| GraphEdgeResponse {
            id: e.id.to_string(),
            source: e.source_repo_id.to_string(),
            target: e.target_repo_id.to_string(),
            edge_type: e.dependency_type,
            status: e.status,
        })
        .collect();

    Ok(Json(GraphResponse { nodes, edges }))
}

/// GET /api/v1/repos/{id}/blast-radius
/// BFS from the specified repo to find all transitive dependents.
pub async fn blast_radius(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<BlastRadiusResponse>, ApiError> {
    let repo_id = Id::new(&id);
    state
        .repos
        .find_by_id(&repo_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("repo {id} not found")))?;

    let all_edges = state
        .dependencies
        .list_all()
        .await
        .map_err(ApiError::Internal)?;

    // BFS over the reverse graph to find all dependents.
    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    queue.push_back((id.clone(), 0));
    visited.insert(id.clone());

    let mut direct: Vec<String> = Vec::new();
    let mut transitive: Vec<String> = Vec::new();

    while let Some((current, depth)) = queue.pop_front() {
        let dependents: Vec<String> = all_edges
            .iter()
            .filter(|e| e.target_repo_id.as_str() == current.as_str())
            .map(|e| e.source_repo_id.to_string())
            .collect();

        for dep in dependents {
            if visited.insert(dep.clone()) {
                if depth == 0 {
                    direct.push(dep.clone());
                } else {
                    transitive.push(dep.clone());
                }
                queue.push_back((dep, depth + 1));
            }
        }
    }

    let total = direct.len() + transitive.len();
    Ok(Json(BlastRadiusResponse {
        repo_id: id,
        direct_dependents: direct,
        transitive_dependents: transitive,
        total,
    }))
}

// ── Stale dependencies endpoint (TASK-021) ─────────────────────────────────

#[derive(Deserialize)]
pub struct StaleQuery {
    pub workspace_id: Option<String>,
}

/// GET /api/v1/dependencies/stale
/// Returns all dependency edges with status `Stale` across the tenant.
/// Optionally filtered by `?workspace_id=`.
pub async fn list_stale_dependencies(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<StaleQuery>,
) -> Result<Json<Vec<DependencyEdgeResponse>>, ApiError> {
    let all_edges = state
        .dependencies
        .list_all()
        .await
        .map_err(ApiError::Internal)?;

    let mut stale: Vec<DependencyEdge> = all_edges
        .into_iter()
        .filter(|e| e.status == DependencyStatus::Stale)
        .collect();

    // If workspace_id filter is provided, restrict to edges whose source repo
    // belongs to that workspace.
    if let Some(ref ws_id) = query.workspace_id {
        let ws_repos = state
            .repos
            .list_by_workspace(&Id::new(ws_id))
            .await
            .map_err(ApiError::Internal)?;
        let ws_repo_ids: HashSet<String> = ws_repos.iter().map(|r| r.id.to_string()).collect();
        stale.retain(|e| ws_repo_ids.contains(&e.source_repo_id.to_string()));
    }

    Ok(Json(stale.into_iter().map(Into::into).collect()))
}

// ── Breaking change endpoints (TASK-020) ────────────────────────────────────

#[derive(Serialize)]
pub struct BreakingChangeResponse {
    pub id: String,
    pub dependency_edge_id: String,
    pub source_repo_id: String,
    pub commit_sha: String,
    pub description: String,
    pub detected_at: u64,
    pub acknowledged: bool,
    pub acknowledged_by: Option<String>,
    pub acknowledged_at: Option<u64>,
}

impl From<gyre_domain::BreakingChange> for BreakingChangeResponse {
    fn from(bc: gyre_domain::BreakingChange) -> Self {
        Self {
            id: bc.id.to_string(),
            dependency_edge_id: bc.dependency_edge_id.to_string(),
            source_repo_id: bc.source_repo_id.to_string(),
            commit_sha: bc.commit_sha,
            description: bc.description,
            detected_at: bc.detected_at,
            acknowledged: bc.acknowledged,
            acknowledged_by: bc.acknowledged_by,
            acknowledged_at: bc.acknowledged_at,
        }
    }
}

#[derive(Serialize)]
pub struct DependencyPolicyResponse {
    pub breaking_change_behavior: gyre_domain::BreakingChangeBehavior,
    pub max_version_drift: u32,
    pub stale_dependency_alert_days: u32,
    pub require_cascade_tests: bool,
    pub auto_create_update_tasks: bool,
}

impl From<gyre_domain::DependencyPolicy> for DependencyPolicyResponse {
    fn from(p: gyre_domain::DependencyPolicy) -> Self {
        Self {
            breaking_change_behavior: p.breaking_change_behavior,
            max_version_drift: p.max_version_drift,
            stale_dependency_alert_days: p.stale_dependency_alert_days,
            require_cascade_tests: p.require_cascade_tests,
            auto_create_update_tasks: p.auto_create_update_tasks,
        }
    }
}

#[derive(Deserialize)]
pub struct UpdateDependencyPolicyRequest {
    pub breaking_change_behavior: Option<gyre_domain::BreakingChangeBehavior>,
    pub max_version_drift: Option<u32>,
    pub stale_dependency_alert_days: Option<u32>,
    pub require_cascade_tests: Option<bool>,
    pub auto_create_update_tasks: Option<bool>,
}

/// GET /api/v1/dependencies/breaking
/// Lists all unacknowledged breaking changes across the tenant.
pub async fn list_breaking_changes(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<BreakingChangeResponse>>, ApiError> {
    let changes = state
        .breaking_changes
        .list_unacknowledged()
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(changes.into_iter().map(Into::into).collect()))
}

/// POST /api/v1/dependencies/breaking/:id/acknowledge
/// Acknowledge a breaking change, clearing any merge block.
pub async fn acknowledge_breaking_change(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let bc_id = Id::new(&id);

    // Verify the breaking change exists.
    let bc = state
        .breaking_changes
        .find_by_id(&bc_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("breaking change {id} not found")))?;

    if bc.acknowledged {
        return Ok(StatusCode::NO_CONTENT);
    }

    let now = now_secs();
    let acknowledged = state
        .breaking_changes
        .acknowledge(&bc_id, "api", now)
        .await
        .map_err(ApiError::Internal)?;

    if !acknowledged {
        return Err(ApiError::NotFound(format!(
            "breaking change {id} not found"
        )));
    }

    // If all breaking changes for this dependency edge are now acknowledged,
    // revert the edge status to Active.
    let edge_id = bc.dependency_edge_id.clone();
    let all_for_edge = state
        .breaking_changes
        .list_by_source_repo(&bc.source_repo_id)
        .await
        .unwrap_or_default();

    let all_acknowledged = all_for_edge
        .iter()
        .filter(|b| b.dependency_edge_id.as_str() == edge_id.as_str())
        .all(|b| b.acknowledged || b.id.as_str() == id);

    if all_acknowledged {
        if let Some(mut edge) = state.dependencies.find_by_id(&edge_id).await.ok().flatten() {
            edge.status = gyre_domain::DependencyStatus::Active;
            edge.last_verified_at = now;
            if let Err(e) = state.dependencies.save(&edge).await {
                tracing::warn!("failed to revert edge status to active: {e}");
            }
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/workspaces/:id/dependency-policy
/// Returns the dependency enforcement policy for a workspace.
pub async fn get_dependency_policy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DependencyPolicyResponse>, ApiError> {
    let workspace_id = Id::new(&id);

    // Verify workspace exists.
    state
        .workspaces
        .find_by_id(&workspace_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {id} not found")))?;

    let policy = state
        .dependency_policies
        .get_for_workspace(&workspace_id)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(policy.into()))
}

/// PUT /api/v1/workspaces/:id/dependency-policy
/// Update the dependency enforcement policy for a workspace.
pub async fn set_dependency_policy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateDependencyPolicyRequest>,
) -> Result<Json<DependencyPolicyResponse>, ApiError> {
    let workspace_id = Id::new(&id);

    // Verify workspace exists.
    state
        .workspaces
        .find_by_id(&workspace_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {id} not found")))?;

    // Get current policy and apply partial updates.
    let mut policy = state
        .dependency_policies
        .get_for_workspace(&workspace_id)
        .await
        .map_err(ApiError::Internal)?;

    if let Some(behavior) = req.breaking_change_behavior {
        policy.breaking_change_behavior = behavior;
    }
    if let Some(drift) = req.max_version_drift {
        policy.max_version_drift = drift;
    }
    if let Some(days) = req.stale_dependency_alert_days {
        policy.stale_dependency_alert_days = days;
    }
    if let Some(cascade) = req.require_cascade_tests {
        policy.require_cascade_tests = cascade;
    }
    if let Some(auto) = req.auto_create_update_tasks {
        policy.auto_create_update_tasks = auto;
    }

    state
        .dependency_policies
        .set_for_workspace(&workspace_id, &policy)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(policy.into()))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;

    fn setup() -> Arc<AppState> {
        test_state()
    }

    async fn create_repo(state: &Arc<AppState>, name: &str) -> Id {
        create_repo_in_workspace(state, name, "proj-1").await
    }

    async fn create_repo_in_workspace(state: &Arc<AppState>, name: &str, workspace_id: &str) -> Id {
        use gyre_domain::Repository;
        let repo = Repository::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            Id::new(workspace_id),
            name,
            format!("/tmp/{name}.git"),
            0,
        );
        state.repos.create(&repo).await.unwrap();
        repo.id
    }

    #[tokio::test]
    async fn test_add_and_list_dependency() {
        let state = setup();
        let a = create_repo(&state, "repo-a").await;
        let b = create_repo(&state, "repo-b").await;

        let edge = DependencyEdge::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            a.clone(),
            b.clone(),
            DependencyType::Code,
            "Cargo.toml",
            "crate-b",
            DetectionMethod::CargoToml,
            1000,
        );
        state.dependencies.save(&edge).await.unwrap();

        let deps = state.dependencies.list_by_repo(&a).await.unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].target_repo_id.as_str(), b.as_str());

        let dependents = state.dependencies.list_dependents(&b).await.unwrap();
        assert_eq!(dependents.len(), 1);
        assert_eq!(dependents[0].source_repo_id.as_str(), a.as_str());
    }

    #[tokio::test]
    async fn test_blast_radius_three_node_chain() {
        // Graph: A → B → C (A depends on B, B depends on C).
        // Blast radius of C: direct=[B], transitive=[A].
        let state = setup();
        let a = create_repo(&state, "chain-a").await;
        let b = create_repo(&state, "chain-b").await;
        let c = create_repo(&state, "chain-c").await;

        state
            .dependencies
            .save(&DependencyEdge::new(
                Id::new(uuid::Uuid::new_v4().to_string()),
                a.clone(),
                b.clone(),
                DependencyType::Code,
                "Cargo.toml",
                "crate-b",
                DetectionMethod::CargoToml,
                1000,
            ))
            .await
            .unwrap();

        state
            .dependencies
            .save(&DependencyEdge::new(
                Id::new(uuid::Uuid::new_v4().to_string()),
                b.clone(),
                c.clone(),
                DependencyType::Code,
                "Cargo.toml",
                "crate-c",
                DetectionMethod::CargoToml,
                1000,
            ))
            .await
            .unwrap();

        let all = state.dependencies.list_all().await.unwrap();
        assert_eq!(all.len(), 2);

        // BFS from C: direct deps of C = [B], transitive = [A].
        let all_edges = state.dependencies.list_all().await.unwrap();
        let direct_of_c: Vec<_> = all_edges
            .iter()
            .filter(|e| e.target_repo_id.as_str() == c.as_str())
            .collect();
        assert_eq!(direct_of_c.len(), 1);
        assert_eq!(direct_of_c[0].source_repo_id.as_str(), b.as_str());

        let direct_of_b: Vec<_> = all_edges
            .iter()
            .filter(|e| e.target_repo_id.as_str() == b.as_str())
            .collect();
        assert_eq!(direct_of_b.len(), 1);
        assert_eq!(direct_of_b[0].source_repo_id.as_str(), a.as_str());
    }

    #[tokio::test]
    async fn test_delete_dependency() {
        let state = setup();
        let a = create_repo(&state, "del-a").await;
        let b = create_repo(&state, "del-b").await;

        let eid = Id::new(uuid::Uuid::new_v4().to_string());
        state
            .dependencies
            .save(&DependencyEdge::new(
                eid.clone(),
                a.clone(),
                b,
                DependencyType::Manual,
                "",
                "",
                DetectionMethod::Manual,
                1000,
            ))
            .await
            .unwrap();

        let deleted = state.dependencies.delete(&eid).await.unwrap();
        assert!(deleted);

        let deps = state.dependencies.list_by_repo(&a).await.unwrap();
        assert!(deps.is_empty());
    }

    #[tokio::test]
    async fn test_cargo_toml_path_dep_detection() {
        let toml_content = r#"
[package]
name = "my-crate"
version = "0.1.0"

[dependencies]
serde = "1.0"
other-crate = { path = "../other-repo" }
gyre-client = { git = "https://github.com/example/gyre-client" }
"#;
        let path_deps = crate::git_http::detect_cargo_path_deps(toml_content);
        assert_eq!(path_deps.len(), 1);
        assert_eq!(path_deps[0], "../other-repo");
    }

    // ── Breaking change detection tests ────────────────────────────────

    #[test]
    fn test_detect_breaking_commits_feat_bang() {
        // Format: SHA\0FULL_MESSAGE\0 (from git log --format="%H%x00%B%x00")
        let log = "abc123\0feat!: remove deprecated API endpoints\0";
        let result = crate::git_http::detect_breaking_commits(log);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "abc123");
        assert_eq!(result[0].1, "remove deprecated API endpoints");
    }

    #[test]
    fn test_detect_breaking_commits_breaking_change_footer() {
        // BREAKING CHANGE: footer appears in the commit body (%B includes it).
        let log = "def456\0feat: redesign authentication flow\n\nBREAKING CHANGE: old tokens no longer work\0";
        let result = crate::git_http::detect_breaking_commits(log);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "def456");
        assert_eq!(result[0].1, "redesign authentication flow");
    }

    #[test]
    fn test_detect_breaking_commits_non_breaking() {
        let log = "abc123\0feat: add new endpoint\0def456\0fix: correct typo\0";
        let result = crate::git_http::detect_breaking_commits(log);
        assert!(result.is_empty());
    }

    #[test]
    fn test_detect_breaking_commits_mixed() {
        let log = "aaa111\0feat: safe change\0bbb222\0feat!: breaking change\0ccc333\0fix: patch\0";
        let result = crate::git_http::detect_breaking_commits(log);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "bbb222");
        assert_eq!(result[0].1, "breaking change");
    }

    #[test]
    fn test_detect_breaking_commits_empty() {
        let result = crate::git_http::detect_breaking_commits("");
        assert!(result.is_empty());
    }

    // ── Breaking change record CRUD tests ──────────────────────────────

    #[tokio::test]
    async fn test_breaking_change_create_and_list() {
        let state = setup();
        let a = create_repo(&state, "bc-a").await;
        let b = create_repo(&state, "bc-b").await;

        // Create a dependency edge: A depends on B.
        let edge = DependencyEdge::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            a.clone(),
            b.clone(),
            DependencyType::Code,
            "Cargo.toml",
            "crate-b",
            DetectionMethod::CargoToml,
            1000,
        );
        state.dependencies.save(&edge).await.unwrap();

        // Create a breaking change record.
        let bc = gyre_domain::BreakingChange::new(
            Id::new("bc-1"),
            edge.id.clone(),
            b.clone(),
            "abc123",
            "remove deprecated API",
            2000,
        );
        state.breaking_changes.create(&bc).await.unwrap();

        // List unacknowledged.
        let unacked = state.breaking_changes.list_unacknowledged().await.unwrap();
        assert_eq!(unacked.len(), 1);
        assert_eq!(unacked[0].commit_sha, "abc123");
        assert_eq!(unacked[0].description, "remove deprecated API");
        assert!(!unacked[0].acknowledged);
    }

    #[tokio::test]
    async fn test_breaking_change_acknowledge() {
        let state = setup();
        let a = create_repo(&state, "ack-a").await;
        let b = create_repo(&state, "ack-b").await;

        let edge = DependencyEdge::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            a.clone(),
            b.clone(),
            DependencyType::Code,
            "Cargo.toml",
            "crate-b",
            DetectionMethod::CargoToml,
            1000,
        );
        state.dependencies.save(&edge).await.unwrap();

        let bc = gyre_domain::BreakingChange::new(
            Id::new("bc-ack-1"),
            edge.id.clone(),
            b.clone(),
            "def456",
            "redesign auth",
            2000,
        );
        state.breaking_changes.create(&bc).await.unwrap();

        // Acknowledge.
        let acked = state
            .breaking_changes
            .acknowledge(&Id::new("bc-ack-1"), "user-1", 3000)
            .await
            .unwrap();
        assert!(acked);

        // Verify acknowledged.
        let found = state
            .breaking_changes
            .find_by_id(&Id::new("bc-ack-1"))
            .await
            .unwrap()
            .unwrap();
        assert!(found.acknowledged);
        assert_eq!(found.acknowledged_by, Some("user-1".to_string()));
        assert_eq!(found.acknowledged_at, Some(3000));

        // List unacknowledged should be empty.
        let unacked = state.breaking_changes.list_unacknowledged().await.unwrap();
        assert!(unacked.is_empty());
    }

    // ── Dependency policy tests ────────────────────────────────────────

    #[tokio::test]
    async fn test_dependency_policy_default() {
        let state = setup();
        // Create workspace for policy lookup.
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-policy"),
            Id::new("tenant-1"),
            "test-workspace",
            "test-ws",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        let policy = state
            .dependency_policies
            .get_for_workspace(&Id::new("ws-policy"))
            .await
            .unwrap();

        assert_eq!(
            policy.breaking_change_behavior,
            gyre_domain::BreakingChangeBehavior::Warn
        );
        assert_eq!(policy.max_version_drift, 3);
        assert_eq!(policy.stale_dependency_alert_days, 30);
        assert!(policy.require_cascade_tests);
        assert!(policy.auto_create_update_tasks);
    }

    #[tokio::test]
    async fn test_dependency_policy_set_and_get() {
        let state = setup();
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-policy-2"),
            Id::new("tenant-1"),
            "test-workspace-2",
            "test-ws-2",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        let custom_policy = gyre_domain::DependencyPolicy {
            breaking_change_behavior: gyre_domain::BreakingChangeBehavior::Block,
            max_version_drift: 5,
            stale_dependency_alert_days: 60,
            require_cascade_tests: false,
            auto_create_update_tasks: false,
        };

        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-policy-2"), &custom_policy)
            .await
            .unwrap();

        let retrieved = state
            .dependency_policies
            .get_for_workspace(&Id::new("ws-policy-2"))
            .await
            .unwrap();

        assert_eq!(
            retrieved.breaking_change_behavior,
            gyre_domain::BreakingChangeBehavior::Block
        );
        assert_eq!(retrieved.max_version_drift, 5);
        assert_eq!(retrieved.stale_dependency_alert_days, 60);
        assert!(!retrieved.require_cascade_tests);
        assert!(!retrieved.auto_create_update_tasks);
    }

    // ── Merge-time enforcement tests ───────────────────────────────────

    #[tokio::test]
    async fn test_merge_blocked_by_breaking_change_policy() {
        // When policy is `block` and there are unacknowledged breaking changes
        // from the MR's repo, the merge processor should fail the queue entry.
        let state = setup();
        let repo_a = create_repo(&state, "merge-block-a").await;
        let repo_b = create_repo(&state, "merge-block-b").await;

        // A depends on B.
        let edge = DependencyEdge::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            repo_a.clone(),
            repo_b.clone(),
            DependencyType::Code,
            "Cargo.toml",
            "crate-b",
            DetectionMethod::CargoToml,
            1000,
        );
        state.dependencies.save(&edge).await.unwrap();

        // Breaking change from B.
        let bc = gyre_domain::BreakingChange::new(
            Id::new("bc-merge-1"),
            edge.id.clone(),
            repo_b.clone(),
            "sha-breaking",
            "removed API",
            2000,
        );
        state.breaking_changes.create(&bc).await.unwrap();

        // Create workspace with block policy.
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-block"),
            Id::new("tenant-1"),
            "workspace-block",
            "ws-block",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        let policy = gyre_domain::DependencyPolicy {
            breaking_change_behavior: gyre_domain::BreakingChangeBehavior::Block,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-block"), &policy)
            .await
            .unwrap();

        // Create an MR for repo_b in workspace ws-block.
        let mr = gyre_domain::MergeRequest::new(
            Id::new("mr-block-1"),
            repo_b.clone(),
            "Test MR",
            "feat/test",
            "main",
            0,
        );
        let mut mr = mr;
        mr.workspace_id = Id::new("ws-block");
        state.merge_requests.create(&mr).await.unwrap();

        // Enqueue the MR.
        let entry = gyre_domain::MergeQueueEntry::new(
            Id::new("qe-block-1"),
            Id::new("mr-block-1"),
            50,
            3000,
        );
        state.merge_queue.enqueue(&entry).await.unwrap();

        // Run merge processor.
        crate::merge_processor::run_once(&state).await.unwrap();

        // The entry should be failed due to blocking policy.
        let updated = state
            .merge_queue
            .find_by_id(&Id::new("qe-block-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, gyre_domain::MergeQueueEntryStatus::Failed);
        assert!(updated
            .error_message
            .as_ref()
            .unwrap()
            .contains("unacknowledged breaking change"));
    }

    #[tokio::test]
    async fn test_merge_proceeds_after_acknowledgment() {
        // When all breaking changes are acknowledged, the merge should not be blocked.
        let state = setup();
        let repo_a = create_repo(&state, "ack-merge-a").await;
        let repo_b = create_repo(&state, "ack-merge-b").await;

        let edge = DependencyEdge::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            repo_a.clone(),
            repo_b.clone(),
            DependencyType::Code,
            "Cargo.toml",
            "crate-b",
            DetectionMethod::CargoToml,
            1000,
        );
        state.dependencies.save(&edge).await.unwrap();

        // Breaking change, already acknowledged.
        let mut bc = gyre_domain::BreakingChange::new(
            Id::new("bc-ack-merge-1"),
            edge.id.clone(),
            repo_b.clone(),
            "sha-ok",
            "acknowledged change",
            2000,
        );
        bc.acknowledged = true;
        bc.acknowledged_by = Some("user-1".to_string());
        bc.acknowledged_at = Some(2500);
        state.breaking_changes.create(&bc).await.unwrap();

        // Create workspace with block policy.
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-ack"),
            Id::new("tenant-1"),
            "workspace-ack",
            "ws-ack",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        let policy = gyre_domain::DependencyPolicy {
            breaking_change_behavior: gyre_domain::BreakingChangeBehavior::Block,
            ..Default::default()
        };
        state
            .dependency_policies
            .set_for_workspace(&Id::new("ws-ack"), &policy)
            .await
            .unwrap();

        // Create an MR and enqueue.
        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-ack-1"),
            repo_b.clone(),
            "Test MR",
            "feat/test",
            "main",
            0,
        );
        mr.workspace_id = Id::new("ws-ack");
        state.merge_requests.create(&mr).await.unwrap();

        let entry =
            gyre_domain::MergeQueueEntry::new(Id::new("qe-ack-1"), Id::new("mr-ack-1"), 50, 3000);
        state.merge_queue.enqueue(&entry).await.unwrap();

        // Run merge processor — should NOT be blocked (all acknowledged).
        crate::merge_processor::run_once(&state).await.unwrap();

        // The entry should NOT be failed (it will fail at the git merge step
        // since there's no real git repo, but that's a different failure).
        let updated = state
            .merge_queue
            .find_by_id(&Id::new("qe-ack-1"))
            .await
            .unwrap()
            .unwrap();
        // It should be Processing or Failed (git merge failure), NOT failed
        // with breaking-change message.
        if let Some(ref msg) = updated.error_message {
            assert!(
                !msg.contains("unacknowledged breaking change"),
                "merge should not have been blocked by acknowledged breaking change"
            );
        }
    }

    #[tokio::test]
    async fn test_merge_warn_policy_does_not_block() {
        // With warn policy, merge should proceed despite unacknowledged changes.
        let state = setup();
        let repo_a = create_repo(&state, "warn-a").await;
        let repo_b = create_repo(&state, "warn-b").await;

        let edge = DependencyEdge::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            repo_a.clone(),
            repo_b.clone(),
            DependencyType::Code,
            "Cargo.toml",
            "crate-b",
            DetectionMethod::CargoToml,
            1000,
        );
        state.dependencies.save(&edge).await.unwrap();

        // Unacknowledged breaking change.
        let bc = gyre_domain::BreakingChange::new(
            Id::new("bc-warn-1"),
            edge.id.clone(),
            repo_b.clone(),
            "sha-break",
            "breaking stuff",
            2000,
        );
        state.breaking_changes.create(&bc).await.unwrap();

        // Warn policy (default).
        let ws = gyre_domain::Workspace::new(
            Id::new("ws-warn"),
            Id::new("tenant-1"),
            "workspace-warn",
            "ws-warn",
            0,
        );
        state.workspaces.create(&ws).await.unwrap();
        // Default policy is Warn — no need to set explicitly.

        let mut mr = gyre_domain::MergeRequest::new(
            Id::new("mr-warn-1"),
            repo_b.clone(),
            "Test MR",
            "feat/test",
            "main",
            0,
        );
        mr.workspace_id = Id::new("ws-warn");
        state.merge_requests.create(&mr).await.unwrap();

        let entry =
            gyre_domain::MergeQueueEntry::new(Id::new("qe-warn-1"), Id::new("mr-warn-1"), 50, 3000);
        state.merge_queue.enqueue(&entry).await.unwrap();

        // Run merge processor — should NOT be blocked under warn policy.
        crate::merge_processor::run_once(&state).await.unwrap();

        let updated = state
            .merge_queue
            .find_by_id(&Id::new("qe-warn-1"))
            .await
            .unwrap()
            .unwrap();
        // Should not fail with breaking change message.
        if let Some(ref msg) = updated.error_message {
            assert!(
                !msg.contains("unacknowledged breaking change"),
                "warn policy should not block merge"
            );
        }
    }

    // ── Auto-task creation tests ───────────────────────────────────────

    #[tokio::test]
    async fn test_breaking_change_auto_creates_task() {
        // Invoke the production code path (process_breaking_changes) to verify
        // that breaking change records, edge status updates, and tasks are created.
        // Uses distinct workspace IDs to verify scope resolution per checklist item 74.
        let state = setup();
        // Repo A (dependent) is in ws-dependent; Repo B (depended-on/pushed) is in ws-pushed.
        let repo_a = create_repo_in_workspace(&state, "task-auto-a", "ws-dependent").await;
        let repo_b = create_repo_in_workspace(&state, "task-auto-b", "ws-pushed").await;

        // A depends on B.
        let edge = DependencyEdge::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            repo_a.clone(),
            repo_b.clone(),
            DependencyType::Code,
            "Cargo.toml",
            "crate-b",
            DetectionMethod::CargoToml,
            1000,
        );
        state.dependencies.save(&edge).await.unwrap();

        let breaking_commits = vec![(
            "sha-break".to_string(),
            "removed deprecated API".to_string(),
        )];
        let dependents = vec![edge.clone()];
        let policy = gyre_domain::DependencyPolicy::default(); // auto_create_update_tasks = true

        // Call the production function with the PUSHED repo's workspace.
        // The function must resolve the dependent repo's workspace internally.
        crate::git_http::process_breaking_changes(
            &state,
            &breaking_commits,
            &dependents,
            repo_b.as_str(),
            "task-auto-b",
            "ws-pushed",
            "tenant-1",
            &policy,
            2000,
        )
        .await;

        // Verify BreakingChange record was created.
        let bcs = state
            .breaking_changes
            .list_by_source_repo(&repo_b)
            .await
            .unwrap();
        assert_eq!(bcs.len(), 1);
        assert_eq!(bcs[0].commit_sha, "sha-break");
        assert_eq!(bcs[0].description, "removed deprecated API");

        // Verify the dependency edge status was updated to Breaking.
        // The edge has source=repo_a (dependent), target=repo_b (depended-on).
        let updated_edge = state
            .dependencies
            .find_by_id(&edge.id)
            .await
            .unwrap()
            .expect("edge should exist");
        assert_eq!(updated_edge.status, DependencyStatus::Breaking);

        // Verify the task was created for the DEPENDENT repo (A = source_repo_id on the edge).
        let tasks = state.tasks.list_by_repo(&repo_a).await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert!(tasks[0].title.contains("Breaking change"));
        assert!(tasks[0].title.contains("task-auto-b"));
        assert_eq!(tasks[0].priority, gyre_domain::TaskPriority::High);
        assert!(tasks[0].labels.contains(&"breaking-change".to_string()));
        assert!(tasks[0].labels.contains(&"auto-created".to_string()));
        assert!(tasks[0].labels.contains(&"dependency-update".to_string()));
        // Verify the task is in the DEPENDENT repo's workspace, not the pushed repo's.
        assert_eq!(tasks[0].workspace_id.as_str(), "ws-dependent");
    }

    // ── Acknowledgment clears edge status test ─────────────────────────

    #[tokio::test]
    async fn test_acknowledge_reverts_edge_status_to_active() {
        let state = setup();
        let a = create_repo(&state, "revert-a").await;
        let b = create_repo(&state, "revert-b").await;

        let edge_id = Id::new(uuid::Uuid::new_v4().to_string());
        let mut edge = DependencyEdge::new(
            edge_id.clone(),
            a.clone(),
            b.clone(),
            DependencyType::Code,
            "Cargo.toml",
            "crate-b",
            DetectionMethod::CargoToml,
            1000,
        );
        edge.status = DependencyStatus::Breaking;
        state.dependencies.save(&edge).await.unwrap();

        let bc = gyre_domain::BreakingChange::new(
            Id::new("bc-revert-1"),
            edge_id.clone(),
            b.clone(),
            "sha-break",
            "breaking",
            2000,
        );
        state.breaking_changes.create(&bc).await.unwrap();

        // Call the acknowledge handler logic.
        let ack_result =
            acknowledge_breaking_change(State(state.clone()), Path("bc-revert-1".to_string()))
                .await;
        assert!(ack_result.is_ok());

        // Verify the edge status is reverted to Active.
        let updated_edge = state
            .dependencies
            .find_by_id(&edge_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated_edge.status, DependencyStatus::Active);
    }

    // ── Acknowledge nonexistent returns 404 ────────────────────────────

    #[tokio::test]
    async fn test_acknowledge_nonexistent_returns_not_found() {
        let state = setup();
        let result =
            acknowledge_breaking_change(State(state.clone()), Path("nonexistent-id".to_string()))
                .await;
        assert!(result.is_err());
    }

    // ── List breaking changes endpoint test ────────────────────────────

    #[tokio::test]
    async fn test_list_breaking_changes_empty() {
        let state = setup();
        let result = list_breaking_changes(State(state.clone())).await;
        assert!(result.is_ok());
        let Json(changes) = result.ok().expect("already checked");
        assert!(changes.is_empty());
    }

    #[tokio::test]
    async fn test_list_breaking_changes_filters_acknowledged() {
        let state = setup();
        let a = create_repo(&state, "filter-a").await;
        let b = create_repo(&state, "filter-b").await;

        let edge = DependencyEdge::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            a.clone(),
            b.clone(),
            DependencyType::Code,
            "Cargo.toml",
            "crate-b",
            DetectionMethod::CargoToml,
            1000,
        );
        state.dependencies.save(&edge).await.unwrap();

        // One unacknowledged.
        let bc1 = gyre_domain::BreakingChange::new(
            Id::new("bc-filter-1"),
            edge.id.clone(),
            b.clone(),
            "sha1",
            "change 1",
            2000,
        );
        state.breaking_changes.create(&bc1).await.unwrap();

        // One acknowledged.
        let mut bc2 = gyre_domain::BreakingChange::new(
            Id::new("bc-filter-2"),
            edge.id.clone(),
            b.clone(),
            "sha2",
            "change 2",
            2000,
        );
        bc2.acknowledged = true;
        bc2.acknowledged_by = Some("user".to_string());
        bc2.acknowledged_at = Some(2500);
        state.breaking_changes.create(&bc2).await.unwrap();

        let result = list_breaking_changes(State(state.clone())).await;
        assert!(result.is_ok());
        let Json(changes) = result.ok().expect("already checked");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].commit_sha, "sha1");
    }

    // ── Stale dependencies endpoint tests (TASK-021) ──────────────────

    #[tokio::test]
    async fn test_list_stale_dependencies_returns_only_stale() {
        let state = setup();
        let a = create_repo(&state, "stale-src").await;
        let b = create_repo(&state, "stale-tgt").await;

        // One active edge.
        let active_edge = DependencyEdge::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            a.clone(),
            b.clone(),
            DependencyType::Code,
            "Cargo.toml",
            "crate-b",
            DetectionMethod::CargoToml,
            1000,
        );
        state.dependencies.save(&active_edge).await.unwrap();

        // One stale edge.
        let mut stale_edge = DependencyEdge::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            a.clone(),
            b.clone(),
            DependencyType::Code,
            "Cargo.toml",
            "crate-b-old",
            DetectionMethod::CargoToml,
            1000,
        );
        stale_edge.status = DependencyStatus::Stale;
        stale_edge.version_drift = Some(5);
        stale_edge.version_pinned = Some("1.0.0".to_string());
        stale_edge.target_version_current = Some("v1.5.0".to_string());
        state.dependencies.save(&stale_edge).await.unwrap();

        let query = axum::extract::Query(StaleQuery { workspace_id: None });
        let result = list_stale_dependencies(State(state.clone()), query).await;
        assert!(result.is_ok());
        let Json(stale_list) = result.ok().unwrap();
        assert_eq!(stale_list.len(), 1);
        assert_eq!(stale_list[0].target_artifact, "crate-b-old");
        assert_eq!(stale_list[0].version_drift, Some(5));
        assert_eq!(
            stale_list[0].target_version_current,
            Some("v1.5.0".to_string())
        );
    }

    #[tokio::test]
    async fn test_list_stale_dependencies_empty_when_none() {
        let state = setup();
        let a = create_repo(&state, "no-stale-src").await;
        let b = create_repo(&state, "no-stale-tgt").await;

        let edge = DependencyEdge::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            a,
            b,
            DependencyType::Code,
            "Cargo.toml",
            "crate-b",
            DetectionMethod::CargoToml,
            1000,
        );
        state.dependencies.save(&edge).await.unwrap();

        let query = axum::extract::Query(StaleQuery { workspace_id: None });
        let result = list_stale_dependencies(State(state.clone()), query).await;
        assert!(result.is_ok());
        let Json(stale_list) = result.ok().unwrap();
        assert!(stale_list.is_empty());
    }

    // ── Cargo.toml version extraction tests (TASK-021) ────────────────

    #[test]
    fn test_extract_cargo_version() {
        let toml = r#"
[package]
name = "my-crate"
version = "1.2.3"

[dependencies]
serde = "1.0"
"#;
        assert_eq!(
            crate::git_http::extract_cargo_version(toml),
            Some("1.2.3".to_string())
        );
    }

    #[test]
    fn test_extract_cargo_version_no_package() {
        let toml = r#"
[dependencies]
serde = "1.0"
"#;
        assert_eq!(crate::git_http::extract_cargo_version(toml), None);
    }

    #[test]
    fn test_extract_dep_version_simple() {
        let toml = r#"
[dependencies]
serde = "1.0.200"
other-crate = "2.3.4"
"#;
        assert_eq!(
            crate::git_http::extract_dep_version(toml, "serde"),
            Some("1.0.200".to_string())
        );
        assert_eq!(
            crate::git_http::extract_dep_version(toml, "other-crate"),
            Some("2.3.4".to_string())
        );
    }

    #[test]
    fn test_extract_dep_version_inline_table() {
        let toml = r#"
[dependencies]
serde = { version = "1.0.200", features = ["derive"] }
"#;
        assert_eq!(
            crate::git_http::extract_dep_version(toml, "serde"),
            Some("1.0.200".to_string())
        );
    }

    #[test]
    fn test_extract_dep_version_not_found() {
        let toml = r#"
[dependencies]
serde = "1.0"
"#;
        assert_eq!(crate::git_http::extract_dep_version(toml, "tokio"), None);
    }
}
