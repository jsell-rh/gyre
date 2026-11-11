//! Impersonation endpoints (identity-security.md §Impersonation).
//!
//! Route → authorization model:
//! - `POST /api/v1/admin/impersonate` and `DELETE /api/v1/admin/impersonate/:request_id`
//!   are Admin-only: ABAC maps them to the `admin` resource (the
//!   `builtin-admin-only-deny` policy rejects non-Admin roles) and the
//!   handlers additionally verify the Admin role and a real user identity
//!   in the body (defense in depth, same pattern as `create_user`).
//! - `POST /api/v1/admin/impersonate/:request_id/approve` and `.../deny`
//!   are ABAC-exempt because the **target user** — not an admin — must call
//!   them, and the target may hold any role (a ReadOnly user must still be
//!   able to consent to or reject being impersonated). Authorization is
//!   enforced per-handler: the caller's verified user id must equal the
//!   request's target user, and approval additionally requires the
//!   one-time code that was delivered only to that user.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::UserRole;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::AuthenticatedAgent;
use crate::impersonation;
use crate::AppState;

use super::error::ApiError;

// ── POST /api/v1/admin/impersonate ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct ImpersonateRequest {
    pub target_user_id: String,
}

#[derive(Serialize)]
pub struct ImpersonateRequestResponse {
    pub request_id: String,
    /// When the approval request (and its code) expires — epoch seconds.
    pub expires_at: u64,
}

/// POST /api/v1/admin/impersonate — Admin requests to impersonate a user.
///
/// The response deliberately does **not** include the approval code: the
/// code is generated server-side and delivered to the target user only
/// (Inbox notification + WebSocket push when online). Returning it to the
/// requesting admin would let the admin self-approve, defeating the spec
/// requirement that the target user provides the code or explicit approval
/// before impersonation begins.
pub async fn request_impersonation(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Json(req): Json<ImpersonateRequest>,
) -> Result<(StatusCode, Json<ImpersonateRequestResponse>), ApiError> {
    let admin_id = impersonation::require_admin_actor(&auth)?;

    // Defense in depth: the ABAC `admin` resource mapping already rejects
    // non-Admin roles; require_admin_actor re-checked it above.

    let target_id = Id::new(req.target_user_id.clone());
    let target = state
        .users
        .find_by_id(&target_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("user {target_id} not found")))?;

    if target.id == admin_id {
        return Err(ApiError::InvalidInput(
            "cannot impersonate yourself".to_string(),
        ));
    }

    // Impersonating another Admin is prohibited. The platform role model
    // (UserRole) has no SuperAdmin role, so the task plan's "requires
    // SuperAdmin or is prohibited" resolves to prohibited.
    if target.roles.contains(&UserRole::Admin) {
        return Err(ApiError::Forbidden(
            "impersonation of Admin users is prohibited".to_string(),
        ));
    }

    // Rate limit (max one active impersonation session per admin) is
    // enforced inside create_request, under the session-write lock it
    // shares with the approval critical section.

    let admin = state
        .users
        .find_by_id(&admin_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("admin user {admin_id} not found")))?;

    let request = impersonation::create_request(&state, &admin, &target, &auth.tenant_id).await?;

    Ok((
        StatusCode::CREATED,
        Json(ImpersonateRequestResponse {
            request_id: request.request_id,
            expires_at: request.expires_at,
        }),
    ))
}

// ── POST /api/v1/admin/impersonate/:request_id/approve ──────────────────────

#[derive(Deserialize)]
pub struct ApproveImpersonationRequest {
    pub approval_code: String,
}

#[derive(Serialize)]
pub struct ApproveImpersonationResponse {
    /// Short-lived JWT: `sub` is the target user (permissions match),
    /// `impersonator_id` always names the acting admin.
    pub impersonation_token: String,
    pub expires_at: u64,
    pub session_id: String,
}

/// POST /api/v1/admin/impersonate/:request_id/approve — target user approves
/// the impersonation with the one-time code delivered to them.
///
/// ABAC-exempt (the caller is the target user, who may hold any role — a
/// ReadOnly target must still be able to consent). Per-handler
/// authorization: the caller's verified user id must equal the request's
/// target user and the code must match the stored hash. Impersonated
/// callers are rejected so an active session cannot extend itself.
pub async fn approve_impersonation(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Path(request_id): Path<String>,
    Json(req): Json<ApproveImpersonationRequest>,
) -> Result<(StatusCode, Json<ApproveImpersonationResponse>), ApiError> {
    if auth.impersonation.is_some() {
        return Err(ApiError::Forbidden(
            "cannot approve impersonation while impersonated".to_string(),
        ));
    }
    let caller_user_id = auth.user_id.clone().ok_or_else(|| {
        ApiError::Forbidden("approval requires an authenticated user identity".to_string())
    })?;

    let (session, token, expires_at) =
        impersonation::approve_request(&state, &request_id, &caller_user_id, &req.approval_code)
            .await?;

    Ok((
        StatusCode::CREATED,
        Json(ApproveImpersonationResponse {
            impersonation_token: token,
            expires_at,
            session_id: session.session_id,
        }),
    ))
}

// ── POST /api/v1/admin/impersonate/:request_id/deny ─────────────────────────

#[derive(Serialize)]
pub struct DenyImpersonationResponse {
    pub status: &'static str,
}

/// POST /api/v1/admin/impersonate/:request_id/deny — target user explicitly
/// denies the impersonation request.
///
/// ABAC-exempt for the same reason as approve; per-handler authorization
/// requires the caller to be the request's target user.
pub async fn deny_impersonation(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Path(request_id): Path<String>,
) -> Result<Json<DenyImpersonationResponse>, ApiError> {
    if auth.impersonation.is_some() {
        return Err(ApiError::Forbidden(
            "cannot deny impersonation while impersonated".to_string(),
        ));
    }
    let caller_user_id = auth.user_id.clone().ok_or_else(|| {
        ApiError::Forbidden("denial requires an authenticated user identity".to_string())
    })?;

    impersonation::deny_request(&state, &request_id, &caller_user_id).await?;

    Ok(Json(DenyImpersonationResponse { status: "denied" }))
}

// ── DELETE /api/v1/admin/impersonate/:request_id ────────────────────────────

#[derive(Serialize)]
pub struct EndImpersonationResponse {
    pub ended: bool,
    pub duration_secs: u64,
    pub actions_taken: u64,
}

/// DELETE /api/v1/admin/impersonate/:request_id — the requesting admin
/// explicitly ends the impersonation session.
///
/// Admin-only via ABAC (`admin` resource); per-handler authorization
/// additionally requires the caller to be the session's impersonator.
/// Emits `ImpersonationEnded` with duration and action count.
pub async fn end_impersonation(
    auth: AuthenticatedAgent,
    State(state): State<Arc<AppState>>,
    Path(request_id): Path<String>,
) -> Result<Json<EndImpersonationResponse>, ApiError> {
    let admin_id = impersonation::require_admin_actor(&auth)?;

    let session = impersonation::find_session_by_request(&state, &request_id)
        .await?
        .ok_or_else(|| {
            ApiError::NotFound(format!("no impersonation session for request {request_id}"))
        })?;

    // Per-handler authorization: only the impersonator may end the session.
    if session.impersonator_id != admin_id.as_str() {
        return Err(ApiError::Forbidden(
            "only the impersonating admin may end this session".to_string(),
        ));
    }

    let ended = impersonation::end_session(&state, &session.session_id, "ended_by_admin").await?;

    Ok(Json(EndImpersonationResponse {
        ended: ended.ended,
        duration_secs: ended
            .ended_at
            .unwrap_or_else(crate::api::now_secs)
            .saturating_sub(ended.started_at),
        actions_taken: ended.actions_taken,
    }))
}

#[cfg(test)]
mod tests {
    use crate::impersonation::{
        self, ImpersonationRequest, ImpersonationSession, REQUEST_NAMESPACE, SESSION_NAMESPACE,
    };
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_common::{Id, NotificationType};
    use gyre_domain::{User, UserRole, Workspace, WorkspaceMembership, WorkspaceRole};
    use http::{Request, StatusCode};
    use std::sync::Arc;
    use tower::ServiceExt;

    use super::*;

    fn app(state: &Arc<AppState>) -> Router {
        crate::api::api_router().with_state(state.clone())
    }

    fn audit_filter(event_type: &str) -> gyre_ports::AuditQueryFilter {
        gyre_ports::AuditQueryFilter {
            event_type: Some(event_type.to_string()),
            limit: 100,
            ..Default::default()
        }
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Seed a user with an API key (so requests authenticate with a real
    /// user identity) and, optionally, a workspace membership (so the user
    /// can receive Inbox notifications). Returns the raw API key.
    async fn seed_user(
        state: &Arc<AppState>,
        user_id: &str,
        name: &str,
        roles: Vec<UserRole>,
        with_membership: bool,
    ) -> String {
        let now = 1_700_000_000u64;
        let ws = Workspace::new(
            Id::new(format!("ws-imp-{user_id}")),
            Id::new("tenant-imp"),
            "imp-test",
            // Slug unique per seeded user: the workspace port contract
            // (enforced by the mem adapter) makes (tenant_id, slug) unique.
            format!("imp-{user_id}"),
            now,
        );
        state.workspaces.create(&ws).await.unwrap();

        let mut user = User::new(Id::new(user_id), format!("ext-{user_id}"), name, now);
        user.roles = roles;
        // The base's task-099 F1 API-key auth fails closed without a tenant
        // binding; seed one so API-key requests resolve the real scope.
        user.tenant_id = Some(Id::new("tenant-imp"));
        state.users.create(&user).await.unwrap();

        if with_membership {
            let membership = WorkspaceMembership::new(
                Id::new(format!("mem-{user_id}")),
                user.id.clone(),
                ws.id.clone(),
                WorkspaceRole::Developer,
                Id::new(user_id),
                now,
            );
            state
                .workspace_memberships
                .create(&membership)
                .await
                .unwrap();
        }

        let raw_key = format!("gyre_{user_id}");
        state
            .api_keys
            .create(&crate::auth::hash_api_key(&raw_key), &user.id, "imp-test")
            .await
            .unwrap();
        raw_key
    }

    /// Run the full request → approve flow and return
    /// (state, admin_key, target_key, request_id, impersonation_token).
    async fn run_full_flow() -> (Arc<AppState>, String, String, String, String) {
        let state = test_state();
        let admin_key =
            seed_user(&state, "admin-1", "Admin One", vec![UserRole::Admin], false).await;
        let target_key = seed_user(
            &state,
            "target-1",
            "Target User",
            vec![UserRole::Developer],
            true,
        )
        .await;

        // Admin requests impersonation.
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/impersonate")
                    .header("Authorization", format!("Bearer {admin_key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"target_user_id": "target-1"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let request_id = json["request_id"].as_str().unwrap().to_string();

        // The approval code must NOT be in the admin-facing response —
        // only the target user receives it.
        assert!(json.get("approval_code").is_none());

        // Target reads the approval code from their Inbox notification.
        let notif_json = app(&state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/notifications")
                    .header("Authorization", format!("Bearer {target_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(notif_json.status(), StatusCode::OK);
        let notifs = body_json(notif_json).await;
        let items = notifs["notifications"].as_array().unwrap();
        let code_notif = items
            .iter()
            .find(|n| n["notification_type"] == "ImpersonationRequest")
            .expect("target must receive an ImpersonationRequest notification");
        let body: serde_json::Value =
            serde_json::from_str(code_notif["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["request_id"].as_str().unwrap(), request_id);
        let code = body["approval_code"].as_str().unwrap().to_string();

        // Target approves with the code.
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/impersonate/{request_id}/approve"))
                    .header("Authorization", format!("Bearer {target_key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"approval_code": code}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let token = json["impersonation_token"].as_str().unwrap().to_string();
        let expires_at = json["expires_at"].as_u64().unwrap();
        // Short TTL (15 min default) — not a long-lived session token.
        assert!(expires_at <= crate::api::now_secs() + 900 + 5);

        (state, admin_key, target_key, request_id, token)
    }

    #[tokio::test]
    async fn full_flow_issues_token_notifies_target_and_audits_start() {
        let (state, _admin_key, target_key, _request_id, token) = run_full_flow().await;

        // ImpersonationStarted audit event with full attribution.
        let events = state
            .audit
            .query(&audit_filter("impersonation_started"))
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(
            e.user_id.as_ref().unwrap().as_str(),
            "admin-1",
            "ImpersonationStarted must be attributed to the impersonator"
        );
        assert_eq!(e.detail["impersonator_id"], "admin-1");
        assert_eq!(e.detail["target_user_id"], "target-1");
        assert!(e.detail["approval_token"].is_string());
        assert_eq!(e.resource_type, "impersonation");

        // The target is notified that impersonation began.
        let notifs = app(&state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/notifications")
                    .header("Authorization", format!("Bearer {target_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let notifs = body_json(notifs).await;
        let alert = notifs["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["notification_type"] == "ImpersonationAlert")
            .expect("target must be alerted when impersonation starts");
        assert_eq!(
            alert["title"].as_str().unwrap(),
            "Admin Admin One is impersonating your account"
        );

        // The impersonation token authenticates as the TARGET user.
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let me = body_json(resp).await;
        assert_eq!(me["username"].as_str().unwrap(), "Target User");
    }

    #[tokio::test]
    async fn impersonated_calls_are_attributed_and_counted() {
        let (state, _admin_key, _target_key, _request_id, token) = run_full_flow().await;

        // Route through the ABAC middleware so impersonated actions are
        // recorded (same layering as build_router).
        crate::abac_middleware::init_resolver();
        crate::abac_middleware::seed_builtin_policies(&state)
            .await
            .expect("seed built-in policies");

        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }
        let router = Router::new()
            .route("/api/v1/tasks", axum::routing::get(ok_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::abac_middleware::abac_middleware,
            ))
            .with_state(state.clone());

        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tasks")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // The action audit event is attributed to the impersonator.
        // (fire-and-forget task — poll briefly for the write to land.)
        let mut found = None;
        for _ in 0..50 {
            let events = state
                .audit
                .query(&audit_filter("impersonation_action"))
                .await
                .unwrap();
            if let Some(e) = events.iter().find(|e| e.detail["path"] == "/api/v1/tasks") {
                found = Some(e.clone());
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let e = found.expect("impersonated API call must be audited");
        assert_eq!(
            e.user_id.as_ref().unwrap().as_str(),
            "admin-1",
            "impersonated action must be attributed to the impersonator"
        );
        assert_eq!(e.detail["impersonator_id"], "admin-1");
        assert_eq!(e.detail["target_user_id"], "target-1");
        assert_eq!(e.detail["method"], "GET");
        assert_eq!(e.outcome, gyre_domain::AuditOutcome::Success);

        // actions_taken was incremented on the session.
        let sessions = state.kv_store.kv_list(SESSION_NAMESPACE).await.unwrap();
        let session: ImpersonationSession = serde_json::from_str(&sessions[0].1).unwrap();
        assert_eq!(session.actions_taken, 1);

        // An impersonated caller cannot request further impersonation.
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/impersonate")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"target_user_id": "someone"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn only_target_with_code_can_approve() {
        let state = test_state();
        let admin_key =
            seed_user(&state, "admin-2", "Admin Two", vec![UserRole::Admin], false).await;
        let target_key = seed_user(
            &state,
            "target-2",
            "Target Two",
            vec![UserRole::Developer],
            true,
        )
        .await;
        let other_key = seed_user(
            &state,
            "other-2",
            "Other User",
            vec![UserRole::Developer],
            true,
        )
        .await;

        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/impersonate")
                    .header("Authorization", format!("Bearer {admin_key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"target_user_id": "target-2"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let request_id = body_json(resp).await["request_id"]
            .as_str()
            .unwrap()
            .to_string();

        // Extract the code from the target's notification.
        let notifs = body_json(
            app(&state)
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/users/me/notifications")
                        .header("Authorization", format!("Bearer {target_key}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        let code = notifs["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["notification_type"] == "ImpersonationRequest")
            .map(|n| {
                serde_json::from_str::<serde_json::Value>(n["body"].as_str().unwrap()).unwrap()
                    ["approval_code"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .unwrap();

        // Wrong code → 403.
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/impersonate/{request_id}/approve"))
                    .header("Authorization", format!("Bearer {target_key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"approval_code": "00000000"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Correct code but wrong caller (not the target) → 403.
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/impersonate/{request_id}/approve"))
                    .header("Authorization", format!("Bearer {other_key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"approval_code": code}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Correct code, wrong caller, no code at all (shared system token) → 403.
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/impersonate/{request_id}/approve"))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"approval_code": code}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Target denies → subsequent approval is rejected.
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/impersonate/{request_id}/deny"))
                    .header("Authorization", format!("Bearer {target_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/impersonate/{request_id}/approve"))
                    .header("Authorization", format!("Bearer {target_key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"approval_code": code}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn authorization_constraints_on_request() {
        let state = test_state();
        let admin_key = seed_user(
            &state,
            "admin-3",
            "Admin Three",
            vec![UserRole::Admin],
            false,
        )
        .await;
        let dev_key = seed_user(
            &state,
            "dev-3",
            "Dev Three",
            vec![UserRole::Developer],
            true,
        )
        .await;
        seed_user(
            &state,
            "admin-3b",
            "Admin Three-B",
            vec![UserRole::Admin],
            true,
        )
        .await;

        let post = |key: &str, target: &str| {
            let state = state.clone();
            let key = key.to_string();
            let target = target.to_string();
            async move {
                app(&state)
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri("/api/v1/admin/impersonate")
                            .header("Authorization", format!("Bearer {key}"))
                            .header("Content-Type", "application/json")
                            .body(Body::from(
                                serde_json::json!({"target_user_id": target}).to_string(),
                            ))
                            .unwrap(),
                    )
                    .await
                    .unwrap()
            }
        };

        // Non-admin cannot request.
        assert_eq!(
            post(&dev_key, "dev-3").await.status(),
            StatusCode::FORBIDDEN
        );
        // Shared system token has the Admin role but no user identity.
        assert_eq!(
            post("test-token", "dev-3").await.status(),
            StatusCode::FORBIDDEN
        );
        // Admin cannot impersonate themselves.
        assert_eq!(
            post(&admin_key, "admin-3").await.status(),
            StatusCode::BAD_REQUEST
        );
        // Admin cannot impersonate another admin (no SuperAdmin role in the
        // platform model → prohibited).
        assert_eq!(
            post(&admin_key, "admin-3b").await.status(),
            StatusCode::FORBIDDEN
        );
        // Unknown target.
        assert_eq!(
            post(&admin_key, "no-such-user").await.status(),
            StatusCode::NOT_FOUND
        );

        // Valid request succeeds; a second concurrent request from the same
        // admin while a session is active is rate-limited (max 1 active).
        assert_eq!(
            post(&admin_key, "dev-3").await.status(),
            StatusCode::CREATED
        );
        let request_id = {
            let requests = state.kv_store.kv_list(REQUEST_NAMESPACE).await.unwrap();
            assert_eq!(requests.len(), 1);
            requests[0].0.clone()
        };

        // Approve to open the session.
        let notifs = body_json(
            app(&state)
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/users/me/notifications")
                        .header("Authorization", format!("Bearer {dev_key}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        let code = notifs["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["notification_type"] == "ImpersonationRequest")
            .map(|n| {
                serde_json::from_str::<serde_json::Value>(n["body"].as_str().unwrap()).unwrap()
                    ["approval_code"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .unwrap();
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/impersonate/{request_id}/approve"))
                    .header("Authorization", format!("Bearer {dev_key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"approval_code": code}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Max one active impersonation per admin.
        assert_eq!(
            post(&admin_key, "dev-3").await.status(),
            StatusCode::CONFLICT
        );
    }

    #[tokio::test]
    async fn end_session_revokes_token_and_emits_ended() {
        let (state, admin_key, _target_key, request_id, token) = run_full_flow().await;

        // Admin explicitly ends the session.
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/admin/impersonate/{request_id}"))
                    .header("Authorization", format!("Bearer {admin_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json["ended"], true);
        assert!(json["duration_secs"].as_u64().is_some());
        assert_eq!(json["actions_taken"].as_u64(), Some(0));

        // The token is revoked immediately (before its exp).
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // ImpersonationEnded audit event with duration and action count.
        let events = state
            .audit
            .query(&audit_filter("impersonation_ended"))
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.user_id.as_ref().unwrap().as_str(), "admin-1");
        assert_eq!(e.detail["impersonator_id"], "admin-1");
        assert_eq!(e.detail["target_user_id"], "target-1");
        assert!(e.detail["duration_secs"].as_u64().is_some());
        assert_eq!(e.detail["actions_taken"], 0);
        assert_eq!(e.detail["reason"], "ended_by_admin");
    }

    #[tokio::test]
    async fn non_impersonator_admin_cannot_end_session() {
        let (state, _admin_key, _target_key, request_id, _token) = run_full_flow().await;
        let other_admin = seed_user(
            &state,
            "admin-other",
            "Other Admin",
            vec![UserRole::Admin],
            false,
        )
        .await;

        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/admin/impersonate/{request_id}"))
                    .header("Authorization", format!("Bearer {other_admin}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn expired_sessions_auto_end_via_sweep() {
        let (state, _admin_key, _target_key, _request_id, token) = run_full_flow().await;

        // Force the session past its TTL.
        let pairs = state.kv_store.kv_list(SESSION_NAMESPACE).await.unwrap();
        let (sid, raw) = pairs[0].clone();
        let mut session: ImpersonationSession = serde_json::from_str(&raw).unwrap();
        session.expires_at = crate::api::now_secs() - 1;
        state
            .kv_store
            .kv_set(
                SESSION_NAMESPACE,
                &sid,
                serde_json::to_string(&session).unwrap(),
            )
            .await
            .unwrap();

        // The expired token is rejected (zero-leeway exp validation).
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // The sweep ends the session and emits ImpersonationEnded.
        let ended = impersonation::expire_sessions(&state).await.unwrap();
        assert_eq!(ended, 1);
        let events = state
            .audit
            .query(&audit_filter("impersonation_ended"))
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].detail["reason"], "expired");

        // Stale pending requests are expired too: approve after expiry is
        // rejected with a conflict, not a token.
        let admin_key = seed_user(
            &state,
            "admin-4",
            "Admin Four",
            vec![UserRole::Admin],
            false,
        )
        .await;
        let target_key = seed_user(
            &state,
            "target-4",
            "Target Four",
            vec![UserRole::Developer],
            true,
        )
        .await;
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/impersonate")
                    .header("Authorization", format!("Bearer {admin_key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"target_user_id": "target-4"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let request_id = body_json(resp).await["request_id"]
            .as_str()
            .unwrap()
            .to_string();

        // Force the request past its approval window.
        let raw = state
            .kv_store
            .kv_get(REQUEST_NAMESPACE, &request_id)
            .await
            .unwrap()
            .unwrap();
        let mut req: ImpersonationRequest = serde_json::from_str(&raw).unwrap();
        req.expires_at = crate::api::now_secs() - 1;
        state
            .kv_store
            .kv_set(
                REQUEST_NAMESPACE,
                &request_id,
                serde_json::to_string(&req).unwrap(),
            )
            .await
            .unwrap();

        impersonation::expire_sessions(&state).await.unwrap();

        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/impersonate/{request_id}/approve"))
                    .header("Authorization", format!("Bearer {target_key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"approval_code": "12345678"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn approval_code_is_eight_digits_and_hashed_at_rest() {
        let code = impersonation::generate_approval_code();
        assert_eq!(code.len(), 8);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
        assert_ne!(
            code,
            impersonation::hash_approval_code(&code),
            "stored hash must differ from the raw code"
        );
    }

    #[tokio::test]
    async fn notification_type_round_trip() {
        assert_eq!(
            NotificationType::ImpersonationRequest.as_str(),
            "ImpersonationRequest"
        );
        assert_eq!(
            NotificationType::parse("ImpersonationAlert"),
            Some(NotificationType::ImpersonationAlert)
        );
        assert_eq!(
            NotificationType::ImpersonationRequest.default_priority_rank(),
            2
        );
        assert_eq!(
            NotificationType::ImpersonationAlert.default_priority_rank(),
            1
        );
    }

    // ── review repairs ──────────────────────────────────────────────────

    /// The impersonation token must work through the FULL production
    /// layering (require_auth_middleware → last_seen → abac → handler),
    /// not just through the ABAC-only test router. Previously
    /// `require_auth_middleware` had no impersonation branch and rejected
    /// the token with 401 before any handler ran.
    #[tokio::test]
    async fn impersonation_works_through_production_router() {
        let (state, _admin_key, _target_key, _request_id, token) = run_full_flow().await;
        crate::abac_middleware::seed_builtin_policies(&state)
            .await
            .expect("seed built-in policies");

        let app = crate::build_router(state.clone());

        // Evaluated route (user resource, Developer role allowed by the
        // seeded policies): must NOT be 401-rejected at require_auth.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // ABAC-exempt route — reading the target user's private
        // notifications under the impersonation token.
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/users/me/notifications")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // BOTH calls are attributed (fire-and-forget — poll for the
        // writes to land): one on the evaluated surface with the policy
        // outcome, one on the exempt surface.
        let mut events = vec![];
        for _ in 0..100 {
            events = state
                .audit
                .query(&audit_filter("impersonation_action"))
                .await
                .unwrap();
            if events.len() >= 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(
            events.len(),
            2,
            "evaluated + exempt impersonated calls must each be audited"
        );
        let surfaces: Vec<&str> = events
            .iter()
            .map(|e| e.detail["surface"].as_str().unwrap())
            .collect();
        assert!(surfaces.contains(&"abac"), "surfaces: {surfaces:?}");
        assert!(surfaces.contains(&"abac-exempt"), "surfaces: {surfaces:?}");
        for e in &events {
            assert_eq!(e.user_id.as_ref().unwrap().as_str(), "admin-1");
            assert_eq!(e.detail["impersonator_id"], "admin-1");
            assert_eq!(e.detail["target_user_id"], "target-1");
        }

        // No double-counting from the require_auth / last_seen / handler
        // extractions of the same requests.
        let sessions = state.kv_store.kv_list(SESSION_NAMESPACE).await.unwrap();
        let session: ImpersonationSession = serde_json::from_str(&sessions[0].1).unwrap();
        assert_eq!(session.actions_taken, 2);
    }

    /// Surfaces that authenticate via the `AuthenticatedAgent` extractor
    /// WITHOUT the ABAC middleware (git smart HTTP, MCP, conversations,
    /// explorer, ws-ticket) must attribute impersonated calls at the
    /// auth-resolution layer.
    #[tokio::test]
    async fn per_handler_surfaces_are_attributed() {
        let (state, _admin_key, _target_key, _request_id, token) = run_full_flow().await;

        async fn ok_handler(auth: crate::auth::AuthenticatedAgent) -> StatusCode {
            assert!(
                auth.impersonation.is_some(),
                "impersonation token must resolve with an ImpersonationContext"
            );
            StatusCode::OK
        }
        // Same wiring as git_http::git_info_refs / mcp::mcp_handler:
        // per-handler auth extractor, no ABAC middleware.
        let router = Router::new()
            .route(
                "/git/:workspace_slug/:repo_name/info/refs",
                axum::routing::get(ok_handler),
            )
            .with_state(state.clone());

        let resp = router
            .oneshot(
                Request::builder()
                    .uri("/git/imp-test/ws-imp-target-1/info/refs")
                    .header("Authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let mut found = None;
        for _ in 0..100 {
            let events = state
                .audit
                .query(&audit_filter("impersonation_action"))
                .await
                .unwrap();
            if let Some(e) = events.iter().find(|e| e.detail["surface"] == "per-handler") {
                found = Some(e.clone());
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let e = found.expect("per-handler impersonated call must be audited");
        assert_eq!(e.user_id.as_ref().unwrap().as_str(), "admin-1");
        assert_eq!(e.detail["method"], "GET");
        assert_eq!(e.detail["path"], "/git/imp-test/ws-imp-target-1/info/refs");

        let sessions = state.kv_store.kv_list(SESSION_NAMESPACE).await.unwrap();
        let session: ImpersonationSession = serde_json::from_str(&sessions[0].1).unwrap();
        assert_eq!(session.actions_taken, 1);
    }

    /// A non-target caller must not learn an impersonation request's
    /// status: the identity check runs BEFORE any status or expiry is
    /// revealed (403 for a non-target, whether the request is pending,
    /// approved, denied or expired — not a 409 that leaks consent state).
    #[tokio::test]
    async fn non_target_cannot_probe_request_status() {
        let state = test_state();
        let admin_key = seed_user(
            &state,
            "admin-5",
            "Admin Five",
            vec![UserRole::Admin],
            false,
        )
        .await;
        let target_key = seed_user(
            &state,
            "target-5",
            "Target Five",
            vec![UserRole::Developer],
            true,
        )
        .await;
        let other_key = seed_user(
            &state,
            "other-5",
            "Other Five",
            vec![UserRole::Developer],
            true,
        )
        .await;

        let post_approve = |key: String, request_id: String, code: String| {
            let state = state.clone();
            async move {
                app(&state)
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri(format!("/api/v1/admin/impersonate/{request_id}/approve"))
                            .header("Authorization", format!("Bearer {key}"))
                            .header("Content-Type", "application/json")
                            .body(Body::from(
                                serde_json::json!({"approval_code": code}).to_string(),
                            ))
                            .unwrap(),
                    )
                    .await
                    .unwrap()
            }
        };

        // Admin requests; target reads the code from the notification.
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/impersonate")
                    .header("Authorization", format!("Bearer {admin_key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"target_user_id": "target-5"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let request_id = body_json(resp).await["request_id"]
            .as_str()
            .unwrap()
            .to_string();
        let notifs = body_json(
            app(&state)
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/users/me/notifications")
                        .header("Authorization", format!("Bearer {target_key}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        let code = notifs["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["notification_type"] == "ImpersonationRequest")
            .map(|n| {
                serde_json::from_str::<serde_json::Value>(n["body"].as_str().unwrap()).unwrap()
                    ["approval_code"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .unwrap();

        // Target approves — the request becomes Approved.
        let resp = post_approve(target_key.clone(), request_id.clone(), code.clone()).await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        // A non-target probing the APPROVED request with the correct code
        // gets 403 Forbidden — not 409 Conflict revealing that consent was
        // already given.
        let resp = post_approve(other_key.clone(), request_id.clone(), code.clone()).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Same for a request the target DENIED: non-target gets 403, not
        // the "no longer pending" conflict.
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/impersonate")
                    .header("Authorization", format!("Bearer {admin_key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"target_user_id": "target-5"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CONFLICT,
            "admin already has an active session"
        );

        // End the active session so a second request can be created.
        let sessions = state.kv_store.kv_list(SESSION_NAMESPACE).await.unwrap();
        let (sid, _raw) = sessions[0].clone();
        impersonation::end_session(&state, &sid, "test")
            .await
            .unwrap();
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/impersonate")
                    .header("Authorization", format!("Bearer {admin_key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"target_user_id": "target-5"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let request_id2 = body_json(resp).await["request_id"]
            .as_str()
            .unwrap()
            .to_string();

        // Target denies.
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/admin/impersonate/{request_id2}/deny"))
                    .header("Authorization", format!("Bearer {target_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Non-target probing the DENIED request → 403, not 409.
        let resp = post_approve(other_key, request_id2, code).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    /// KV adapter that yields before every operation — models a real async
    /// backend (SQLite/Postgres) where the load→increment→save
    /// read-modify-write genuinely interleaves across concurrent tasks.
    /// The sync in-memory adapter cannot reproduce the lost-update race
    /// this test guards against.
    struct YieldingKvStore {
        inner: Arc<dyn gyre_ports::KvJsonStore>,
    }

    impl YieldingKvStore {
        fn new(inner: Arc<dyn gyre_ports::KvJsonStore>) -> Self {
            Self { inner }
        }
    }

    #[axum::async_trait]
    impl gyre_ports::KvJsonStore for YieldingKvStore {
        async fn kv_set(&self, namespace: &str, key: &str, value: String) -> anyhow::Result<()> {
            tokio::task::yield_now().await;
            self.inner.kv_set(namespace, key, value).await
        }

        async fn kv_get(&self, namespace: &str, key: &str) -> anyhow::Result<Option<String>> {
            tokio::task::yield_now().await;
            self.inner.kv_get(namespace, key).await
        }

        async fn kv_remove(&self, namespace: &str, key: &str) -> anyhow::Result<()> {
            self.inner.kv_remove(namespace, key).await
        }

        async fn kv_list(&self, namespace: &str) -> anyhow::Result<Vec<(String, String)>> {
            self.inner.kv_list(namespace).await
        }

        async fn kv_clear(&self, namespace: &str) -> anyhow::Result<()> {
            self.inner.kv_clear(namespace).await
        }
    }

    /// Concurrent impersonated calls must not lose `actions_taken` counts
    /// (the increment is a read-modify-write serialized by the session
    /// write lock; an unlocked RMW lets concurrent calls persist the same
    /// count and underreport the session in ImpersonationEnded).
    #[tokio::test]
    async fn concurrent_impersonated_actions_are_counted_atomically() {
        let (state, _admin_key, _target_key, _request_id, _token) = run_full_flow().await;

        // Swap in the yielding adapter so the read-modify-write actually
        // interleaves across the concurrent tasks below.
        let mut yielding_state = (*state).clone();
        yielding_state.kv_store = Arc::new(YieldingKvStore::new(state.kv_store.clone()));
        let state = Arc::new(yielding_state);

        let sessions = state.kv_store.kv_list(SESSION_NAMESPACE).await.unwrap();
        let (_sid, raw) = sessions[0].clone();
        let session: ImpersonationSession = serde_json::from_str(&raw).unwrap();
        let ctx = crate::auth::ImpersonationContext {
            impersonator_id: gyre_common::Id::new(session.impersonator_id.clone()),
            impersonator_name: session.impersonator_name.clone(),
            target_user_id: gyre_common::Id::new(session.target_user_id.clone()),
            session_id: session.session_id.clone(),
        };

        // 8 concurrent recordings of impersonated actions.
        let mut handles = vec![];
        for i in 0..8 {
            let state = state.clone();
            let ctx = ctx.clone();
            handles.push(tokio::spawn(async move {
                impersonation::record_impersonated_action(
                    &state,
                    &ctx,
                    "GET",
                    &format!("/api/v1/some-resource/{i}"),
                    true,
                    "abac",
                )
                .await;
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        let sessions = state.kv_store.kv_list(SESSION_NAMESPACE).await.unwrap();
        let session: ImpersonationSession = serde_json::from_str(&sessions[0].1).unwrap();
        assert_eq!(
            session.actions_taken, 8,
            "every concurrent impersonated action must be counted exactly once"
        );

        // The audit events carry distinct sequential counts 1..=8 — no
        // duplicated (lost-update) counts.
        let events = state
            .audit
            .query(&audit_filter("impersonation_action"))
            .await
            .unwrap();
        let mut counts: Vec<u64> = events
            .iter()
            .map(|e| e.detail["actions_taken"].as_u64().unwrap())
            .collect();
        counts.sort_unstable();
        assert_eq!(counts, vec![1, 2, 3, 4, 5, 6, 7, 8]);

        // Ending the session afterwards reports the full count.
        impersonation::end_session(&state, &session.session_id, "test")
            .await
            .unwrap();
        let events = state
            .audit
            .query(&audit_filter("impersonation_ended"))
            .await
            .unwrap();
        assert_eq!(events[0].detail["actions_taken"], 8);
    }

    /// Two concurrent approvals of the same request must yield exactly
    /// one session, one token and one `ImpersonationStarted` event: the
    /// session-creation critical section is serialized by the session
    /// write lock. The yielding adapter makes the unlocked
    /// read-modify-write genuinely interleave (as on a real async
    /// backend); without the lock, both approvals load the request as
    /// Pending and both mint live tokens — two impersonation sessions
    /// for one consent, violating the max-1-active-per-admin constraint.
    #[tokio::test]
    async fn concurrent_approvals_create_exactly_one_session() {
        let state = test_state();
        let admin_key =
            seed_user(&state, "admin-6", "Admin Six", vec![UserRole::Admin], false).await;
        let target_key = seed_user(
            &state,
            "target-6",
            "Target Six",
            vec![UserRole::Developer],
            true,
        )
        .await;

        // Admin requests impersonation; target reads the code from the
        // notification.
        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/impersonate")
                    .header("Authorization", format!("Bearer {admin_key}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"target_user_id": "target-6"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let request_id = body_json(resp).await["request_id"]
            .as_str()
            .unwrap()
            .to_string();
        let notifs = body_json(
            app(&state)
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/users/me/notifications")
                        .header("Authorization", format!("Bearer {target_key}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        let code = notifs["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["notification_type"] == "ImpersonationRequest")
            .map(|n| {
                serde_json::from_str::<serde_json::Value>(n["body"].as_str().unwrap()).unwrap()
                    ["approval_code"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .unwrap();

        // Swap in the yielding adapter so the two concurrent approvals
        // interleave on the request load/save (the sync mem adapter
        // cannot reproduce the race).
        let mut yielding_state = (*state).clone();
        yielding_state.kv_store = Arc::new(YieldingKvStore::new(state.kv_store.clone()));
        let state = Arc::new(yielding_state);

        let caller = Id::new("target-6");
        let spawn_approve = |code: String| {
            let state = state.clone();
            let request_id = request_id.clone();
            let caller = caller.clone();
            tokio::spawn(async move {
                impersonation::approve_request(&state, &request_id, &caller, &code).await
            })
        };
        let (a, b) = (spawn_approve(code.clone()), spawn_approve(code.clone()));
        let results = [a.await.unwrap(), b.await.unwrap()];

        let ok = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(ok, 1, "exactly one approval must succeed");
        let conflicts = results
            .iter()
            .filter(|r| matches!(r, Err(ApiError::Conflict(_))))
            .count();
        assert_eq!(conflicts, 1, "the losing approval must conflict");

        // Exactly one live session and one ImpersonationStarted event —
        // not two tokens for one consent.
        let sessions = state.kv_store.kv_list(SESSION_NAMESPACE).await.unwrap();
        assert_eq!(sessions.len(), 1);
        let events = state
            .audit
            .query(&audit_filter("impersonation_started"))
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
    }
}
