//! Spec registry API endpoints.
//!
//! GET  /api/v1/specs               — list all specs with ledger state
//! GET  /api/v1/specs/pending       — specs awaiting approval
//! GET  /api/v1/specs/drifted       — specs with open drift-review tasks
//! GET  /api/v1/specs/index         — auto-generated markdown index
//! GET  /api/v1/specs/:path         — single spec (URL-encoded path)
//! POST /api/v1/specs/:path/approve — approve a spec version
//! POST /api/v1/specs/:path/revoke  — revoke an approval
//! GET  /api/v1/specs/:path/history — approval history

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::spec_registry::{ApprovalStatus, SpecApprovalEvent, SpecLedgerEntry};
use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SpecLedgerResponse {
    pub path: String,
    pub title: String,
    pub owner: String,
    pub current_sha: String,
    pub approval_mode: String,
    pub approval_status: String,
    pub linked_tasks: Vec<String>,
    pub linked_mrs: Vec<String>,
    pub drift_status: String,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<SpecLedgerEntry> for SpecLedgerResponse {
    fn from(e: SpecLedgerEntry) -> Self {
        Self {
            path: e.path,
            title: e.title,
            owner: e.owner,
            current_sha: e.current_sha,
            approval_mode: e.approval_mode,
            approval_status: e.approval_status.to_string(),
            linked_tasks: e.linked_tasks,
            linked_mrs: e.linked_mrs,
            drift_status: e.drift_status,
            created_at: e.created_at,
            updated_at: e.updated_at,
        }
    }
}

#[derive(Serialize)]
pub struct SpecApprovalEventResponse {
    pub id: String,
    pub spec_path: String,
    pub spec_sha: String,
    pub approver_type: String,
    pub approver_id: String,
    pub persona: Option<String>,
    pub approved_at: u64,
    pub revoked_at: Option<u64>,
    pub revoked_by: Option<String>,
    pub revocation_reason: Option<String>,
    pub is_active: bool,
}

impl From<SpecApprovalEvent> for SpecApprovalEventResponse {
    fn from(e: SpecApprovalEvent) -> Self {
        let is_active = e.is_active();
        Self {
            id: e.id,
            spec_path: e.spec_path,
            spec_sha: e.spec_sha,
            approver_type: e.approver_type,
            approver_id: e.approver_id,
            persona: e.persona,
            approved_at: e.approved_at,
            revoked_at: e.revoked_at,
            revoked_by: e.revoked_by,
            revocation_reason: e.revocation_reason,
            is_active,
        }
    }
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ApproveSpecRequest {
    /// The git blob SHA being approved. Must match the ledger's `current_sha`.
    pub sha: String,
    /// Optional Sigstore signature.
    pub signature: Option<String>,
    /// If approving as an agent persona, set this. If absent, treated as human approval.
    pub persona: Option<String>,
}

#[derive(Deserialize)]
pub struct RevokeSpecApprovalRequest {
    pub reason: String,
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs — list all specs
// ---------------------------------------------------------------------------

pub async fn list_specs(State(state): State<Arc<AppState>>) -> Json<Vec<SpecLedgerResponse>> {
    let ledger = state.spec_ledger.lock().await;
    let mut specs: Vec<SpecLedgerResponse> = ledger.values().cloned().map(Into::into).collect();
    specs.sort_by(|a, b| a.path.cmp(&b.path));
    Json(specs)
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/pending — specs awaiting approval
// ---------------------------------------------------------------------------

pub async fn list_pending_specs(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<SpecLedgerResponse>> {
    let ledger = state.spec_ledger.lock().await;
    let mut specs: Vec<SpecLedgerResponse> = ledger
        .values()
        .filter(|e| e.approval_status == ApprovalStatus::Pending)
        .cloned()
        .map(Into::into)
        .collect();
    specs.sort_by(|a, b| a.path.cmp(&b.path));
    Json(specs)
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/drifted — specs with open drift-review tasks
// ---------------------------------------------------------------------------

pub async fn list_drifted_specs(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<SpecLedgerResponse>> {
    let ledger = state.spec_ledger.lock().await;
    let mut specs: Vec<SpecLedgerResponse> = ledger
        .values()
        .filter(|e| e.drift_status == "drifted")
        .cloned()
        .map(Into::into)
        .collect();
    specs.sort_by(|a, b| a.path.cmp(&b.path));
    Json(specs)
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/index — auto-generated markdown index
// ---------------------------------------------------------------------------

pub async fn spec_index(State(state): State<Arc<AppState>>) -> axum::response::Response<String> {
    let ledger = state.spec_ledger.lock().await;

    // Group specs by directory.
    let mut by_dir: std::collections::BTreeMap<String, Vec<SpecLedgerEntry>> =
        std::collections::BTreeMap::new();
    for entry in ledger.values() {
        let dir = entry.path.split('/').next().unwrap_or("other").to_string();
        by_dir.entry(dir).or_default().push(entry.clone());
    }

    let mut md = String::from("# Spec Registry Index\n\n");
    md.push_str("> Auto-generated from `specs/manifest.yaml` + forge ledger.\n\n");

    for (dir, mut entries) in by_dir {
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        md.push_str(&format!("## {}\n\n", capitalize_dir(&dir)));
        md.push_str("| Spec | Status | SHA |\n");
        md.push_str("|------|--------|-----|\n");
        for e in entries {
            let short_sha = if e.current_sha.len() >= 8 {
                &e.current_sha[..8]
            } else {
                &e.current_sha
            };
            md.push_str(&format!(
                "| [{title}](specs/{path}) | {status} | `{sha}` |\n",
                title = e.title,
                path = e.path,
                status = e.approval_status,
                sha = short_sha,
            ));
        }
        md.push('\n');
    }

    axum::response::Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/markdown; charset=utf-8")
        .body(md)
        .unwrap()
}

fn capitalize_dir(dir: &str) -> String {
    let mut chars = dir.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/:path — single spec
//
// The path parameter is URL-encoded, e.g. system%2Fdesign-principles.md
// ---------------------------------------------------------------------------

pub async fn get_spec(
    State(state): State<Arc<AppState>>,
    Path(encoded_path): Path<String>,
) -> Result<Json<SpecLedgerResponse>, ApiError> {
    // axum already URL-decodes path segments.
    let spec_path = encoded_path;
    let ledger = state.spec_ledger.lock().await;
    ledger
        .get(&spec_path)
        .cloned()
        .map(|e| Json(e.into()))
        .ok_or_else(|| ApiError::NotFound(format!("spec '{spec_path}' not in registry")))
}

// ---------------------------------------------------------------------------
// POST /api/v1/specs/:path/approve — approve a spec version
// ---------------------------------------------------------------------------

pub async fn approve_spec(
    State(state): State<Arc<AppState>>,
    Path(encoded_path): Path<String>,
    auth: crate::auth::AuthenticatedAgent,
    Json(req): Json<ApproveSpecRequest>,
) -> Result<(StatusCode, Json<SpecApprovalEventResponse>), ApiError> {
    let spec_path = encoded_path;
    let now = now_secs();

    // ReadOnly users cannot approve specs (M21.1-C).
    if auth.roles.contains(&gyre_domain::UserRole::ReadOnly)
        && !auth.roles.contains(&gyre_domain::UserRole::Admin)
        && !auth.roles.contains(&gyre_domain::UserRole::Developer)
        && !auth.roles.contains(&gyre_domain::UserRole::Agent)
        && auth.agent_id != "system"
    {
        return Err(ApiError::Forbidden(
            "ReadOnly users cannot approve specs".to_string(),
        ));
    }

    // Validate SHA format (must be 40-char hex).
    if req.sha.len() != 40 || !req.sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ApiError::InvalidInput(
            "sha must be a 40-character hex string".to_string(),
        ));
    }

    // Verify spec is in the ledger.
    {
        let ledger = state.spec_ledger.lock().await;
        if !ledger.contains_key(&spec_path) {
            return Err(ApiError::NotFound(format!(
                "spec '{spec_path}' not in registry"
            )));
        }
    }

    // Determine approver type from auth token kind (not request body).
    // JWT bearer tokens → agent; global token / API key → human.
    let (approver_type, approver_id) = if auth.jwt_claims.is_some() {
        ("agent".to_string(), format!("agent:{}", auth.agent_id))
    } else {
        ("human".to_string(), format!("user:{}", auth.agent_id))
    };

    let event = SpecApprovalEvent {
        id: new_id().to_string(),
        spec_path: spec_path.clone(),
        spec_sha: req.sha.clone(),
        approver_type,
        approver_id,
        persona: req.persona,
        approved_at: now,
        revoked_at: None,
        revoked_by: None,
        revocation_reason: None,
    };

    // Record in approval history.
    state.spec_approval_history.lock().await.push(event.clone());

    // Update ledger approval_status based on new approval.
    // For simplicity: any valid approval for the current SHA sets status to Approved.
    {
        let mut ledger = state.spec_ledger.lock().await;
        if let Some(entry) = ledger.get_mut(&spec_path) {
            if entry.current_sha == req.sha {
                entry.approval_status = ApprovalStatus::Approved;
                entry.updated_at = now;
            }
        }
    }

    Ok((StatusCode::CREATED, Json(event.into())))
}

// ---------------------------------------------------------------------------
// POST /api/v1/specs/:path/revoke — revoke an approval
// ---------------------------------------------------------------------------

pub async fn revoke_spec_approval(
    State(state): State<Arc<AppState>>,
    Path(encoded_path): Path<String>,
    auth: crate::auth::AuthenticatedAgent,
    Json(req): Json<RevokeSpecApprovalRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let spec_path = encoded_path;
    let now = now_secs();

    // Find the most recent active approval for this spec path.
    let mut history = state.spec_approval_history.lock().await;
    let active = history
        .iter_mut()
        .rev()
        .find(|e| e.spec_path == spec_path && e.is_active());

    match active {
        None => Err(ApiError::NotFound(format!(
            "no active approval for spec '{spec_path}'"
        ))),
        Some(ev) => {
            // Only the original approver or an Admin can revoke.
            let is_admin =
                auth.agent_id == "system" || auth.roles.contains(&gyre_domain::UserRole::Admin);
            let caller_id = format!(
                "{}:{}",
                if auth.jwt_claims.is_some() {
                    "agent"
                } else {
                    "user"
                },
                auth.agent_id
            );
            if ev.approver_id != caller_id && !is_admin {
                return Err(ApiError::Forbidden(
                    "only the original approver or an Admin can revoke".to_string(),
                ));
            }

            ev.revoked_at = Some(now);
            ev.revoked_by = Some(auth.agent_id.clone());
            ev.revocation_reason = Some(req.reason);

            // Reset ledger approval_status to Pending.
            drop(history);
            let mut ledger = state.spec_ledger.lock().await;
            if let Some(entry) = ledger.get_mut(&spec_path) {
                entry.approval_status = ApprovalStatus::Pending;
                entry.updated_at = now;
            }

            Ok(Json(serde_json::json!({
                "spec_path": spec_path,
                "revoked_by": auth.agent_id,
                "revoked_at": now,
            })))
        }
    }
}

// ---------------------------------------------------------------------------
// GET /api/v1/specs/:path/history — approval history
// ---------------------------------------------------------------------------

pub async fn spec_approval_history(
    State(state): State<Arc<AppState>>,
    Path(encoded_path): Path<String>,
) -> Json<Vec<SpecApprovalEventResponse>> {
    let spec_path = encoded_path;
    let history = state.spec_approval_history.lock().await;
    let events: Vec<SpecApprovalEventResponse> = history
        .iter()
        .filter(|e| e.spec_path == spec_path)
        .cloned()
        .map(Into::into)
        .collect();
    Json(events)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    use crate::spec_registry::{ApprovalStatus, SpecLedgerEntry};

    fn app() -> Router {
        let state = test_state();
        crate::api::api_router().with_state(state)
    }

    fn app_with_spec() -> (Router, std::sync::Arc<crate::AppState>) {
        let state = test_state();

        // Seed a spec entry into the ledger.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mut ledger = state.spec_ledger.lock().await;
                ledger.insert(
                    "system/design-principles.md".to_string(),
                    SpecLedgerEntry {
                        path: "system/design-principles.md".to_string(),
                        title: "Design Principles".to_string(),
                        owner: "user:jsell".to_string(),
                        current_sha: "a".repeat(40),
                        approval_mode: "human_only".to_string(),
                        approval_status: ApprovalStatus::Pending,
                        linked_tasks: vec![],
                        linked_mrs: vec![],
                        drift_status: "unknown".to_string(),
                        created_at: 1700000000,
                        updated_at: 1700000000,
                    },
                );
            })
        });

        let router = crate::api::api_router().with_state(state.clone());
        (router, state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_specs_empty() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_specs_returns_seeded_entry() {
        let (app, _) = app_with_spec();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["path"], "system/design-principles.md");
        assert_eq!(arr[0]["approval_status"], "pending");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_spec_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/system%2Fnonexistent.md")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_spec_found() {
        let (app, _) = app_with_spec();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/system%2Fdesign-principles.md")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["title"], "Design Principles");
        assert_eq!(json["approval_status"], "pending");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_spec_bad_sha() {
        let (app, _) = app_with_spec();
        let body = serde_json::json!({ "sha": "tooshort" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_spec_ok() {
        let (app, state) = app_with_spec();
        let sha = "a".repeat(40);
        let body = serde_json::json!({ "sha": sha });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Ledger should now be Approved.
        let ledger = state.spec_ledger.lock().await;
        let entry = ledger.get("system/design-principles.md").unwrap();
        assert_eq!(entry.approval_status, ApprovalStatus::Approved);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn approve_then_revoke() {
        let (app, state) = app_with_spec();
        let sha = "a".repeat(40);

        // Approve.
        let body = serde_json::json!({ "sha": sha });
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/approve")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Revoke.
        let revoke_body = serde_json::json!({ "reason": "outdated" });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/revoke")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&revoke_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Ledger back to Pending.
        let ledger = state.spec_ledger.lock().await;
        let entry = ledger.get("system/design-principles.md").unwrap();
        assert_eq!(entry.approval_status, ApprovalStatus::Pending);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_pending_filters_correctly() {
        let state = test_state();
        {
            let mut ledger = state.spec_ledger.lock().await;
            ledger.insert(
                "system/pending.md".to_string(),
                SpecLedgerEntry {
                    path: "system/pending.md".to_string(),
                    title: "Pending".to_string(),
                    owner: "user:jsell".to_string(),
                    current_sha: "a".repeat(40),
                    approval_mode: "human_only".to_string(),
                    approval_status: ApprovalStatus::Pending,
                    linked_tasks: vec![],
                    linked_mrs: vec![],
                    drift_status: "unknown".to_string(),
                    created_at: 1700000000,
                    updated_at: 1700000000,
                },
            );
            ledger.insert(
                "system/approved.md".to_string(),
                SpecLedgerEntry {
                    path: "system/approved.md".to_string(),
                    title: "Approved".to_string(),
                    owner: "user:jsell".to_string(),
                    current_sha: "b".repeat(40),
                    approval_mode: "human_only".to_string(),
                    approval_status: ApprovalStatus::Approved,
                    linked_tasks: vec![],
                    linked_mrs: vec![],
                    drift_status: "clean".to_string(),
                    created_at: 1700000000,
                    updated_at: 1700000000,
                },
            );
        }
        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/pending")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["path"], "system/pending.md");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn spec_history_empty() {
        let (app, _) = app_with_spec();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/system%2Fdesign-principles.md/history")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert!(json.as_array().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn spec_index_returns_markdown() {
        let (app, _) = app_with_spec();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/specs/index")
                    .header("authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("markdown"));
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("Spec Registry Index"));
        assert!(text.contains("Design Principles"));
    }
}
