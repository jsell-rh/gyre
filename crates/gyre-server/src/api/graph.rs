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
//!   GET  /api/v1/workspaces/{id}/briefing      — HSI briefing summary (?since=)
//!   POST /api/v1/workspaces/{id}/briefing/ask  — LLM Q&A grounded in briefing (SSE)
//!   POST /api/v1/repos/{id}/graph/link         — manually link node to spec
//!   GET  /api/v1/repos/{id}/graph/predict      — structural prediction stub

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::{stream, StreamExt as _};
use gyre_common::{
    graph::{ArchitecturalDelta, EdgeType, GraphEdge, GraphNode, NodeType, SpecConfidence},
    Id,
};
use gyre_domain::{MrStatus, TaskStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use super::{error::ApiError, new_id, now_secs};
use crate::{
    auth::AuthenticatedAgent,
    llm_rate_limit::{check_rate_limit, LLM_RATE_LIMIT, LLM_WINDOW_SECS},
    AppState,
};

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
    pub test_coverage: Option<f64>,
    /// Unix timestamp when this node first appeared in any extraction.
    pub first_seen_at: u64,
    /// Unix timestamp of the most recent extraction that included this node.
    pub last_seen_at: u64,
    /// Set when a node is no longer present in extraction (soft-delete). `None` = active.
    pub deleted_at: Option<u64>,
    /// Whether this node is a test function/class (for structural test coverage analysis).
    pub test_node: bool,
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
            test_coverage: n.test_coverage,
            first_seen_at: n.first_seen_at,
            last_seen_at: n.last_seen_at,
            deleted_at: n.deleted_at,
            test_node: n.test_node,
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
    /// Unix timestamp when this edge first appeared in any extraction.
    pub first_seen_at: u64,
    /// Unix timestamp of the most recent extraction that included this edge.
    pub last_seen_at: u64,
    /// Set when an edge is no longer present in extraction (soft-delete). `None` = active.
    pub deleted_at: Option<u64>,
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
            first_seen_at: e.first_seen_at,
            last_seen_at: e.last_seen_at,
            deleted_at: e.deleted_at,
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
pub struct RepoGraphQuery {
    /// Optional case-insensitive substring filter on node name / qualified_name.
    pub concept: Option<String>,
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

/// One completed-agent entry for the Briefing "Completed" section (HSI §4).
#[derive(Serialize)]
pub struct BriefingCompletedAgent {
    pub agent_id: String,
    pub spec_ref: Option<String>,
    pub decisions: Vec<serde_json::Value>,
    pub uncertainties: Vec<String>,
    pub conversation_sha: Option<String>,
    pub completed_at: u64,
}

/// HSI §9 briefing response schema.
#[derive(Serialize)]
pub struct BriefingResponse {
    pub workspace_id: String,
    pub since: u64,
    pub completed: Vec<BriefingItem>,
    pub in_progress: Vec<BriefingItem>,
    pub cross_workspace: Vec<BriefingItem>,
    pub exceptions: Vec<BriefingItem>,
    pub metrics: BriefingMetrics,
    /// LLM-synthesized narrative (stubbed for now).
    pub summary: String,
    /// Completed agents with their decisions and uncertainties (HSI §4).
    pub completed_agents: Vec<BriefingCompletedAgent>,
}

#[derive(Serialize)]
pub struct BriefingItem {
    pub title: String,
    pub description: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub spec_path: Option<String>,
    pub timestamp: u64,
}

#[derive(Serialize)]
pub struct BriefingMetrics {
    pub mrs_merged: u32,
    pub gate_runs: u32,
    pub budget_spent_usd: f64,
    pub budget_pct: u32,
}

#[derive(Deserialize)]
pub struct BriefingAskRequest {
    pub question: String,
    pub history: Option<Vec<HistoryEntry>>,
}

#[derive(Deserialize, Serialize)]
pub struct HistoryEntry {
    pub role: String,
    pub content: String,
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
/// Optional `?concept=<substring>` filters nodes by case-insensitive substring
/// match on `name` or `qualified_name`.
pub async fn get_repo_graph(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<RepoGraphQuery>,
) -> Result<Json<KnowledgeGraphResponse>, ApiError> {
    require_repo(&state, &id).await?;
    let repo_id = Id::new(&id);

    let all_nodes = state
        .graph_store
        .list_nodes(&repo_id, None)
        .await
        .map_err(ApiError::Internal)?;

    let nodes: Vec<GraphNode> = if let Some(pattern) = &q.concept {
        let pat = pattern.to_lowercase();
        all_nodes
            .into_iter()
            .filter(|n| {
                n.name.to_lowercase().contains(&pat)
                    || n.qualified_name.to_lowercase().contains(&pat)
            })
            .collect()
    } else {
        all_nodes
    };

    let all_edges = state
        .graph_store
        .list_edges(&repo_id, None)
        .await
        .map_err(ApiError::Internal)?;

    // When concept filtering is active, restrict edges to those where both
    // endpoints are in the matched node set (consistent with /graph/concept/:name).
    let edges: Vec<GraphEdge> = if q.concept.is_some() {
        let node_ids: std::collections::HashSet<String> =
            nodes.iter().map(|n| n.id.to_string()).collect();
        all_edges
            .into_iter()
            .filter(|e| {
                node_ids.contains(e.source_id.as_str()) && node_ids.contains(e.target_id.as_str())
            })
            .collect()
    } else {
        all_edges
    };

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
        .repos
        .list_by_workspace(&Id::new(&id))
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.id.to_string())
        .collect();

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
/// Returns the HSI-defined briefing for a workspace (HSI §9).
/// When `?since=` is omitted, uses `last_seen_at` from `user_workspace_state` as default.
/// Falls back to 24 hours ago if no row exists (first visit). Always returns 200.
pub async fn get_workspace_briefing(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(id): Path<String>,
    Query(q): Query<BriefingQuery>,
) -> Result<Json<BriefingResponse>, ApiError> {
    require_workspace(&state, &id).await?;

    // Resolve `since`: explicit param > last_seen_at from user_workspace_state > 24h fallback.
    let since: u64 = if let Some(s) = q.since {
        s
    } else if let Some(uid) = &auth.user_id {
        let last_seen = state
            .user_workspace_state
            .get_last_seen(uid.as_str(), &id)
            .await
            .unwrap_or(None);
        last_seen
            .map(|ts| ts as u64)
            .unwrap_or_else(|| now_secs().saturating_sub(24 * 3600))
    } else {
        now_secs().saturating_sub(24 * 3600)
    };

    // Collect MRs and tasks for this workspace.
    let workspace_id = Id::new(&id);
    let all_mrs = state
        .merge_requests
        .list_by_workspace(&workspace_id)
        .await
        .unwrap_or_default();
    let all_tasks = state
        .tasks
        .list_by_workspace(&workspace_id)
        .await
        .unwrap_or_default();

    // Section: completed — MRs with status Merged updated since `since`.
    let completed: Vec<BriefingItem> = all_mrs
        .iter()
        .filter(|mr| mr.status == MrStatus::Merged && mr.updated_at >= since)
        .map(|mr| BriefingItem {
            title: mr.title.clone(),
            description: format!("{} → {}", mr.source_branch, mr.target_branch),
            entity_type: "mr".to_string(),
            entity_id: Some(mr.id.to_string()),
            spec_path: mr
                .spec_ref
                .as_ref()
                .map(|s| s.split('@').next().unwrap_or(s).to_string()),
            timestamp: mr.updated_at,
        })
        .collect();

    // Section: in_progress — tasks with status InProgress or Review updated since `since`.
    let in_progress: Vec<BriefingItem> = all_tasks
        .iter()
        .filter(|t| {
            (t.status == TaskStatus::InProgress || t.status == TaskStatus::Review)
                && t.updated_at >= since
        })
        .map(|t| BriefingItem {
            title: t.title.clone(),
            description: t.description.clone().unwrap_or_default(),
            entity_type: "task".to_string(),
            entity_id: Some(t.id.to_string()),
            spec_path: t.spec_path.clone(),
            timestamp: t.updated_at,
        })
        .collect();

    // Section: cross_workspace — stub (empty for now).
    let cross_workspace: Vec<BriefingItem> = Vec::new();

    // Section: exceptions — stub (empty for now, future: gate failures).
    let exceptions: Vec<BriefingItem> = Vec::new();

    // Metrics: count merged MRs since `since`.
    let mrs_merged = completed.len() as u32;
    let metrics = BriefingMetrics {
        mrs_merged,
        gate_runs: 0,
        budget_spent_usd: 0.0,
        budget_pct: 0,
    };

    // ── Completed agents section (HSI §4) ────────────────────────────────────
    // Read AgentCompleted Event-tier messages from the message bus for this workspace.
    let ws_id_obj = Id::new(&id);
    let since_ms = since.saturating_mul(1000); // convert epoch seconds to milliseconds
    let completed_msgs = state
        .messages
        .list_by_workspace(
            &ws_id_obj,
            Some("agent_completed"),
            Some(since_ms),
            None,
            None,
            Some(50),
        )
        .await
        .unwrap_or_default();

    let completed_agents: Vec<BriefingCompletedAgent> = completed_msgs
        .into_iter()
        .filter_map(|msg| {
            let payload = msg.payload?;
            let agent_id = payload.get("agent_id")?.as_str()?.to_string();
            let spec_ref = payload
                .get("spec_ref")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let decisions = payload
                .get("decisions")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let uncertainties = payload
                .get("uncertainties")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let conversation_sha = payload
                .get("conversation_sha")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            // created_at is epoch milliseconds per message bus convention
            let completed_at = msg.created_at / 1000;
            Some(BriefingCompletedAgent {
                agent_id,
                spec_ref,
                decisions,
                uncertainties,
                conversation_sha,
                completed_at,
            })
        })
        .collect();

    // Build human-readable summary string.
    let summary = {
        let since_str = {
            let now = now_secs();
            let diff = now.saturating_sub(since);
            if diff < 60 {
                "just now".to_string()
            } else if diff < 3600 {
                let m = diff / 60;
                format!("{m} minute{} ago", if m == 1 { "" } else { "s" })
            } else if diff < 86400 {
                let h = diff / 3600;
                format!("{h} hour{} ago", if h == 1 { "" } else { "s" })
            } else {
                let d = diff / 86400;
                format!("{d} day{} ago", if d == 1 { "" } else { "s" })
            }
        };
        let mr_word = if mrs_merged == 1 { "MR" } else { "MRs" };
        let task_count = in_progress.len();
        let task_word = if task_count == 1 { "task" } else { "tasks" };
        format!(
            "{mrs_merged} {mr_word} merged, {task_count} {task_word} in progress since {since_str}",
        )
    };

    Ok(Json(BriefingResponse {
        workspace_id: id,
        since,
        completed,
        in_progress,
        cross_workspace,
        exceptions,
        metrics,
        summary,
        completed_agents,
    }))
}

/// POST /api/v1/workspaces/{id}/briefing/ask
/// SSE streaming Q&A grounded in briefing data (HSI §9). ABAC: workspace/generate.
pub async fn briefing_ask(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    caller: AuthenticatedAgent,
    Json(mut req): Json<BriefingAskRequest>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>>, ApiError>
{
    require_workspace(&state, &id).await?;

    // Per-user/workspace sliding-window rate limit (HSI §6): 10 req/60 s.
    {
        let mut limiter = state.llm_rate_limiter.lock().await;
        if let Err(retry_after) = check_rate_limit(
            &mut limiter,
            &caller.agent_id,
            &id,
            LLM_RATE_LIMIT,
            LLM_WINDOW_SECS,
        ) {
            return Err(ApiError::RateLimited(retry_after));
        }
    }

    // Cap history at 20 entries (truncate oldest).
    if let Some(ref mut history) = req.history {
        if history.len() > 20 {
            let excess = history.len() - 20;
            history.drain(..excess);
        }
    }

    // Require LLM to be configured.
    let factory = state.llm.as_ref().ok_or(ApiError::LlmUnavailable)?;

    let workspace_id_obj = Id::new(&id);

    // Load effective prompt; fall back to hardcoded default.
    let template_content = state
        .prompt_templates
        .get_effective(&workspace_id_obj, "briefing-ask")
        .await
        .map_err(ApiError::Internal)?
        .map(|t| t.content)
        .unwrap_or_else(|| crate::llm_defaults::PROMPT_BRIEFING_ASK.to_string());

    let system_prompt = template_content
        .replace("{{workspace_id}}", &id)
        .replace("{{context}}", "")
        .replace("{{question}}", &req.question);
    let user_prompt = req.question.clone();

    // Resolve model and call streaming LLM.
    let (model, _) =
        crate::llm_helpers::resolve_llm_model(&state, &Id::new(&id), "briefing-ask").await;
    let stream = factory
        .for_model(&model)
        .stream_complete(&system_prompt, &user_prompt, None)
        .await
        .map_err(ApiError::Internal)?;

    let chunks: Vec<String> = stream.filter_map(|r| async { r.ok() }).collect().await;
    let full_text = chunks.join("");

    let mut events: Vec<Result<Event, std::convert::Infallible>> = Vec::new();
    for chunk in &chunks {
        let data = serde_json::to_string(&serde_json::json!({"text": chunk})).unwrap_or_default();
        events.push(Ok(Event::default().event("partial").data(data)));
    }
    let complete_data =
        serde_json::to_string(&serde_json::json!({"text": full_text})).unwrap_or_default();
    events.push(Ok(Event::default().event("complete").data(complete_data)));

    Ok(Sse::new(stream::iter(events)).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    ))
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

/// GET /workspaces/{id}/graph/concept/{name}
/// Workspace-scoped concept search — filters nodes across all repos in the workspace
/// by case-insensitive substring match on `name` or `qualified_name`.
///
/// This avoids downloading the full workspace graph for concept queries.
pub async fn get_workspace_graph_concept(
    State(state): State<Arc<AppState>>,
    Path((id, concept_name)): Path<(String, String)>,
) -> Result<Json<KnowledgeGraphResponse>, ApiError> {
    require_workspace(&state, &id).await?;
    let pattern = concept_name.to_lowercase();

    let repo_ids: Vec<String> = state
        .repos
        .list_by_workspace(&Id::new(&id))
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.id.to_string())
        .collect();

    let mut matched_nodes = Vec::new();
    let mut matched_edges = Vec::new();

    for rid in &repo_ids {
        let repo_id = Id::new(rid);
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

        matched_nodes.extend(nodes);
        matched_edges.extend(edges);
    }

    Ok(Json(KnowledgeGraphResponse {
        repo_id: id,
        nodes: matched_nodes.into_iter().map(Into::into).collect(),
        edges: matched_edges.into_iter().map(Into::into).collect(),
    }))
}

/// GET /api/v1/repos/{id}/graph/predict (legacy compat)
/// POST /api/v1/repos/{id}/graph/predict
/// Structural prediction via LLM — analyzes graph nodes and returns structured predictions.
/// Request body (POST): `{spec_path, draft_content}` — reserved for future implementation.
pub async fn predict_graph(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<PredictResponse>, ApiError> {
    // Load repo to get workspace_id.
    let repo = state
        .repos
        .find_by_id(&Id::new(&id))
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("repo {id} not found")))?;

    // Require LLM to be configured.
    let factory = state.llm.as_ref().ok_or(ApiError::LlmUnavailable)?;

    let workspace_id = repo.workspace_id.clone();

    // Load effective prompt; fall back to hardcoded default.
    let template_content = state
        .prompt_templates
        .get_effective(&workspace_id, "graph-predict")
        .await
        .map_err(ApiError::Internal)?
        .map(|t| t.content)
        .unwrap_or_else(|| crate::llm_defaults::PROMPT_GRAPH_PREDICT.to_string());

    // Load graph nodes for context.
    let repo_id = Id::new(&id);
    let nodes = state
        .graph_store
        .list_nodes(&repo_id, None)
        .await
        .map_err(ApiError::Internal)?;

    let nodes_summary: Vec<serde_json::Value> = nodes
        .iter()
        .map(|n| {
            serde_json::json!({
                "name": n.name,
                "qualified_name": n.qualified_name,
                "type": format!("{:?}", n.node_type),
            })
        })
        .collect();
    let nodes_json = serde_json::to_string(&nodes_summary).unwrap_or_else(|_| "[]".to_string());

    let system_prompt = template_content.replace("{{nodes}}", &nodes_json);
    let user_prompt = format!("Predict structural improvements for repo {id}.");

    // Resolve model and call LLM for structured JSON output.
    let (model, _) =
        crate::llm_helpers::resolve_llm_model(&state, &workspace_id, "graph-predict").await;
    let result = factory
        .for_model(&model)
        .predict_json(&system_prompt, &user_prompt)
        .await
        .map_err(ApiError::Internal)?;

    let predictions = if let Some(arr) = result.as_array() {
        arr.clone()
    } else {
        vec![result]
    };

    Ok(Json(PredictResponse {
        repo_id: id,
        predictions,
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
        test_coverage: None,
        first_seen_at: now,
        last_seen_at: now,
        deleted_at: None,
        test_node: false,
    }
}

fn _new_edge(repo_id: &str, source_id: &Id, target_id: &Id, edge_type: EdgeType) -> GraphEdge {
    let now = now_secs();
    GraphEdge {
        id: Id::new(uuid::Uuid::new_v4().to_string()),
        repo_id: Id::new(repo_id),
        source_id: source_id.clone(),
        target_id: target_id.clone(),
        edge_type,
        metadata: None,
        first_seen_at: now,
        last_seen_at: now,
        deleted_at: None,
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
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    fn auth() -> &'static str {
        "Bearer test-token"
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn briefing_ask_rate_limited_after_10_requests() {
        let app = app();

        // Create a workspace so require_workspace passes.
        let ws_body = serde_json::json!({"name": "rate-limit-ws", "tenant_id": "tenant-1"});
        let ws_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&ws_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ws_resp.status(), StatusCode::CREATED);
        let ws_json = body_json(ws_resp).await;
        let ws_id = ws_json["id"].as_str().unwrap().to_string();

        let ask_body = serde_json::json!({"question": "What changed recently?"});

        // First 10 requests must succeed (SSE 200).
        for i in 0..10 {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/workspaces/{ws_id}/briefing/ask"))
                        .header("Authorization", auth())
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&ask_body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "request {i} should succeed");
        }

        // 11th request must be rate-limited.
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/briefing/ask"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&ask_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry_after = resp
            .headers()
            .get("Retry-After")
            .expect("Retry-After header present");
        let secs: u64 = retry_after.to_str().unwrap().parse().unwrap();
        assert!(secs >= 1, "Retry-After must be at least 1 second");
    }

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

    // ── LLM endpoint tests ───────────────────────────────────────────────────

    fn app_no_llm() -> Router {
        let mut s = (*test_state()).clone();
        s.llm = None;
        crate::api::api_router().with_state(std::sync::Arc::new(s))
    }

    #[tokio::test]
    async fn predict_graph_returns_503_when_llm_unavailable() {
        let app = app_no_llm();

        // Create a repo first.
        let create_body = serde_json::json!({
            "name": "predict-test-repo",
            "workspace_id": "ws-predict",
            "tenant_id": "tenant-1",
        });
        let repo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(repo_resp.status(), StatusCode::CREATED);
        let repo_json = body_json(repo_resp).await;
        let repo_id = repo_json["id"].as_str().unwrap().to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/graph/predict"))
                    .header("Authorization", auth())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn predict_graph_with_mock_llm_returns_non_empty_predictions() {
        let app = app();

        // Create a repo.
        let create_body = serde_json::json!({
            "name": "predict-llm-repo",
            "workspace_id": "ws-predict-llm",
            "tenant_id": "tenant-1",
        });
        let repo_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(repo_resp.status(), StatusCode::CREATED);
        let repo_json = body_json(repo_resp).await;
        let repo_id = repo_json["id"].as_str().unwrap().to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/repos/{repo_id}/graph/predict"))
                    .header("Authorization", auth())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let predictions = json["predictions"].as_array().unwrap();
        assert!(
            !predictions.is_empty(),
            "mock LLM should return at least one prediction"
        );
    }

    #[tokio::test]
    async fn briefing_ask_returns_503_when_llm_unavailable() {
        let app = app_no_llm();

        let ws_body = serde_json::json!({"name": "llm-unavail-ws", "tenant_id": "tenant-1"});
        let ws_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&ws_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ws_resp.status(), StatusCode::CREATED);
        let ws_json = body_json(ws_resp).await;
        let ws_id = ws_json["id"].as_str().unwrap().to_string();

        let ask_body = serde_json::json!({"question": "What changed?"});
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/briefing/ask"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&ask_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn briefing_ask_with_mock_llm_streams_sse_events() {
        let app = app();

        let ws_body = serde_json::json!({"name": "llm-ask-ws", "tenant_id": "tenant-1"});
        let ws_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces")
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&ws_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ws_resp.status(), StatusCode::CREATED);
        let ws_json = body_json(ws_resp).await;
        let ws_id = ws_json["id"].as_str().unwrap().to_string();

        let ask_body = serde_json::json!({"question": "What changed recently?"});
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{ws_id}/briefing/ask"))
                    .header("Authorization", auth())
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&ask_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers().get("content-type").unwrap();
        assert!(ct.to_str().unwrap().contains("text/event-stream"));
    }
}
