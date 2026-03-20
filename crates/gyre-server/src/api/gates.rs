//! Quality gate CRUD and gate-result endpoints.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{GateResult, GateStatus, GateType, QualityGate};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::instrument;

use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateGateRequest {
    pub name: String,
    pub gate_type: GateType,
    /// Shell command (required for TestCommand / LintCommand).
    pub command: Option<String>,
    /// Minimum approvals (required for RequiredApprovals).
    pub required_approvals: Option<u32>,
}

#[derive(Serialize)]
pub struct GateResponse {
    pub id: String,
    pub repo_id: String,
    pub name: String,
    pub gate_type: String,
    pub command: Option<String>,
    pub required_approvals: Option<u32>,
    pub created_at: u64,
}

impl From<QualityGate> for GateResponse {
    fn from(g: QualityGate) -> Self {
        Self {
            id: g.id.to_string(),
            repo_id: g.repo_id.to_string(),
            name: g.name,
            gate_type: gate_type_str(&g.gate_type),
            command: g.command,
            required_approvals: g.required_approvals,
            created_at: g.created_at,
        }
    }
}

fn gate_type_str(t: &GateType) -> String {
    match t {
        GateType::TestCommand => "test_command",
        GateType::LintCommand => "lint_command",
        GateType::RequiredApprovals => "required_approvals",
    }
    .to_string()
}

#[derive(Serialize)]
pub struct GateResultResponse {
    pub id: String,
    pub gate_id: String,
    pub mr_id: String,
    pub status: String,
    pub output: Option<String>,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
}

impl From<GateResult> for GateResultResponse {
    fn from(r: GateResult) -> Self {
        Self {
            id: r.id.to_string(),
            gate_id: r.gate_id.to_string(),
            mr_id: r.mr_id.to_string(),
            status: gate_status_str(&r.status),
            output: r.output,
            started_at: r.started_at,
            finished_at: r.finished_at,
        }
    }
}

fn gate_status_str(s: &GateStatus) -> String {
    match s {
        GateStatus::Pending => "pending",
        GateStatus::Running => "running",
        GateStatus::Passed => "passed",
        GateStatus::Failed => "failed",
    }
    .to_string()
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/repos/:id/gates — create a quality gate for a repo.
#[instrument(skip(state, req, _admin), fields(repo_id = %repo_id))]
pub async fn create_gate(
    State(state): State<Arc<AppState>>,
    _admin: crate::auth::AdminOnly,
    Path(repo_id): Path<String>,
    Json(req): Json<CreateGateRequest>,
) -> Result<(StatusCode, Json<GateResponse>), ApiError> {
    // Validate the repo exists.
    state
        .repos
        .find_by_id(&Id::new(&repo_id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {repo_id} not found")))?;

    // Validate required fields per gate type.
    match req.gate_type {
        GateType::TestCommand | GateType::LintCommand => {
            if req.command.is_none() {
                return Err(ApiError::InvalidInput(
                    "command is required for test_command / lint_command gates".to_string(),
                ));
            }
        }
        GateType::RequiredApprovals => {
            if req.required_approvals.is_none() {
                return Err(ApiError::InvalidInput(
                    "required_approvals is required for required_approvals gate".to_string(),
                ));
            }
        }
    }

    let gate = QualityGate {
        id: new_id(),
        repo_id: Id::new(repo_id),
        name: req.name,
        gate_type: req.gate_type,
        command: req.command,
        required_approvals: req.required_approvals,
        created_at: now_secs(),
    };

    state
        .quality_gates
        .lock()
        .await
        .insert(gate.id.to_string(), gate.clone());

    Ok((StatusCode::CREATED, Json(GateResponse::from(gate))))
}

/// GET /api/v1/repos/:id/gates — list quality gates for a repo.
pub async fn list_gates(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<Vec<GateResponse>>, ApiError> {
    let gates = state.quality_gates.lock().await;
    let mut result: Vec<GateResponse> = gates
        .values()
        .filter(|g| g.repo_id.as_str() == repo_id)
        .cloned()
        .map(GateResponse::from)
        .collect();
    result.sort_by_key(|g| g.created_at);
    Ok(Json(result))
}

/// DELETE /api/v1/repos/:id/gates/:gate_id — remove a gate.
pub async fn delete_gate(
    State(state): State<Arc<AppState>>,
    Path((repo_id, gate_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    let mut gates = state.quality_gates.lock().await;
    match gates.get(&gate_id) {
        None => Err(ApiError::NotFound(format!("gate {gate_id} not found"))),
        Some(g) if g.repo_id.as_str() != repo_id => {
            Err(ApiError::NotFound(format!("gate {gate_id} not found")))
        }
        _ => {
            gates.remove(&gate_id);
            Ok(StatusCode::NO_CONTENT)
        }
    }
}

/// GET /api/v1/merge-requests/:id/gates — list gate results for an MR.
pub async fn list_mr_gate_results(
    State(state): State<Arc<AppState>>,
    Path(mr_id): Path<String>,
) -> Result<Json<Vec<GateResultResponse>>, ApiError> {
    let results = state.gate_results.lock().await;
    let mut out: Vec<GateResultResponse> = results
        .values()
        .filter(|r| r.mr_id.as_str() == mr_id)
        .cloned()
        .map(GateResultResponse::from)
        .collect();
    out.sort_by_key(|r| r.started_at.unwrap_or(0));
    Ok(Json(out))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_domain::Repository;
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

    async fn create_repo(state: std::sync::Arc<crate::AppState>) -> String {
        let repo = Repository::new(
            gyre_common::Id::new("repo-1"),
            gyre_common::Id::new("proj-1"),
            "test-repo",
            "/tmp/test-repo",
            0,
        );
        state.repos.create(&repo).await.unwrap();
        "repo-1".to_string()
    }

    #[tokio::test]
    async fn create_and_list_gate() {
        let state = test_state();
        create_repo(state.clone()).await;
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "name": "unit-tests",
            "gate_type": "test_command",
            "command": "cargo test"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/repo-1/gates")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["name"], "unit-tests");
        assert_eq!(json["gate_type"], "test_command");

        // list
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repos/repo-1/gates")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn create_gate_missing_command_returns_bad_request() {
        let state = test_state();
        create_repo(state.clone()).await;
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "name": "lint",
            "gate_type": "lint_command"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/repo-1/gates")
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
    async fn delete_gate() {
        let state = test_state();
        create_repo(state.clone()).await;
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "name": "tests",
            "gate_type": "test_command",
            "command": "cargo test"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/repos/repo-1/gates")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        let gate_id = json["id"].as_str().unwrap().to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/repos/repo-1/gates/{gate_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn list_mr_gates_initially_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/merge-requests/mr-1/gates")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 0);
    }
}
