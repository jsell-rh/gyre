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
// Spec Approval types
// ---------------------------------------------------------------------------

/// Request body for POST /api/v1/specs/approve.
#[derive(Deserialize)]
pub struct ApproveSpecRequest {
    /// Relative spec path, e.g. "specs/system/agent-gates.md".
    pub path: String,
    /// Git blob SHA of the spec at approval time (must be 40-char hex).
    pub sha: String,
    /// Optional Sigstore signature.
    pub signature: Option<String>,
}

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

/// Request body for POST /api/v1/specs/revoke.
#[derive(Deserialize)]
pub struct RevokeSpecRequest {
    /// Approval ID to revoke.
    pub approval_id: String,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Handlers — Quality Gates
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
        GateType::AgentReview | GateType::AgentValidation => {
            // persona is optional — defaults to "personas/default.md" at execution time.
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
// Handlers — Spec Approval Ledger
// ---------------------------------------------------------------------------

/// POST /api/v1/specs/approve — record an approval of a spec at a specific SHA.
///
/// The SHA must be a 40-character hex string to prevent git argument injection.
/// The approver identity is derived server-side from the authenticated caller.
#[instrument(skip(state, req, auth))]
pub async fn approve_spec(
    State(state): State<Arc<AppState>>,
    auth: crate::auth::AuthenticatedAgent,
    Json(req): Json<ApproveSpecRequest>,
) -> Result<(StatusCode, Json<SpecApprovalResponse>), ApiError> {
    // Validate SHA is 40-char hex (security: prevents git argument injection).
    if req.sha.len() != 40 || !req.sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::InvalidInput(
            "spec_sha must be a 40-character hex string".to_string(),
        ));
    }
    if req.path.is_empty() {
        return Err(ApiError::InvalidInput(
            "spec path must not be empty".to_string(),
        ));
    }

    // Derive approver_id server-side from authenticated caller — never trust client-supplied value.
    let approver_id = auth
        .user_id
        .map(|id| format!("user:{id}"))
        .unwrap_or_else(|| format!("agent:{}", auth.agent_id));

    let now = now_secs();
    let mut approval = SpecApproval::new(new_id(), req.path, req.sha, approver_id, now);
    approval.signature = req.signature;

    state
        .spec_approvals
        .lock()
        .await
        .insert(approval.id.to_string(), approval.clone());

    Ok((
        StatusCode::CREATED,
        Json(SpecApprovalResponse::from(approval)),
    ))
}

/// GET /api/v1/specs/approvals — list all spec approvals (optionally filter by ?path=).
pub async fn list_spec_approvals(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<SpecApprovalResponse>>, ApiError> {
    let path_filter = params.get("path").map(|s| s.as_str());
    let approvals = state.spec_approvals.lock().await;
    let mut result: Vec<SpecApprovalResponse> = approvals
        .values()
        .filter(|a| path_filter.is_none_or(|p| a.spec_path == p))
        .cloned()
        .map(SpecApprovalResponse::from)
        .collect();
    result.sort_by_key(|a| a.approved_at);
    Ok(Json(result))
}

/// POST /api/v1/specs/revoke — revoke an existing spec approval.
///
/// Only the original approver or an Admin may revoke an approval.
#[instrument(skip(state, req, auth))]
pub async fn revoke_spec_approval(
    State(state): State<Arc<AppState>>,
    auth: crate::auth::AuthenticatedAgent,
    Json(req): Json<RevokeSpecRequest>,
) -> Result<Json<SpecApprovalResponse>, ApiError> {
    // Derive caller identity server-side.
    let caller_id = auth
        .user_id
        .as_ref()
        .map(|id| format!("user:{id}"))
        .unwrap_or_else(|| format!("agent:{}", auth.agent_id));
    let is_admin = auth.roles.contains(&gyre_domain::UserRole::Admin);

    let mut approvals = state.spec_approvals.lock().await;
    let approval = approvals
        .get_mut(&req.approval_id)
        .ok_or_else(|| ApiError::NotFound(format!("approval {} not found", req.approval_id)))?;

    if approval.revoked_at.is_some() {
        return Err(ApiError::InvalidInput(
            "approval is already revoked".to_string(),
        ));
    }

    // Authorization: caller must be the original approver or an Admin.
    if !is_admin && approval.approver_id != caller_id {
        return Err(ApiError::forbidden(
            "only the original approver or an Admin may revoke this approval",
        ));
    }

    approval.revoked_at = Some(now_secs());
    approval.revoked_by = Some(caller_id);
    approval.revocation_reason = Some(req.reason);

    Ok(Json(SpecApprovalResponse::from(approval.clone())))
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

    let approvals = state.spec_approvals.lock().await;
    let has_active_approval = approvals
        .values()
        .any(|a| a.spec_path == path && a.spec_sha == sha && a.is_active());

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

    // ---- Spec Approval tests ----

    #[tokio::test]
    async fn approve_spec_and_list() {
        let app = app();
        // Note: approver_id is NOT in the request body — it is derived server-side.
        let body = serde_json::json!({
            "path": "specs/system/agent-gates.md",
            "sha": "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["spec_path"], "specs/system/agent-gates.md");
        assert_eq!(json["active"], true);
        // Approver identity is derived from auth token, not the request body.
        let approver_id = json["approver_id"].as_str().unwrap();
        assert!(
            approver_id.starts_with("agent:") || approver_id.starts_with("user:"),
            "approver_id should be server-derived: {approver_id}"
        );

        // list all
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/approvals")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    /// Client-supplied approver_id in body must be ignored (not trusted).
    #[tokio::test]
    async fn approve_spec_ignores_client_supplied_approver_id() {
        let app = app();
        // Include approver_id in body — server should ignore it and use auth identity.
        let body = serde_json::json!({
            "path": "specs/system/agent-gates.md",
            "sha": "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2",
            "approver_id": "user:attacker"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        // Must NOT be the attacker-supplied value.
        assert_ne!(json["approver_id"], "user:attacker");
    }

    #[tokio::test]
    async fn approve_spec_invalid_sha_rejected() {
        let app = app();
        let body = serde_json::json!({
            "path": "specs/system/agent-gates.md",
            "sha": "not-a-sha"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/approve")
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
    async fn revoke_spec_approval() {
        let app = app();
        // First approve (same token = same caller)
        let body = serde_json::json!({
            "path": "specs/system/agent-gates.md",
            "sha": "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let json = body_json(resp).await;
        let approval_id = json["id"].as_str().unwrap().to_string();

        // Then revoke with the same caller — should succeed.
        // Note: revoked_by is NOT in the request body — it is derived server-side.
        let revoke_body = serde_json::json!({
            "approval_id": approval_id,
            "reason": "spec was superseded"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/revoke")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&revoke_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["active"], false);
        assert_eq!(json["revocation_reason"], "spec was superseded");
        // revoked_by must be set to server-derived caller identity.
        let revoked_by = json["revoked_by"].as_str().unwrap();
        assert!(
            revoked_by.starts_with("agent:") || revoked_by.starts_with("user:"),
            "revoked_by should be server-derived: {revoked_by}"
        );
    }
}
