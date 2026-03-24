//! Knowledge graph API endpoints (realized-model.md §7).
//!
//! Endpoints:
//!   GET  /api/v1/repos/{id}/graph              — full knowledge graph (nodes + edges)
//!   GET  /api/v1/repos/{id}/graph/types        — nodes by NodeType::Type with edges
//!   GET  /api/v1/repos/{id}/graph/modules      — nodes by NodeType::Module + containment
//!   GET  /api/v1/repos/{id}/graph/node/{nid}   — single node + all connected edges
//!   GET  /api/v1/repos/{id}/graph/spec/{path}  — nodes governed by a spec path
//!   GET  /api/v1/repos/{id}/graph/concept/{n}  — concept view by name pattern
//!   GET  /api/v1/repos/{id}/graph/timeline     — architectural deltas (?since=&until=)
//!   GET  /api/v1/repos/{id}/graph/risks        — risk metrics per module
//!   GET  /api/v1/repos/{id}/graph/diff         — graph diff between commits (?from=&to=)
//!   GET  /api/v1/workspaces/{id}/graph         — cross-repo graph for a workspace
//!   GET  /api/v1/workspaces/{id}/briefing      — narrative summary (?since=)
//!   POST /api/v1/repos/{id}/graph/link         — manually link node to spec
//!   GET  /api/v1/repos/{id}/graph/predict      — structural prediction stub

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::{
    graph::{ArchitecturalDelta, EdgeType, GraphEdge, GraphNode, NodeType, SpecConfidence},
    Id,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{error::ApiError, new_id, now_secs};
use crate::AppState;

// ── Response / Request types ─────────────────────────────────────────────────

#[derive(Serialize)]
pub struct GraphNodeResponse {
    pub id: String,
    pub repo_id: String,
    pub node_type: NodeType,
    pub name: String,
    pub qualified_name: String,
    pub file_path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub visibility: gyre_common::graph::Visibility,
    pub doc_comment: Option<String>,
    pub spec_path: Option<String>,
    pub spec_confidence: SpecConfidence,
    pub last_modified_sha: String,
    pub last_modified_by: Option<String>,
    pub last_modified_at: u64,
    pub created_sha: String,
    pub created_at: u64,
    pub complexity: Option<u32>,
    pub churn_count_30d: u32,
}

impl From<GraphNode> for GraphNodeResponse {
    fn from(n: GraphNode) -> Self {
        Self {
            id: n.id.to_string(),
            repo_id: n.repo_id.to_string(),
            node_type: n.node_type,
            name: n.name,
            qualified_name: n.qualified_name,
            file_path: n.file_path,
            line_start: n.line_start,
            line_end: n.line_end,
            visibility: n.visibility,
            doc_comment: n.doc_comment,
            spec_path: n.spec_path,
            spec_confidence: n.spec_confidence,
            last_modified_sha: n.last_modified_sha,
            last_modified_by: n.last_modified_by.map(|id| id.to_string()),
            last_modified_at: n.last_modified_at,
            created_sha: n.created_sha,
            created_at: n.created_at,
            complexity: n.complexity,
            churn_count_30d: n.churn_count_30d,
        }
    }
}

#[derive(Serialize)]
pub struct GraphEdgeResponse {
    pub id: String,
    pub repo_id: String,
    pub source_id: String,
    pub target_id: String,
    pub edge_type: EdgeType,
    pub metadata: Option<String>,
}

impl From<GraphEdge> for GraphEdgeResponse {
    fn from(e: GraphEdge) -> Self {
        Self {
            id: e.id.to_string(),
            repo_id: e.repo_id.to_string(),
            source_id: e.source_id.to_string(),
            target_id: e.target_id.to_string(),
            edge_type: e.edge_type,
            metadata: e.metadata,
        }
    }
}

#[derive(Serialize)]
pub struct KnowledgeGraphResponse {
    pub repo_id: String,
    pub nodes: Vec<GraphNodeResponse>,
    pub edges: Vec<GraphEdgeResponse>,
}

#[derive(Serialize)]
pub struct NodeWithEdgesResponse {
    pub node: GraphNodeResponse,
    pub edges: Vec<GraphEdgeResponse>,
}

#[derive(Deserialize)]
pub struct TimelineQuery {
    pub since: Option<u64>,
    pub until: Option<u64>,
}

#[derive(Serialize)]
pub struct DeltaResponse {
    pub id: String,
    pub repo_id: String,
    pub commit_sha: String,
    pub timestamp: u64,
    pub spec_ref: Option<String>,
    pub agent_id: Option<String>,
    pub delta_json: String,
}

impl From<ArchitecturalDelta> for DeltaResponse {
    fn from(d: ArchitecturalDelta) -> Self {
        Self {
            id: d.id.to_string(),
            repo_id: d.repo_id.to_string(),
            commit_sha: d.commit_sha,
            timestamp: d.timestamp,
            spec_ref: d.spec_ref,
            agent_id: d.agent_id.map(|id| id.to_string()),
            delta_json: d.delta_json,
        }
    }
}

#[derive(Serialize)]
pub struct RiskMetricsResponse {
    pub node_id: String,
    pub name: String,
    pub qualified_name: String,
    pub churn_rate: u32,
    pub fan_out: usize,
    pub fan_in: usize,
    pub complexity: Option<u32>,
    pub spec_covered: bool,
}

#[derive(Deserialize)]
pub struct DiffQuery {
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Serialize)]
pub struct GraphDiffResponse {
    pub from: String,
    pub to: String,
    pub message: String,
    pub deltas: Vec<DeltaResponse>,
}

#[derive(Deserialize)]
pub struct BriefingQuery {
    pub since: Option<u64>,
}

#[derive(Serialize)]
pub struct BriefingResponse {
    pub workspace_id: String,
    pub since: u64,
    pub summary: String,
    pub deltas: Vec<DeltaResponse>,
}

#[derive(Deserialize)]
pub struct LinkNodeRequest {
    pub node_id: String,
    pub spec_path: String,
    pub confidence: Option<String>,
}

#[derive(Serialize)]
pub struct LinkNodeResponse {
    pub node_id: String,
    pub spec_path: String,
    pub confidence: String,
}

#[derive(Serialize)]
pub struct PredictResponse {
    pub repo_id: String,
    pub predictions: Vec<serde_json::Value>,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

async fn require_repo(state: &AppState, repo_id: &str) -> Result<(), ApiError> {
    state
        .repos
        .find_by_id(&Id::new(repo_id))
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;
    Ok(())
}

async fn require_workspace(state: &AppState, ws_id: &str) -> Result<(), ApiError> {
    state
        .workspaces
        .find_by_id(&Id::new(ws_id))
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {ws_id} not found")))?;
    Ok(())
}

fn parse_confidence(s: &str) -> SpecConfidence {
    match s.to_lowercase().as_str() {
        "high" => SpecConfidence::High,
        "medium" => SpecConfidence::Medium,
        "low" => SpecConfidence::Low,
        _ => SpecConfidence::None,
    }
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// GET /api/v1/repos/{id}/graph
/// Returns the full knowledge graph (all nodes + all edges) for a repository.
pub async fn get_repo_graph(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<KnowledgeGraphResponse>, ApiError> {
    require_repo(&state, &id).await?;
    let repo_id = Id::new(&id);

    let nodes = state
        .graph_store
        .list_nodes(&repo_id, None)
        .await
        .map_err(ApiError::Internal)?;

    let edges = state
        .graph_store
        .list_edges(&repo_id, None)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(KnowledgeGraphResponse {
        repo_id: id,
        nodes: nodes.into_iter().map(Into::into).collect(),
        edges: edges.into_iter().map(Into::into).collect(),
    }))
}

/// GET /api/v1/repos/{id}/graph/types
/// Returns all Type nodes (structs, enums, classes) with their edges.
pub async fn get_graph_types(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<KnowledgeGraphResponse>, ApiError> {
    require_repo(&state, &id).await?;
    let repo_id = Id::new(&id);

    let nodes = state
        .graph_store
        .list_nodes(&repo_id, Some(NodeType::Type))
        .await
        .map_err(ApiError::Internal)?;

    // Include edges where at least one endpoint is a Type node.
    let node_ids: std::collections::HashSet<String> =
        nodes.iter().map(|n| n.id.to_string()).collect();

    let all_edges = state
        .graph_store
        .list_edges(&repo_id, None)
        .await
        .map_err(ApiError::Internal)?;

    let edges: Vec<GraphEdge> = all_edges
        .into_iter()
        .filter(|e| {
            node_ids.contains(e.source_id.as_str()) || node_ids.contains(e.target_id.as_str())
        })
        .collect();

    Ok(Json(KnowledgeGraphResponse {
        repo_id: id,
        nodes: nodes.into_iter().map(Into::into).collect(),
        edges: edges.into_iter().map(Into::into).collect(),
    }))
}

/// GET /api/v1/repos/{id}/graph/modules
/// Returns all Module nodes with their containment edges.
pub async fn get_graph_modules(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<KnowledgeGraphResponse>, ApiError> {
    require_repo(&state, &id).await?;
    let repo_id = Id::new(&id);

    let nodes = state
        .graph_store
        .list_nodes(&repo_id, Some(NodeType::Module))
        .await
        .map_err(ApiError::Internal)?;

    let node_ids: std::collections::HashSet<String> =
        nodes.iter().map(|n| n.id.to_string()).collect();

    // Only containment edges between modules.
    let all_edges = state
        .graph_store
        .list_edges(&repo_id, Some(EdgeType::Contains))
        .await
        .map_err(ApiError::Internal)?;

    let edges: Vec<GraphEdge> = all_edges
        .into_iter()
        .filter(|e| {
            node_ids.contains(e.source_id.as_str()) || node_ids.contains(e.target_id.as_str())
        })
        .collect();

    Ok(Json(KnowledgeGraphResponse {
        repo_id: id,
        nodes: nodes.into_iter().map(Into::into).collect(),
        edges: edges.into_iter().map(Into::into).collect(),
    }))
}

/// GET /api/v1/repos/{id}/graph/node/{node_id}
/// Returns a single node and all edges connected to it.
pub async fn get_graph_node(
    State(state): State<Arc<AppState>>,
    Path((id, node_id)): Path<(String, String)>,
) -> Result<Json<NodeWithEdgesResponse>, ApiError> {
    require_repo(&state, &id).await?;
    let nid = Id::new(&node_id);

    let node = state
        .graph_store
        .get_node(&nid)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("node {node_id} not found")))?;

    // Verify node belongs to this repo.
    if node.repo_id.as_str() != id.as_str() {
        return Err(ApiError::NotFound(format!("node {node_id} not found")));
    }

    let edges = state
        .graph_store
        .list_edges_for_node(&nid)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(NodeWithEdgesResponse {
        node: node.into(),
        edges: edges.into_iter().map(Into::into).collect(),
    }))
}

/// GET /api/v1/repos/{id}/graph/spec/{spec_path}
/// Returns all nodes whose spec_path matches the given (URL-decoded) path.
pub async fn get_graph_by_spec(
    State(state): State<Arc<AppState>>,
    Path((id, spec_path)): Path<(String, String)>,
) -> Result<Json<KnowledgeGraphResponse>, ApiError> {
    require_repo(&state, &id).await?;
    let repo_id = Id::new(&id);

    let nodes = state
        .graph_store
        .get_nodes_by_spec(&repo_id, &spec_path)
        .await
        .map_err(ApiError::Internal)?;

    let node_ids: std::collections::HashSet<String> =
        nodes.iter().map(|n| n.id.to_string()).collect();

    let all_edges = state
        .graph_store
        .list_edges(&repo_id, None)
        .await
        .map_err(ApiError::Internal)?;

    let edges: Vec<GraphEdge> = all_edges
        .into_iter()
        .filter(|e| {
            node_ids.contains(e.source_id.as_str()) || node_ids.contains(e.target_id.as_str())
        })
        .collect();

    Ok(Json(KnowledgeGraphResponse {
        repo_id: id,
        nodes: nodes.into_iter().map(Into::into).collect(),
        edges: edges.into_iter().map(Into::into).collect(),
    }))
}

/// GET /api/v1/repos/{id}/graph/concept/{name}
/// Returns nodes matching the concept name pattern (case-insensitive substring match).
///
/// In the full implementation this would use ConceptView definitions from the spec manifest.
/// For now, it matches nodes whose `name` or `qualified_name` contains the concept name.
pub async fn get_graph_concept(
    State(state): State<Arc<AppState>>,
    Path((id, concept_name)): Path<(String, String)>,
) -> Result<Json<KnowledgeGraphResponse>, ApiError> {
    require_repo(&state, &id).await?;
    let repo_id = Id::new(&id);
    let pattern = concept_name.to_lowercase();

    let all_nodes = state
        .graph_store
        .list_nodes(&repo_id, None)
        .await
        .map_err(ApiError::Internal)?;

    let nodes: Vec<GraphNode> = all_nodes
        .into_iter()
        .filter(|n| {
            n.name.to_lowercase().contains(&pattern)
                || n.qualified_name.to_lowercase().contains(&pattern)
        })
        .collect();

    let node_ids: std::collections::HashSet<String> =
        nodes.iter().map(|n| n.id.to_string()).collect();

    let all_edges = state
        .graph_store
        .list_edges(&repo_id, None)
        .await
        .map_err(ApiError::Internal)?;

    let edges: Vec<GraphEdge> = all_edges
        .into_iter()
        .filter(|e| {
            node_ids.contains(e.source_id.as_str()) && node_ids.contains(e.target_id.as_str())
        })
        .collect();

    Ok(Json(KnowledgeGraphResponse {
        repo_id: id,
        nodes: nodes.into_iter().map(Into::into).collect(),
        edges: edges.into_iter().map(Into::into).collect(),
    }))
}

/// GET /api/v1/repos/{id}/graph/timeline
/// Returns architectural deltas, optionally filtered by ?since=<epoch>&until=<epoch>.
pub async fn get_graph_timeline(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<TimelineQuery>,
) -> Result<Json<Vec<DeltaResponse>>, ApiError> {
    require_repo(&state, &id).await?;
    let repo_id = Id::new(&id);

    let deltas = state
        .graph_store
        .list_deltas(&repo_id, q.since, q.until)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(deltas.into_iter().map(Into::into).collect()))
}

/// GET /api/v1/repos/{id}/graph/risks
/// Returns risk metrics per node, computed from graph structure and git history.
pub async fn get_graph_risks(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<RiskMetricsResponse>>, ApiError> {
    require_repo(&state, &id).await?;
    let repo_id = Id::new(&id);

    let all_nodes = state
        .graph_store
        .list_nodes(&repo_id, None)
        .await
        .map_err(ApiError::Internal)?;

    let all_edges = state
        .graph_store
        .list_edges(&repo_id, None)
        .await
        .map_err(ApiError::Internal)?;

    let risks: Vec<RiskMetricsResponse> = all_nodes
        .into_iter()
        .map(|n| {
            let nid = n.id.to_string();
            let fan_out = all_edges
                .iter()
                .filter(|e| e.source_id.as_str() == nid.as_str())
                .count();
            let fan_in = all_edges
                .iter()
                .filter(|e| e.target_id.as_str() == nid.as_str())
                .count();
            RiskMetricsResponse {
                node_id: nid,
                name: n.name,
                qualified_name: n.qualified_name,
                churn_rate: n.churn_count_30d,
                fan_out,
                fan_in,
                complexity: n.complexity,
                spec_covered: n.spec_path.is_some(),
            }
        })
        .collect();

    Ok(Json(risks))
}

/// GET /api/v1/repos/{id}/graph/diff
/// Returns the graph diff between two git commit SHAs (?from=&to=).
///
/// Returns deltas as an approximation; full diff requires the extraction pipeline.
pub async fn get_graph_diff(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<DiffQuery>,
) -> Result<Json<GraphDiffResponse>, ApiError> {
    require_repo(&state, &id).await?;
    let repo_id = Id::new(&id);

    let from = q.from.unwrap_or_else(|| "HEAD~1".to_string());
    let to = q.to.unwrap_or_else(|| "HEAD".to_string());

    let deltas = state
        .graph_store
        .list_deltas(&repo_id, None, None)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(GraphDiffResponse {
        from,
        to,
        message: "full diff requires extraction pipeline; returning all deltas".to_string(),
        deltas: deltas.into_iter().map(Into::into).collect(),
    }))
}

/// GET /api/v1/workspaces/{id}/graph
/// Returns the aggregated knowledge graph across all repos in the workspace.
pub async fn get_workspace_graph(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<KnowledgeGraphResponse>, ApiError> {
    require_workspace(&state, &id).await?;

    let repo_ids: Vec<String> = state
        .kv_store
        .kv_get("workspace_repos", &id)
        .await
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default();

    let mut all_nodes = Vec::new();
    let mut all_edges = Vec::new();

    for rid in &repo_ids {
        let repo_id = Id::new(rid);
        let nodes = state
            .graph_store
            .list_nodes(&repo_id, None)
            .await
            .map_err(ApiError::Internal)?;
        let edges = state
            .graph_store
            .list_edges(&repo_id, None)
            .await
            .map_err(ApiError::Internal)?;
        all_nodes.extend(nodes);
        all_edges.extend(edges);
    }

    Ok(Json(KnowledgeGraphResponse {
        repo_id: id,
        nodes: all_nodes.into_iter().map(Into::into).collect(),
        edges: all_edges.into_iter().map(Into::into).collect(),
    }))
}

/// GET /api/v1/workspaces/{id}/briefing
/// Returns a template-based narrative summary of recent architectural changes.
pub async fn get_workspace_briefing(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<BriefingQuery>,
) -> Result<Json<BriefingResponse>, ApiError> {
    require_workspace(&state, &id).await?;

    let since = q.since.unwrap_or(0);

    let repo_ids: Vec<String> = state
        .kv_store
        .kv_get("workspace_repos", &id)
        .await
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default();

    let mut all_deltas = Vec::new();
    for rid in &repo_ids {
        let repo_id = Id::new(rid);
        let deltas = state
            .graph_store
            .list_deltas(&repo_id, Some(since), None)
            .await
            .map_err(ApiError::Internal)?;
        all_deltas.extend(deltas);
    }

    // Sort by timestamp, newest first.
    all_deltas.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let summary = if all_deltas.is_empty() {
        format!("No architectural changes since epoch {since}.")
    } else {
        format!(
            "{} architectural delta(s) across {} repo(s) since epoch {since}.",
            all_deltas.len(),
            repo_ids.len(),
        )
    };

    Ok(Json(BriefingResponse {
        workspace_id: id,
        since,
        summary,
        deltas: all_deltas.into_iter().map(Into::into).collect(),
    }))
}

/// POST /api/v1/repos/{id}/graph/link
/// Manually link a graph node to a spec path (human confirmation of suggested links).
pub async fn link_node_to_spec(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<LinkNodeRequest>,
) -> Result<(StatusCode, Json<LinkNodeResponse>), ApiError> {
    require_repo(&state, &id).await?;

    if req.node_id.is_empty() {
        return Err(ApiError::InvalidInput("node_id is required".to_string()));
    }
    if req.spec_path.is_empty() {
        return Err(ApiError::InvalidInput("spec_path is required".to_string()));
    }

    let confidence_str = req.confidence.as_deref().unwrap_or("high");
    let confidence = parse_confidence(confidence_str);
    let nid = Id::new(&req.node_id);

    // Verify node exists and belongs to this repo.
    let node = state
        .graph_store
        .get_node(&nid)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("node {} not found", req.node_id)))?;

    if node.repo_id.as_str() != id.as_str() {
        return Err(ApiError::NotFound(format!(
            "node {} not found",
            req.node_id
        )));
    }

    state
        .graph_store
        .link_node_to_spec(&nid, &req.spec_path, confidence)
        .await
        .map_err(ApiError::Internal)?;

    Ok((
        StatusCode::OK,
        Json(LinkNodeResponse {
            node_id: req.node_id,
            spec_path: req.spec_path,
            confidence: confidence_str.to_string(),
        }),
    ))
}

/// GET /api/v1/repos/{id}/graph/predict
/// Structural prediction stub — returns an empty predictions array.
///
/// The full implementation would accept ?spec_path=&draft= and predict
/// which code elements need to be added/modified to implement a spec change.
pub async fn get_graph_predictions(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<PredictResponse>, ApiError> {
    require_repo(&state, &id).await?;

    Ok(Json(PredictResponse {
        repo_id: id,
        predictions: vec![],
    }))
}

// ── Helper for tests ──────────────────────────────────────────────────────────

fn _new_node(repo_id: &str, name: &str, node_type: NodeType) -> GraphNode {
    let now = now_secs();
    GraphNode {
        id: Id::new(uuid::Uuid::new_v4().to_string()),
        repo_id: Id::new(repo_id),
        node_type,
        name: name.to_string(),
        qualified_name: format!("qualified::{name}"),
        file_path: format!("src/{name}.rs"),
        line_start: 1,
        line_end: 10,
        visibility: gyre_common::graph::Visibility::Public,
        doc_comment: None,
        spec_path: None,
        spec_confidence: SpecConfidence::None,
        last_modified_sha: "abc123".to_string(),
        last_modified_by: None,
        last_modified_at: now,
        created_sha: "abc123".to_string(),
        created_at: now,
        complexity: None,
        churn_count_30d: 0,
    }
}

fn _new_edge(repo_id: &str, source_id: &Id, target_id: &Id, edge_type: EdgeType) -> GraphEdge {
    GraphEdge {
        id: Id::new(uuid::Uuid::new_v4().to_string()),
        repo_id: Id::new(repo_id),
        source_id: source_id.clone(),
        target_id: target_id.clone(),
        edge_type,
        metadata: None,
    }
}

fn _new_delta(repo_id: &str, sha: &str, timestamp: u64) -> ArchitecturalDelta {
    ArchitecturalDelta {
        id: Id::new(new_id().to_string()),
        repo_id: Id::new(repo_id),
        commit_sha: sha.to_string(),
        timestamp,
        agent_id: None,
        spec_ref: None,
        delta_json: "{}".to_string(),
    }
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::test_state;

    #[tokio::test]
    async fn test_save_and_list_nodes() {
        let state = test_state();
        let node = _new_node("repo-1", "MyStruct", NodeType::Type);
        state.graph_store.create_node(node.clone()).await.unwrap();

        let nodes = state
            .graph_store
            .list_nodes(&Id::new("repo-1"), None)
            .await
            .unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "MyStruct");
    }

    #[tokio::test]
    async fn test_filter_by_type() {
        let state = test_state();
        let t = _new_node("repo-filter", "AType", NodeType::Type);
        let m = _new_node("repo-filter", "AModule", NodeType::Module);
        state.graph_store.create_node(t).await.unwrap();
        state.graph_store.create_node(m).await.unwrap();

        let types = state
            .graph_store
            .list_nodes(&Id::new("repo-filter"), Some(NodeType::Type))
            .await
            .unwrap();
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "AType");
    }

    #[tokio::test]
    async fn test_edges_for_node() {
        let state = test_state();
        let a = _new_node("repo-edges", "A", NodeType::Type);
        let b = _new_node("repo-edges", "B", NodeType::Type);
        let aid = a.id.clone();
        let bid = b.id.clone();
        state.graph_store.create_node(a).await.unwrap();
        state.graph_store.create_node(b).await.unwrap();

        let edge = _new_edge("repo-edges", &aid, &bid, EdgeType::Implements);
        state.graph_store.create_edge(edge).await.unwrap();

        let edges = state.graph_store.list_edges_for_node(&aid).await.unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].edge_type, EdgeType::Implements);
    }

    #[tokio::test]
    async fn test_link_node_to_spec() {
        let state = test_state();
        let node = _new_node("repo-link", "SearchService", NodeType::Type);
        let nid = node.id.clone();
        state.graph_store.create_node(node).await.unwrap();

        state
            .graph_store
            .link_node_to_spec(&nid, "specs/system/search.md", SpecConfidence::High)
            .await
            .unwrap();

        let updated = state.graph_store.get_node(&nid).await.unwrap().unwrap();
        assert_eq!(updated.spec_path.as_deref(), Some("specs/system/search.md"));
    }

    #[tokio::test]
    async fn test_deltas() {
        let state = test_state();
        let delta = _new_delta("repo-delta", "abcdef", 1000);
        state.graph_store.record_delta(delta).await.unwrap();

        let deltas = state
            .graph_store
            .list_deltas(&Id::new("repo-delta"), None, None)
            .await
            .unwrap();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].commit_sha, "abcdef");

        // since filter
        let none = state
            .graph_store
            .list_deltas(&Id::new("repo-delta"), Some(2000), None)
            .await
            .unwrap();
        assert!(none.is_empty());
    }

    #[tokio::test]
    async fn test_get_nodes_by_spec() {
        let state = test_state();
        let mut node = _new_node("repo-spec", "SpecNode", NodeType::Interface);
        node.spec_path = Some("specs/search.md".to_string());
        node.spec_confidence = SpecConfidence::High;
        let nid = node.id.clone();
        state.graph_store.create_node(node).await.unwrap();

        let nodes = state
            .graph_store
            .get_nodes_by_spec(&Id::new("repo-spec"), "specs/search.md")
            .await
            .unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].id.as_str(), nid.as_str());
    }
}
