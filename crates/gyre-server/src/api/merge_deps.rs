//! MR dependency graph API (TASK-100)
//!
//! Endpoints:
//!   PUT  /api/v1/merge-requests/{id}/dependencies       — set depends_on list
//!   GET  /api/v1/merge-requests/{id}/dependencies       — list deps + dependents
//!   DELETE /api/v1/merge-requests/{id}/dependencies/{dep_id} — remove one dep
//!   PUT  /api/v1/merge-requests/{id}/atomic-group       — set atomic group
//!   GET  /api/v1/merge-queue/graph                      — full queue DAG

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::MrStatus;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;
use super::now_secs;

// ── Request / Response types ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SetDependenciesRequest {
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Per-dependency detail including source and reason.
#[derive(Serialize)]
pub struct DependencyDetailResponse {
    pub mr_id: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl From<gyre_domain::MergeRequestDependency> for DependencyDetailResponse {
    fn from(dep: gyre_domain::MergeRequestDependency) -> Self {
        let source = match dep.source {
            gyre_domain::DependencySource::Explicit => "explicit",
            gyre_domain::DependencySource::BranchLineage => "branch-lineage",
            gyre_domain::DependencySource::AgentDeclared => "agent-declared",
        };
        Self {
            mr_id: dep.target_mr_id.to_string(),
            source: source.to_string(),
            reason: dep.reason,
        }
    }
}

#[derive(Serialize)]
pub struct DependenciesResponse {
    pub mr_id: String,
    pub depends_on: Vec<DependencyDetailResponse>,
    pub dependents: Vec<String>,
}

#[derive(Deserialize)]
pub struct SetAtomicGroupRequest {
    pub group: Option<String>,
}

#[derive(Serialize)]
pub struct AtomicGroupResponse {
    pub mr_id: String,
    pub atomic_group: Option<String>,
}

#[derive(Serialize)]
pub struct GraphDependencyEdge {
    pub mr_id: String,
    pub source: String,
}

#[derive(Serialize)]
pub struct GraphNode {
    pub mr_id: String,
    pub title: String,
    pub status: String,
    pub priority: u32,
    pub depends_on: Vec<GraphDependencyEdge>,
    pub atomic_group: Option<String>,
}

#[derive(Serialize)]
pub struct QueueGraphResponse {
    pub nodes: Vec<GraphNode>,
}

// ── Cycle detection (DFS) ───────────────────────────────────────────────────

/// Returns `true` if adding edge `from → to` would create a cycle in the
/// current dependency graph (represented as adjacency list `deps`).
pub(crate) fn would_create_cycle(
    from: &str,
    new_deps: &[String],
    all_mrs: &HashMap<String, Vec<String>>,
) -> bool {
    // BFS/DFS from each new dependency: if we can reach `from`, it's a cycle.
    for dep in new_deps {
        if dep == from {
            return true;
        }
        // BFS from dep: if any node reachable from dep equals `from`, cycle.
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<String> = VecDeque::new();
        queue.push_back(dep.clone());
        while let Some(node) = queue.pop_front() {
            if node == from {
                return true;
            }
            if visited.contains(&node) {
                continue;
            }
            visited.insert(node.clone());
            if let Some(node_deps) = all_mrs.get(&node) {
                for d in node_deps {
                    queue.push_back(d.clone());
                }
            }
        }
    }
    false
}

// ── Handlers ─────────────────────────────────────────────────────────────────

pub async fn set_dependencies(
    State(state): State<Arc<AppState>>,
    auth: crate::auth::AuthenticatedAgent,
    Path(id): Path<String>,
    Json(req): Json<SetDependenciesRequest>,
) -> Result<(StatusCode, Json<DependenciesResponse>), ApiError> {
    let mr_id = Id::new(&id);
    let mut mr = state
        .merge_requests
        .find_by_id(&mr_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;

    // Validate all dep IDs exist
    for dep_id_str in &req.depends_on {
        let dep_id = Id::new(dep_id_str);
        if dep_id_str == &id {
            return Err(ApiError::InvalidInput(
                "an MR cannot depend on itself".to_string(),
            ));
        }
        state
            .merge_requests
            .find_by_id(&dep_id)
            .await?
            .ok_or_else(|| {
                ApiError::NotFound(format!("dependency merge request {dep_id_str} not found"))
            })?;
    }

    // Build adjacency map for cycle detection
    let all_mrs = state.merge_requests.list().await?;
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    for m in &all_mrs {
        adj.insert(
            m.id.to_string(),
            m.depends_on
                .iter()
                .map(|d| d.target_mr_id.to_string())
                .collect(),
        );
    }

    if would_create_cycle(&id, &req.depends_on, &adj) {
        return Err(ApiError::InvalidInput(
            "cycle detected: adding these dependencies would create a circular dependency chain"
                .to_string(),
        ));
    }

    // Apply — agent-declared if caller is an agent JWT, explicit if human API key.
    // Agent JWTs have jwt_claims populated; API key callers have jwt_claims = None.
    let source = if auth.jwt_claims.is_some() {
        gyre_domain::DependencySource::AgentDeclared
    } else {
        gyre_domain::DependencySource::Explicit
    };
    mr.depends_on = req
        .depends_on
        .iter()
        .map(|dep_id| {
            let mut dep = gyre_domain::MergeRequestDependency::new(Id::new(dep_id), source.clone());
            dep.reason = req.reason.clone();
            dep
        })
        .collect();
    mr.updated_at = now_secs();
    state.merge_requests.update(&mr).await?;

    let dependents = state
        .merge_requests
        .list_dependents(&mr_id)
        .await?
        .into_iter()
        .map(|id| id.to_string())
        .collect();

    Ok((
        StatusCode::OK,
        Json(DependenciesResponse {
            mr_id: id,
            depends_on: mr
                .depends_on
                .into_iter()
                .map(DependencyDetailResponse::from)
                .collect(),
            dependents,
        }),
    ))
}

pub async fn get_dependencies(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DependenciesResponse>, ApiError> {
    let mr_id = Id::new(&id);
    let mr = state
        .merge_requests
        .find_by_id(&mr_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;

    let dependents = state
        .merge_requests
        .list_dependents(&mr_id)
        .await?
        .into_iter()
        .map(|id| id.to_string())
        .collect();

    Ok(Json(DependenciesResponse {
        mr_id: id,
        depends_on: mr
            .depends_on
            .into_iter()
            .map(DependencyDetailResponse::from)
            .collect(),
        dependents,
    }))
}

pub async fn remove_dependency(
    State(state): State<Arc<AppState>>,
    Path((id, dep_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let mr_id = Id::new(&id);
    let mut mr = state
        .merge_requests
        .find_by_id(&mr_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;

    let before_len = mr.depends_on.len();
    mr.depends_on
        .retain(|d| d.target_mr_id.as_str() != dep_id.as_str());

    if mr.depends_on.len() == before_len {
        return Err(ApiError::NotFound(format!(
            "dependency {dep_id} not found on merge request {id}"
        )));
    }

    mr.updated_at = now_secs();
    state.merge_requests.update(&mr).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_atomic_group(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<SetAtomicGroupRequest>,
) -> Result<Json<AtomicGroupResponse>, ApiError> {
    let mr_id = Id::new(&id);
    let mut mr = state
        .merge_requests
        .find_by_id(&mr_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("merge request {id} not found")))?;

    mr.atomic_group = req.group.clone();
    mr.updated_at = now_secs();
    state.merge_requests.update(&mr).await?;

    Ok(Json(AtomicGroupResponse {
        mr_id: id,
        atomic_group: req.group,
    }))
}

pub async fn get_queue_graph(
    State(state): State<Arc<AppState>>,
) -> Result<Json<QueueGraphResponse>, ApiError> {
    let queue_entries = state.merge_queue.list_queue().await?;

    let mut nodes = Vec::new();
    for entry in &queue_entries {
        if let Ok(Some(mr)) = state
            .merge_requests
            .find_by_id(&entry.merge_request_id)
            .await
        {
            let status = match mr.status {
                MrStatus::Open => "open",
                MrStatus::Approved => "approved",
                MrStatus::Merged => "merged",
                MrStatus::Closed => "closed",
                MrStatus::Reverted => "reverted",
            };
            nodes.push(GraphNode {
                mr_id: mr.id.to_string(),
                title: mr.title,
                status: status.to_string(),
                priority: entry.priority,
                depends_on: mr
                    .depends_on
                    .iter()
                    .map(|d| {
                        let source = match d.source {
                            gyre_domain::DependencySource::Explicit => "explicit",
                            gyre_domain::DependencySource::BranchLineage => "branch-lineage",
                            gyre_domain::DependencySource::AgentDeclared => "agent-declared",
                        };
                        GraphDependencyEdge {
                            mr_id: d.target_mr_id.to_string(),
                            source: source.to_string(),
                        }
                    })
                    .collect(),
                atomic_group: mr.atomic_group,
            });
        }
    }

    Ok(Json(QueueGraphResponse { nodes }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn create_mr(app: Router, title: &str) -> (Router, String) {
        let body = serde_json::json!({
            "repository_id": "repo-1",
            "title": title,
            "source_branch": "feat/x",
            "target_branch": "main"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/merge-requests")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED, "create_mr failed");
        let json = body_json(resp).await;
        let id = json["id"].as_str().unwrap().to_string();
        (app, id)
    }

    #[tokio::test]
    async fn set_and_get_dependencies() {
        let app = app();
        let (app, mr1_id) = create_mr(app, "MR1").await;
        let (app, mr2_id) = create_mr(app, "MR2").await;

        // Set mr2 depends_on mr1
        let body = serde_json::json!({ "depends_on": [mr1_id] });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{mr2_id}/dependencies"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["depends_on"][0]["mr_id"], mr1_id);
        // Global token has no jwt_claims → Explicit source.
        assert_eq!(json["depends_on"][0]["source"], "explicit");

        // GET dependencies for mr1 should show mr2 as dependent
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/merge-requests/{mr1_id}/dependencies"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["dependents"][0], mr2_id);
        assert_eq!(json["depends_on"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn self_dependency_rejected() {
        let app = app();
        let (app, mr_id) = create_mr(app, "Self-dep").await;
        let body = serde_json::json!({ "depends_on": [mr_id] });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{mr_id}/dependencies"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn cycle_detection() {
        let app = app();
        let (app, mr1_id) = create_mr(app, "Cycle A").await;
        let (app, mr2_id) = create_mr(app, "Cycle B").await;

        // mr2 depends_on mr1
        let body = serde_json::json!({ "depends_on": [mr1_id] });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{mr2_id}/dependencies"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Now try mr1 depends_on mr2 — should fail (cycle)
        let body = serde_json::json!({ "depends_on": [mr2_id] });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{mr1_id}/dependencies"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = body_json(resp).await;
        assert!(json["error"].as_str().unwrap().contains("cycle"));
    }

    #[tokio::test]
    async fn transitive_cycle_detection() {
        let app = app();
        let (app, mr1_id) = create_mr(app, "Trans A").await;
        let (app, mr2_id) = create_mr(app, "Trans B").await;
        let (app, mr3_id) = create_mr(app, "Trans C").await;

        // mr2 -> mr1, mr3 -> mr2
        let body = serde_json::json!({ "depends_on": [mr1_id] });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{mr2_id}/dependencies"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = serde_json::json!({ "depends_on": [mr2_id] });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{mr3_id}/dependencies"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // mr1 -> mr3 would create cycle: mr1 -> mr3 -> mr2 -> mr1
        let body = serde_json::json!({ "depends_on": [mr3_id] });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{mr1_id}/dependencies"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn remove_dependency() {
        let app = app();
        let (app, mr1_id) = create_mr(app, "RM A").await;
        let (app, mr2_id) = create_mr(app, "RM B").await;

        // Set dep
        let body = serde_json::json!({ "depends_on": [mr1_id] });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{mr2_id}/dependencies"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Remove it
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/merge-requests/{mr2_id}/dependencies/{mr1_id}"
                    ))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Verify gone
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/merge-requests/{mr2_id}/dependencies"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        assert_eq!(json["depends_on"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn remove_nonexistent_dependency_returns_404() {
        let app = app();
        let (app, mr_id) = create_mr(app, "NoRM").await;
        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/merge-requests/{mr_id}/dependencies/no-such"
                    ))
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn set_atomic_group() {
        let app = app();
        let (app, mr_id) = create_mr(app, "Atomic").await;
        let body = serde_json::json!({ "group": "migration-bundle" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{mr_id}/atomic-group"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["atomic_group"], "migration-bundle");
    }

    #[tokio::test]
    async fn queue_graph_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/merge-queue/graph")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["nodes"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn dependency_on_nonexistent_mr_rejected() {
        let app = app();
        let (app, mr_id) = create_mr(app, "No dep target").await;
        let body = serde_json::json!({ "depends_on": ["does-not-exist"] });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{mr_id}/dependencies"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── Source tracking & reason persistence tests (TASK-028) ────────────

    #[tokio::test]
    async fn dependency_source_is_explicit_for_non_jwt_caller() {
        let app = app();
        let (app, mr1_id) = create_mr(app, "Source MR").await;
        let (app, mr2_id) = create_mr(app, "Dep MR").await;

        // Set via PUT /dependencies with global token (no JWT claims) → Explicit source.
        let body = serde_json::json!({
            "depends_on": [mr1_id],
            "reason": "MR-A adds the UserPort trait that this code implements"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{mr2_id}/dependencies"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        // Global token has no jwt_claims → Explicit source.
        assert_eq!(json["depends_on"][0]["source"], "explicit");
        assert_eq!(
            json["depends_on"][0]["reason"],
            "MR-A adds the UserPort trait that this code implements"
        );
    }

    #[tokio::test]
    async fn reason_persisted_and_retrievable() {
        let app = app();
        let (app, mr1_id) = create_mr(app, "Reason A").await;
        let (app, mr2_id) = create_mr(app, "Reason B").await;

        // Set dep with reason.
        let body = serde_json::json!({
            "depends_on": [mr1_id],
            "reason": "needs migration schema from MR-A"
        });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{mr2_id}/dependencies"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // GET dependencies and verify reason is returned.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/merge-requests/{mr2_id}/dependencies"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["depends_on"][0]["mr_id"], mr1_id);
        // Global token has no jwt_claims → Explicit source.
        assert_eq!(json["depends_on"][0]["source"], "explicit");
        assert_eq!(
            json["depends_on"][0]["reason"],
            "needs migration schema from MR-A"
        );
    }

    #[tokio::test]
    async fn dependency_without_reason_has_null_reason() {
        let app = app();
        let (app, mr1_id) = create_mr(app, "NoReason A").await;
        let (app, mr2_id) = create_mr(app, "NoReason B").await;

        let body = serde_json::json!({ "depends_on": [mr1_id] });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{mr2_id}/dependencies"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        // Global token has no jwt_claims → Explicit source.
        assert_eq!(json["depends_on"][0]["source"], "explicit");
        // reason should be null (absent from JSON due to skip_serializing_if).
        assert!(json["depends_on"][0]["reason"].is_null());
    }

    #[tokio::test]
    async fn dependency_source_is_agent_declared_for_jwt_caller() {
        let state = test_state();

        // Mint an agent JWT and register it in the agent_tokens store.
        let agent_jwt = state
            .agent_signing_key
            .mint("agent-42", "task-1", "system", &state.base_url, 3600)
            .unwrap();
        state
            .kv_store
            .kv_set("agent_tokens", "agent-42", agent_jwt.clone())
            .await
            .unwrap();

        let app = crate::api::api_router().with_state(state);

        // Create two MRs using the global token (for setup).
        let (app, mr1_id) = create_mr(app, "JWT Source MR").await;
        let (app, mr2_id) = create_mr(app, "JWT Dep MR").await;

        // Set dependency using agent JWT → should get "agent-declared" source.
        let body = serde_json::json!({
            "depends_on": [mr1_id],
            "reason": "Agent discovered runtime dependency"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/merge-requests/{mr2_id}/dependencies"))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {agent_jwt}"))
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        // Agent JWT has jwt_claims → AgentDeclared source.
        assert_eq!(json["depends_on"][0]["source"], "agent-declared");
        assert_eq!(
            json["depends_on"][0]["reason"],
            "Agent discovered runtime dependency"
        );
    }
}
