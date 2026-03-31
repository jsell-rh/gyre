//! Quality gate CRUD, gate-result endpoints, and spec approval ledger.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{GateResult, GateStatus, GateType, QualityGate, SpecApproval};
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
    /// Persona path (used by AgentReview / AgentValidation).
    pub persona: Option<String>,
    /// When false, gate is advisory only — failures do not block merging. Defaults to true.
    pub required: Option<bool>,
}

#[derive(Serialize)]
pub struct GateResponse {
    pub id: String,
    pub repo_id: String,
    pub name: String,
    pub gate_type: String,
    pub command: Option<String>,
    pub required_approvals: Option<u32>,
    pub persona: Option<String>,
    /// Whether this gate is blocking (true) or advisory-only (false).
    pub required: bool,
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
            persona: g.persona,
            required: g.required,
            created_at: g.created_at,
        }
    }
}

fn gate_type_str(t: &GateType) -> String {
    match t {
        GateType::TestCommand => "test_command",
        GateType::LintCommand => "lint_command",
        GateType::RequiredApprovals => "required_approvals",
        GateType::AgentReview => "agent_review",
        GateType::AgentValidation => "agent_validation",
        GateType::TraceCapture => "trace_capture",
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
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
            gate_name: None,
            gate_type: None,
            required: None,
            command: None,
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
// Spec Approval types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SpecApprovalResponse {
    pub id: String,
    pub spec_path: String,
    pub spec_sha: String,
    pub approver_id: String,
    pub signature: Option<String>,
    pub approved_at: u64,
    pub revoked_at: Option<u64>,
    pub revoked_by: Option<String>,
    pub revocation_reason: Option<String>,
    pub active: bool,
}

impl From<SpecApproval> for SpecApprovalResponse {
    fn from(a: SpecApproval) -> Self {
        let active = a.is_active();
        Self {
            id: a.id.to_string(),
            spec_path: a.spec_path,
            spec_sha: a.spec_sha,
            approver_id: a.approver_id,
            signature: a.signature,
            approved_at: a.approved_at,
            revoked_at: a.revoked_at,
            revoked_by: a.revoked_by,
            revocation_reason: a.revocation_reason,
            active,
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers — Quality Gates
// ---------------------------------------------------------------------------

/// POST /api/v1/repos/:id/gates — create a quality gate for a repo.
#[instrument(skip(state, req), fields(repo_id = %repo_id))]
pub async fn create_gate(
    State(state): State<Arc<AppState>>,
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
        GateType::AgentReview | GateType::AgentValidation => {
            // persona is optional — defaults to "personas/default.md" at execution time.
        }
        GateType::TraceCapture => {
            // config is optional — defaults are applied at execution time.
        }
    }

    let gate = QualityGate {
        id: new_id(),
        repo_id: Id::new(repo_id),
        name: req.name,
        gate_type: req.gate_type,
        command: req.command,
        required_approvals: req.required_approvals,
        persona: req.persona,
        required: req.required.unwrap_or(true),
        created_at: now_secs(),
    };

    state.quality_gates.save(&gate).await?;

    Ok((StatusCode::CREATED, Json(GateResponse::from(gate))))
}

/// GET /api/v1/repos/:id/gates — list quality gates for a repo.
pub async fn list_gates(
    State(state): State<Arc<AppState>>,
    Path(repo_id): Path<String>,
) -> Result<Json<Vec<GateResponse>>, ApiError> {
    let mut result: Vec<GateResponse> = state
        .quality_gates
        .list_by_repo_id(&repo_id)
        .await?
        .into_iter()
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
    match state.quality_gates.find_by_id(&gate_id).await? {
        None => Err(ApiError::NotFound(format!("gate {gate_id} not found"))),
        Some(g) if g.repo_id.as_str() != repo_id => {
            Err(ApiError::NotFound(format!("gate {gate_id} not found")))
        }
        _ => {
            state.quality_gates.delete(&gate_id).await?;
            Ok(StatusCode::NO_CONTENT)
        }
    }
}

/// GET /api/v1/merge-requests/:id/gates — list gate results for an MR.
///
/// Enriches each result with gate definition fields (name, type, required,
/// command) so the UI can display meaningful information without a separate
/// gate definitions fetch.
pub async fn list_mr_gate_results(
    State(state): State<Arc<AppState>>,
    Path(mr_id): Path<String>,
) -> Result<Json<Vec<GateResultResponse>>, ApiError> {
    let results = state.gate_results.list_by_mr_id(&mr_id).await?;

    // Collect unique gate_ids to look up definitions
    let gate_ids: Vec<String> = results
        .iter()
        .map(|r| r.gate_id.to_string())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    // Build a map of gate_id → definition (best-effort)
    let mut gate_defs = std::collections::HashMap::new();
    for gid in &gate_ids {
        if let Ok(Some(def)) = state.quality_gates.find_by_id(gid).await {
            gate_defs.insert(gid.clone(), def);
        }
    }

    let mut out: Vec<GateResultResponse> = results
        .into_iter()
        .map(|r| {
            let mut resp = GateResultResponse::from(r);
            if let Some(def) = gate_defs.get(&resp.gate_id) {
                resp.gate_name = Some(def.name.clone());
                resp.gate_type = Some(gate_type_str(&def.gate_type));
                resp.required = Some(def.required);
                resp.command = def.command.clone();
            }
            resp
        })
        .collect();
    out.sort_by_key(|r| r.started_at.unwrap_or(0));
    Ok(Json(out))
}

// ---------------------------------------------------------------------------
// Handlers — Spec Approval Ledger
// ---------------------------------------------------------------------------

/// GET /api/v1/specs/approvals — list all spec approvals (optionally filter by ?path=).
pub async fn list_spec_approvals(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<SpecApprovalResponse>>, ApiError> {
    let path_filter = params.get("path").map(|s| s.as_str());
    let all = if let Some(path) = path_filter {
        state.spec_approvals.list_by_path(path).await?
    } else {
        state.spec_approvals.list_all().await?
    };
    let mut result: Vec<SpecApprovalResponse> =
        all.into_iter().map(SpecApprovalResponse::from).collect();
    result.sort_by_key(|a| a.approved_at);
    Ok(Json(result))
}

/// Check that a spec_ref ("path@sha") has an active approval in the ledger.
/// Returns Ok(()) if approved, Err(msg) if not.
pub async fn verify_spec_ref(state: &AppState, spec_ref: &str) -> Result<(), String> {
    // Parse "path@sha" format.
    let (path, sha) = spec_ref
        .rsplit_once('@')
        .ok_or_else(|| format!("invalid spec_ref format '{spec_ref}': expected 'path@sha'"))?;

    // Validate SHA is 40-char hex.
    if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "spec_ref SHA '{sha}' is not a valid 40-char hex SHA"
        ));
    }

    let active = state
        .spec_approvals
        .list_active_by_path(path)
        .await
        .unwrap_or_default();
    let has_active_approval = active.iter().any(|a| a.spec_sha == sha);

    if has_active_approval {
        Ok(())
    } else {
        Err(format!(
            "spec '{path}' at SHA '{sha}' has no active approval in the ledger"
        ))
    }
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
    async fn create_agent_review_gate() {
        let state = test_state();
        create_repo(state.clone()).await;
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "name": "security-review",
            "gate_type": "agent_review",
            "persona": "personas/security.md"
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
        assert_eq!(json["gate_type"], "agent_review");
        assert_eq!(json["persona"], "personas/security.md");
    }

    #[tokio::test]
    async fn create_agent_validation_gate() {
        let state = test_state();
        create_repo(state.clone()).await;
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "name": "domain-validation",
            "gate_type": "agent_validation",
            "persona": "personas/accountability.md"
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
        assert_eq!(json["gate_type"], "agent_validation");
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
