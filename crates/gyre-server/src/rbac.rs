//! Role-based access control extractors for gyre-server.
//!
//! Role hierarchy (highest to lowest):
//!   Admin > Developer > Agent > ReadOnly
//!
//! Each extractor first resolves the authenticated user via [`AuthenticatedAgent`],
//! then checks whether the user's roles satisfy the required permission level.
//! Returns 403 Forbidden with `{"error":"insufficient permissions"}` on failure.

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use gyre_domain::{AuditEvent, AuditEventType, AuditOutcome, UserRole};
use serde_json::json;
use std::sync::Arc;

use crate::{auth::AuthenticatedAgent, AppState};

/// Record an `RbacDenied` audit event (observability.md §Audit Event Types:
/// role check rejects request) with the spec'd detail fields: user_id,
/// required_role, actual_role, endpoint.
///
/// Called from every live role-check rejection point — the extractors below
/// and the per-handler role guards in `api/` — so a role denial always lands
/// in the audit trail.
pub async fn record_rbac_denied(
    state: &AppState,
    auth: &AuthenticatedAgent,
    required_role: &str,
    endpoint: &str,
) {
    let actual_role = auth
        .roles
        .first()
        .map(|r| r.as_str().to_string())
        .unwrap_or_else(|| "none".to_string());
    let event = AuditEvent::new(
        gyre_common::Id::new(uuid::Uuid::new_v4().to_string()),
        AuditEventType::RbacDenied,
        if auth.agent_id.is_empty() {
            None
        } else {
            Some(gyre_common::Id::new(auth.agent_id.clone()))
        },
        auth.user_id.clone(),
        None,
        None,
        None,
        "endpoint".to_string(),
        Some(endpoint.to_string()),
        AuditOutcome::Blocked,
        json!({
            "user_id": auth.user_id.as_ref().map(|id| id.to_string()),
            "agent_id": auth.agent_id,
            "required_role": required_role,
            "actual_role": actual_role,
            "endpoint": endpoint,
        }),
        None,
        None,
        crate::api::now_secs(),
    );
    crate::api::audit::emit_server_audit_event(state, event).await;
}

#[allow(dead_code)]
fn forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({"error": "insufficient permissions"})),
    )
        .into_response()
}

// RequireDeveloper extractor removed (M34 Slice 4). ABAC middleware is the
// sole authorization layer. Callers that need developer-level access are now
// enforced via the developer-write-access built-in ABAC policy.

/// Requires Admin, Developer, or Agent role.
#[allow(dead_code)]
pub struct RequireAgent(pub AuthenticatedAgent);

#[axum::async_trait]
impl FromRequestParts<Arc<AppState>> for RequireAgent {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let auth = AuthenticatedAgent::from_request_parts(parts, state).await?;
        let ok = auth.agent_id == "system"
            || auth.roles.contains(&UserRole::Admin)
            || auth.roles.contains(&UserRole::Developer)
            || auth.roles.contains(&UserRole::Agent);
        if ok {
            Ok(RequireAgent(auth))
        } else {
            record_rbac_denied(state, &auth, "Agent", parts.uri.path()).await;
            Err(forbidden())
        }
    }
}

/// Requires any authenticated user (ReadOnly and above).
#[allow(dead_code)]
pub struct RequireReadOnly(pub AuthenticatedAgent);

#[axum::async_trait]
impl FromRequestParts<Arc<AppState>> for RequireReadOnly {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let auth = AuthenticatedAgent::from_request_parts(parts, state).await?;
        Ok(RequireReadOnly(auth))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt},
        mem::test_state,
    };
    use axum::{body::Body, http::StatusCode, routing::get, Router};
    use http::Request;
    use tower::ServiceExt;

    use super::*;

    async fn agent_handler(_: RequireAgent) -> StatusCode {
        StatusCode::OK
    }

    fn app_with_jwt() -> Router {
        let state = make_test_state_with_jwt();
        Router::new()
            .route("/agent", get(agent_handler))
            .with_state(state)
    }

    fn agent_jwt() -> String {
        sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "default",
                "sub": "agent-sub",
                "preferred_username": "agent-user",
                "realm_access": { "roles": ["agent"] }
            }),
            3600,
        )
    }

    fn readonly_jwt() -> String {
        sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "default",
                "sub": "ro-sub",
                "preferred_username": "readonly-user",
                "realm_access": { "roles": ["readonly"] }
            }),
            3600,
        )
    }

    #[tokio::test]
    async fn agent_can_access_agent_endpoint() {
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/agent")
                    .header("Authorization", format!("Bearer {}", agent_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn readonly_cannot_access_agent_endpoint() {
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/agent")
                    .header("Authorization", format!("Bearer {}", readonly_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn global_token_can_access_agent_endpoint() {
        let state = test_state();
        let app = Router::new()
            .route("/agent", get(agent_handler))
            .with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/agent")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn no_auth_returns_401_on_agent_endpoint() {
        let resp = app_with_jwt()
            .oneshot(
                Request::builder()
                    .uri("/agent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // -- task-104: RbacDenied audit events -----------------------------------

    async fn rbac_denied_events(state: &crate::AppState) -> Vec<gyre_domain::AuditEvent> {
        gyre_ports::AuditRepository::query(
            &*state.audit,
            &gyre_ports::AuditQueryFilter {
                event_type: Some("rbac_denied".to_string()),
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn role_check_failure_records_rbac_denied_audit_event() {
        // The RequireAgent extractor rejects a ReadOnly caller — the
        // rejection must be audited with the spec'd detail fields.
        let state = make_test_state_with_jwt();
        let app = Router::new()
            .route("/agent", get(agent_handler))
            .with_state(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/agent")
                    .header("Authorization", format!("Bearer {}", readonly_jwt()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let events = rbac_denied_events(&state).await;
        assert_eq!(events.len(), 1, "one rbac_denied event");
        let e = &events[0];
        assert_eq!(e.detail["required_role"], "Agent");
        assert_eq!(e.detail["actual_role"], "ReadOnly");
        assert_eq!(e.detail["endpoint"], "/agent");
        assert_eq!(e.outcome, gyre_domain::AuditOutcome::Blocked);
    }

    #[tokio::test]
    async fn handler_role_guard_records_rbac_denied_audit_event() {
        // Per-handler role guards (api/ handlers) are live role checks —
        // a Developer JWT creating a tenant must be audited as RbacDenied.
        let state = make_test_state_with_jwt();
        let app = crate::api::api_router().with_state(state.clone());
        let token = sign_test_jwt(
            &serde_json::json!({
                "sub": "dev-sub",
                "preferred_username": "dev-user",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tenants")
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&serde_json::json!({
                            "name": "t",
                            "slug": "t"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        let events = rbac_denied_events(&state).await;
        assert_eq!(events.len(), 1, "one rbac_denied event");
        let e = &events[0];
        assert_eq!(e.detail["required_role"], "Admin");
        assert_eq!(e.detail["actual_role"], "Developer");
        assert_eq!(e.detail["endpoint"], "POST /api/v1/tenants");
        assert_eq!(e.outcome, gyre_domain::AuditOutcome::Blocked);
    }

    async fn assert_one_rbac_denied(state: &crate::AppState, required_role: &str, endpoint: &str) {
        let events = rbac_denied_events(state).await;
        assert_eq!(events.len(), 1, "one rbac_denied event for {endpoint}");
        let e = &events[0];
        assert_eq!(e.detail["required_role"], required_role);
        assert_eq!(e.detail["endpoint"], endpoint);
        assert_eq!(e.outcome, gyre_domain::AuditOutcome::Blocked);
    }

    #[tokio::test]
    async fn uninstrumented_role_guards_record_rbac_denied_audit_event() {
        // Review repair (task-104 round 8): five pure role guards rejected
        // with 403 while recording no rbac_denied event, contradicting the
        // spec trigger "Role check rejects request". Each sub-case gets a
        // fresh state so the recorded event count is exactly one.
        let developer_jwt = sign_test_jwt(
            &serde_json::json!({
                "sub": "dev-sub",
                "preferred_username": "dev-user",
                "realm_access": { "roles": ["developer"] }
            }),
            3600,
        );

        // 1. Reviewer's probe: Developer JWT listing the meta-specs registry
        //    (ABAC-exempt route — the handler guard is the only auth surface).
        {
            let state = make_test_state_with_jwt();
            let app = crate::api::api_router().with_state(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/meta-specs-registry")
                        .header("Authorization", format!("Bearer {developer_jwt}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            assert_one_rbac_denied(&state, "Admin", "GET /api/v1/meta-specs-registry").await;
        }

        // 2. ReadOnly caller running a meta-spec preview (Admin-or-Developer).
        {
            let state = make_test_state_with_jwt();
            let app = crate::api::api_router().with_state(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/workspaces/ws-1/meta-specs/preview")
                        .header("Authorization", format!("Bearer {}", readonly_jwt()))
                        .header("Content-Type", "application/json")
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            assert_one_rbac_denied(
                &state,
                "Admin or Developer",
                "POST /api/v1/workspaces/:id/meta-specs/preview",
            )
            .await;
        }

        // 3. ReadOnly caller approving a spec (minimum approving role: Agent).
        {
            let state = make_test_state_with_jwt();
            let app = crate::api::api_router().with_state(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/specs/system%2Fdesign-principles.md/approve")
                        .header("Authorization", format!("Bearer {}", readonly_jwt()))
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::to_string(&serde_json::json!({
                                "sha": "a".repeat(40)
                            }))
                            .unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            assert_one_rbac_denied(&state, "Agent", "POST /api/v1/specs/:path/approve").await;
        }

        // 4. Agent caller rejecting a spec (Admin-or-Developer required).
        {
            let state = make_test_state_with_jwt();
            let app = crate::api::api_router().with_state(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/specs/system%2Fdesign-principles.md/reject")
                        .header("Authorization", format!("Bearer {}", agent_jwt()))
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::to_string(&serde_json::json!({ "reason": "out of scope" }))
                                .unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            assert_one_rbac_denied(
                &state,
                "Admin or Developer",
                "POST /api/v1/specs/:path/reject",
            )
            .await;
        }

        // 5. Agent caller setting an MR to Approved (human-only escape from
        //    the supervised trust gate; the guard is a live role check).
        {
            let state = make_test_state_with_jwt();
            // create_mr resolves the MR's workspace scope from its
            // repository and refuses unknown repositories (M34 hierarchy
            // enforcement) — seed the repository the probe MR targets.
            let repo = gyre_domain::Repository::new(
                gyre_common::Id::new("repo-1"),
                gyre_common::Id::new("ws1"),
                "repo-1",
                "/repos/repo-1",
                1000,
            );
            state.repos.create(&repo).await.unwrap();
            let app = crate::api::api_router().with_state(state.clone());
            // create_mr has no auth extractor, so seed the MR without a token.
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/merge-requests")
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::to_string(&serde_json::json!({
                                "repository_id": "repo-1",
                                "title": "agent approval probe",
                                "source_branch": "feat/x",
                                "target_branch": "main"
                            }))
                            .unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::CREATED);
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            let mr_id = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string();
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/api/v1/merge-requests/{mr_id}/status"))
                        .header("Authorization", format!("Bearer {}", agent_jwt()))
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::to_string(&serde_json::json!({ "status": "approved" }))
                                .unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            assert_one_rbac_denied(&state, "non-Agent", "PUT /api/v1/merge-requests/:id/status")
                .await;
        }
    }

    #[tokio::test]
    async fn round9_uninstrumented_role_guards_record_rbac_denied_audit_event() {
        // Review repair (task-104 round 9): live pure role guards that
        // rejected with 403 while recording no rbac_denied event —
        // update_peer / delete_peer (network.rs), put_rollout_policy
        // (meta_specs.rs, arrived via the task-157 base merge), the secrets
        // require_admin helper covering four endpoints (secrets.rs, arrived
        // via the task-098 base merge), and the broadcast-destination guard
        // (messages.rs). Same class as the round-8 finding. Each sub-case
        // gets a fresh state so the recorded event count is exactly one.
        // A valid 44-char base64 Curve25519 pubkey (32 bytes).
        const VALID_PUBKEY: &str = "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=";

        // Register a peer for `agent_id` using the global Admin token.
        async fn register_peer(app: &Router, agent_id: &str) -> String {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/network/peers")
                        .header("Authorization", "Bearer test-token")
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::to_string(&serde_json::json!({
                                "agent_id": agent_id,
                                "wireguard_pubkey": VALID_PUBKEY,
                            }))
                            .unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::CREATED);
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string()
        }

        // 1. Per-agent uuid token updating another agent's peer record
        //    (reviewer's probe: non-JWT, non-Admin caller -> the Admin role
        //    guard is the operative enforcement).
        {
            let state = make_test_state_with_jwt();
            state
                .kv_store
                .kv_set(
                    "agent_tokens",
                    "other-agent",
                    "other-agent-secret".to_string(),
                )
                .await
                .unwrap();
            let app = crate::api::api_router().with_state(state.clone());
            let peer_id = register_peer(&app, "peer-owner").await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/api/v1/network/peers/{peer_id}"))
                        .header("Authorization", "Bearer other-agent-secret")
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::to_string(&serde_json::json!({
                                "endpoint": "10.0.0.9:51820"
                            }))
                            .unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            assert_one_rbac_denied(&state, "Admin", "PUT /api/v1/network/peers/:id").await;
        }

        // 2. Per-agent uuid token deleting another agent's peer record.
        {
            let state = make_test_state_with_jwt();
            state
                .kv_store
                .kv_set(
                    "agent_tokens",
                    "other-agent",
                    "other-agent-secret".to_string(),
                )
                .await
                .unwrap();
            let app = crate::api::api_router().with_state(state.clone());
            let peer_id = register_peer(&app, "peer-owner").await;
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("DELETE")
                        .uri(format!("/api/v1/network/peers/{peer_id}"))
                        .header("Authorization", "Bearer other-agent-secret")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            assert_one_rbac_denied(&state, "Admin", "DELETE /api/v1/network/peers/:id").await;
        }

        // 3. Developer JWT updating a workspace rollout policy (the guard
        //    arrived via the task-157 base merge).
        {
            let state = make_test_state_with_jwt();
            let app = crate::api::api_router().with_state(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri("/api/v1/workspaces/ws-1/meta-specs/rollout-policy")
                        .header(
                            "Authorization",
                            format!(
                                "Bearer {}",
                                sign_test_jwt(
                                    &serde_json::json!({
                                        "sub": "dev-sub",
                                        "preferred_username": "dev-user",
                                        "realm_access": { "roles": ["developer"] }
                                    }),
                                    3600,
                                )
                            ),
                        )
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::to_string(&serde_json::json!({
                                "strategy": "manual"
                            }))
                            .unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            assert_one_rbac_denied(
                &state,
                "Admin",
                "PUT /api/v1/workspaces/:id/meta-specs/rollout-policy",
            )
            .await;
        }

        // 4. Developer JWT listing secrets (Admin-only via the shared
        //    require_admin helper — one guard, four endpoints).
        {
            let state = make_test_state_with_jwt();
            let app = crate::api::api_router().with_state(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/secrets?scope=tenant")
                        .header(
                            "Authorization",
                            format!(
                                "Bearer {}",
                                sign_test_jwt(
                                    &serde_json::json!({
                                        "sub": "dev-sub",
                                        "preferred_username": "dev-user",
                                        "realm_access": { "roles": ["developer"] }
                                    }),
                                    3600,
                                )
                            ),
                        )
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            assert_one_rbac_denied(&state, "Admin", "GET /api/v1/secrets").await;
        }

        // 5. Per-agent uuid token sending a broadcast message (only Server
        //    origin or Admin may broadcast).
        {
            let state = make_test_state_with_jwt();
            state
                .kv_store
                .kv_set(
                    "agent_tokens",
                    "other-agent",
                    "other-agent-secret".to_string(),
                )
                .await
                .unwrap();
            let app = crate::api::api_router().with_state(state.clone());
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/workspaces/ws-1/messages")
                        .header("Authorization", "Bearer other-agent-secret")
                        .header("Content-Type", "application/json")
                        .body(Body::from(
                            serde_json::to_string(&serde_json::json!({
                                "to": "broadcast",
                                "kind": "task104_round9_probe"
                            }))
                            .unwrap(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::FORBIDDEN);
            assert_one_rbac_denied(
                &state,
                "Server or Admin",
                "POST /api/v1/workspaces/:workspace_id/messages",
            )
            .await;
        }
    }
}
