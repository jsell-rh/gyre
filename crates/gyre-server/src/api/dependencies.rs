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
    _admin: crate::auth::AdminOnly,
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
    _admin: crate::auth::AdminOnly,
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

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;

    fn setup() -> Arc<AppState> {
        test_state()
    }

    async fn create_repo(state: &Arc<AppState>, name: &str) -> Id {
        use gyre_domain::Repository;
        let repo = Repository::new(
            Id::new(uuid::Uuid::new_v4().to_string()),
            Id::new("proj-1"),
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
}
