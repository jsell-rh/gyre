//! Message bus REST API — TASK-239 (Phase 3)
//!
//! Endpoints:
//!   POST /api/v1/workspaces/:workspace_id/messages      — unified send
//!   GET  /api/v1/agents/:id/messages                   — cursor poll (replaces drain)
//!   PUT  /api/v1/agents/:id/messages/:message_id/ack   — idempotent ack
//!   GET  /api/v1/workspaces/:id/messages               — workspace event query

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::{
    message::{Destination, Message, MessageKind, MessageOrigin, MessageTier},
    Id,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{auth::AuthenticatedAgent, signing::sign_message, AppState};

use super::error::ApiError;

// ── helpers ───────────────────────────────────────────────────────────────────

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Derive MessageOrigin from the authenticated caller.
///
/// Message-bus.md §Origin and tenant resolution from auth context: the
/// origin follows the AUTH MECHANISM the extractor verified — never role
/// membership or identity strings (a user named "system", or an Admin-role
/// API-key user, must not receive Server origin):
///   - global GYRE_AUTH_TOKEN            → Server (full forge authority)
///   - API key (gyre_*)                  → User(user_id from key lookup)
///   - Keycloak/OIDC JWT                 → User(user_id from token)
///   - agent JWT / per-agent token       → Agent(sub)
///   - impersonation JWT                 → User(target user)
///   - federated instance JWT            → Agent(remote subject)
///
/// The Admin role is a SEPARATE axis: the Broadcast guard in the send
/// handler still allows Admin-role API-key callers to target Broadcast
/// (scoping rule 6) via its own role check — but they resolve to User
/// origin, so server_only kinds and the rule-3 Directed-membership check
/// apply to them like any other user.
fn origin_from_auth(auth: &AuthenticatedAgent) -> MessageOrigin {
    use crate::auth::AuthMechanism;
    match auth.auth_mechanism {
        AuthMechanism::GlobalToken => MessageOrigin::Server,
        AuthMechanism::ApiKey | AuthMechanism::UserJwt | AuthMechanism::Impersonation => {
            match &auth.user_id {
                Some(user_id) => MessageOrigin::User(user_id.clone()),
                // Defensive: these mechanisms always resolve a user row; if
                // one ever reaches here without one, fall back to the
                // non-Server agent shape rather than granting forge authority.
                None => MessageOrigin::Agent(Id::new(&auth.agent_id)),
            }
        }
        AuthMechanism::AgentToken | AuthMechanism::FederatedJwt => {
            MessageOrigin::Agent(Id::new(&auth.agent_id))
        }
    }
}

/// Build a response Message that excludes `acknowledged` (per spec: excluded from POST responses).
fn send_response(msg: Message) -> Value {
    let mut v = serde_json::to_value(&msg).unwrap_or_default();
    if let Some(obj) = v.as_object_mut() {
        obj.remove("acknowledged");
    }
    v
}

// ── send endpoint ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub to: Value,
    pub kind: String,
    /// For Custom kinds: "directed" to opt into ack-based delivery.
    pub tier: Option<String>,
    pub payload: Option<Value>,
}

/// POST /api/v1/workspaces/:workspace_id/messages
pub async fn send_message(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let ws_id = Id::new(&workspace_id);

    // Parse destination from request body.
    let to = parse_destination(&req.to)?;

    // Parse kind.
    let kind: MessageKind = serde_json::from_value(Value::String(req.kind.clone()))
        .map_err(|_| ApiError::BadRequest(format!("unknown message kind: {}", req.kind)))?;

    // Determine effective tier (Custom can opt into Directed).
    let effective_tier = if let MessageKind::Custom(_) = &kind {
        if req.tier.as_deref() == Some("directed") {
            MessageTier::Directed
        } else {
            MessageTier::Event
        }
    } else {
        kind.tier()
    };

    // Derive origin from auth context.
    let from = origin_from_auth(&auth);

    // server_only() check.
    if kind.server_only() && !matches!(from, MessageOrigin::Server) {
        return Err(ApiError::Forbidden(format!(
            "kind '{}' can only be sent by the server",
            req.kind
        )));
    }

    // Tier + destination constraints.
    match (&effective_tier, &to) {
        (MessageTier::Directed, Destination::Workspace(_)) => {
            return Err(ApiError::BadRequest(
                "Directed tier requires Agent destination, not Workspace".to_string(),
            ));
        }
        (MessageTier::Directed, Destination::Broadcast) => {
            return Err(ApiError::BadRequest(
                "Directed tier requires Agent destination, not Broadcast".to_string(),
            ));
        }
        (MessageTier::Telemetry, Destination::Agent(_)) => {
            return Err(ApiError::BadRequest(
                "Telemetry tier cannot target Agent destination".to_string(),
            ));
        }
        (MessageTier::Telemetry, Destination::Broadcast) => {
            return Err(ApiError::BadRequest(
                "Telemetry tier cannot target Broadcast destination".to_string(),
            ));
        }
        _ => {}
    }

    // Broadcast destination: only Server or Admin.
    if matches!(to, Destination::Broadcast) {
        let is_admin = auth.roles.contains(&gyre_domain::UserRole::Admin);
        if !matches!(from, MessageOrigin::Server) && !is_admin {
            crate::rbac::record_rbac_denied(
                &state,
                &auth,
                "Server or Admin",
                "POST /api/v1/workspaces/:workspace_id/messages",
            )
            .await;
            return Err(ApiError::Forbidden(
                "Broadcast destination requires Server origin or Admin role".to_string(),
            ));
        }
    }

    // Validate workspace scoping for Agent destination.
    if let Destination::Agent(ref target_agent_id) = to {
        let target_agent = state
            .agents
            .find_by_id(target_agent_id)
            .await
            .map_err(ApiError::Internal)?
            .ok_or_else(|| ApiError::NotFound(format!("agent {} not found", target_agent_id)))?;

        // Target must be in this workspace (workspace IDs are globally unique
        // and each workspace belongs to exactly one tenant).
        if target_agent.workspace_id != ws_id {
            return Err(ApiError::BadRequest(format!(
                "target agent {} is not in workspace {}",
                target_agent_id, workspace_id
            )));
        }

        // Scoping rule 1 (message-bus.md §Scoping Rules): ALL Agent-directed
        // messages validate tenant isolation — the target agent's tenant
        // (resolved from its workspace record; an agent has no tenant field
        // of its own) must match the message's tenant (the caller's verified
        // tenant), REGARDLESS of origin, including Server-origin callers.
        // This is exactly the server-bug cross-tenant leak class rule 1
        // exists to prevent: a global-token caller whose tenant is T1 must
        // not be able to store a T1-tenant message pointed at a T2 agent.
        // If the target's workspace record cannot be resolved, the tenant
        // scope is indeterminable — the send is REJECTED (fail-closed),
        // never allowed through: workspace deletes do not cascade agents,
        // so a dangling agent in a deleted workspace is a real
        // inconsistent-state leak vector, not a theoretical one. There is
        // no valid tenant to compare against, so allowing the send would be
        // exactly the fail-open ref-resolution class this repo prohibits.
        let target_ws = state
            .workspaces
            .find_by_id(&target_agent.workspace_id)
            .await
            .map_err(ApiError::Internal)?;
        let target_ws = match target_ws {
            Some(ws) => ws,
            None => {
                tracing::warn!(
                    workspace_id = %target_agent.workspace_id,
                    "send: target agent's workspace not found; tenant isolation cannot be verified"
                );
                return Err(ApiError::Forbidden(format!(
                    "target agent {} workspace {} cannot be resolved; tenant isolation cannot be verified",
                    target_agent_id, target_agent.workspace_id
                )));
            }
        };
        if target_ws.tenant_id.as_str() != auth.tenant_id {
            crate::rbac::record_rbac_denied(
                &state,
                &auth,
                "same-tenant target",
                "POST /api/v1/workspaces/:workspace_id/messages",
            )
            .await;
            return Err(ApiError::Forbidden(format!(
                "target agent {} belongs to a different tenant",
                target_agent_id
            )));
        }

        // Agent-to-agent: sender must be in the same workspace.
        if let MessageOrigin::Agent(ref sender_id) = from {
            let sender_agent = state
                .agents
                .find_by_id(sender_id)
                .await
                .map_err(ApiError::Internal)?
                .ok_or_else(|| {
                    ApiError::NotFound(format!("sender agent {} not found", sender_id))
                })?;
            if sender_agent.workspace_id != ws_id {
                return Err(ApiError::Forbidden(
                    "agents can only send Directed messages to agents in the same workspace"
                        .to_string(),
                ));
            }
        }

        // User-to-agent: verify workspace membership.
        if let MessageOrigin::User(ref user_id) = from {
            let membership = state
                .workspace_memberships
                .find_by_user_and_workspace(user_id, &ws_id)
                .await
                .map_err(ApiError::Internal)?;
            if membership.is_none() {
                return Err(ApiError::Forbidden(
                    "not a member of this workspace".to_string(),
                ));
            }
        }

        // Queue depth check for Directed tier.
        if effective_tier == MessageTier::Directed {
            let unacked = state
                .messages
                .count_unacked(target_agent_id)
                .await
                .map_err(ApiError::Internal)?;
            if unacked >= state.agent_inbox_max {
                return Err(ApiError::TooManyRequests(format!(
                    "agent {} inbox is full ({} unacked messages)",
                    target_agent_id, unacked
                )));
            }
        }
    }

    // Workspace fan-out: validate sender membership.
    if let Destination::Workspace(ref dest_ws_id) = to {
        if *dest_ws_id != ws_id {
            return Err(ApiError::BadRequest(
                "workspace destination must match URL workspace".to_string(),
            ));
        }
        // Agents must send to their own workspace.
        if let MessageOrigin::Agent(ref sender_id) = from {
            let sender_agent = state
                .agents
                .find_by_id(sender_id)
                .await
                .map_err(ApiError::Internal)?
                .ok_or_else(|| {
                    ApiError::NotFound(format!("sender agent {} not found", sender_id))
                })?;
            if sender_agent.workspace_id != ws_id {
                return Err(ApiError::Forbidden(
                    "agents can only send to their own workspace".to_string(),
                ));
            }
        }
        // Users: verify workspace membership.
        if let MessageOrigin::User(ref user_id) = from {
            let membership = state
                .workspace_memberships
                .find_by_user_and_workspace(user_id, &ws_id)
                .await
                .map_err(ApiError::Internal)?;
            if membership.is_none() {
                return Err(ApiError::Forbidden(
                    "not a member of this workspace".to_string(),
                ));
            }
        }
    }

    // Payload schema validation (message-bus.md §Payload Schemas): a payload that
    // does not carry its kind's required fields is rejected on receipt with 400,
    // before anything is signed or persisted.
    if let Err(reason) = kind.validate_payload(req.payload.as_ref()) {
        return Err(ApiError::BadRequest(reason));
    }

    let workspace_id_opt = if matches!(to, Destination::Broadcast) {
        None
    } else {
        Some(ws_id.clone())
    };

    let created_at = now_ms();
    let msg_id = Id::new(new_id());

    let mut msg = Message {
        id: msg_id,
        tenant_id: Id::new(&auth.tenant_id),
        from,
        workspace_id: workspace_id_opt,
        to,
        kind,
        payload: req.payload,
        created_at,
        signature: None,
        key_id: None,
        acknowledged: false,
    };

    // Sign Directed and Event tier.
    if effective_tier != MessageTier::Telemetry {
        let (sig, kid) = sign_message(&state, &msg);
        msg.signature = Some(sig);
        msg.key_id = Some(kid);
    }

    // Persist Directed and Event tier only (Telemetry is in-memory, Broadcast
    // is not stored). Broadcast-only kinds (DataSeeded/QueueUpdated) are
    // storage-exempt regardless of destination (message-bus.md
    // §Broadcast-only kinds): they flow through the in-memory broadcast
    // channel only, never hitting MessageRepository::store().
    if effective_tier != MessageTier::Telemetry
        && !matches!(msg.to, Destination::Broadcast)
        && !msg.kind.broadcast_only()
    {
        state
            .messages
            .store(&msg)
            .await
            .map_err(ApiError::Internal)?;
    }

    // Broadcast to WebSocket clients for real-time delivery.
    let _ = state.message_broadcast_tx.send(msg.clone());

    // Dispatch to consumers (notification bridge, etc.). Non-blocking and
    // best-effort: consumer drops under backpressure are logged, never
    // block the send path (message-bus.md §Relationship to Notifications).
    match state.message_dispatch_tx.try_send(msg.clone()) {
        Ok(()) => {}
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
            tracing::warn!(
                "message dispatch channel full; consumer dropped a message (best-effort tier)"
            );
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
            tracing::debug!("message dispatch channel closed; no consumers registered");
        }
    }

    Ok((StatusCode::CREATED, Json(send_response(msg))))
}

fn parse_destination(v: &Value) -> Result<Destination, ApiError> {
    if let Some(obj) = v.as_object() {
        if let Some(agent_id) = obj.get("agent").and_then(|v| v.as_str()) {
            return Ok(Destination::Agent(Id::new(agent_id)));
        }
        if let Some(ws_id) = obj.get("workspace").and_then(|v| v.as_str()) {
            return Ok(Destination::Workspace(Id::new(ws_id)));
        }
    }
    if v.as_str() == Some("broadcast") {
        return Ok(Destination::Broadcast);
    }
    Err(ApiError::BadRequest(
        "invalid 'to' field: expected {\"agent\": \"id\"}, {\"workspace\": \"id\"}, or \"broadcast\"".to_string()
    ))
}

// ── receive endpoint (poll) ───────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct PollQuery {
    pub after_ts: Option<u64>,
    pub after_id: Option<String>,
    pub limit: Option<usize>,
    /// ?acknowledged=false for crash recovery (delegates to list_unacked).
    pub acknowledged: Option<String>,
}

/// GET /api/v1/agents/:id/messages
pub async fn poll_messages(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(agent_id): Path<String>,
    Query(params): Query<PollQuery>,
) -> Result<Json<Vec<Message>>, ApiError> {
    // Agent must be the authenticated caller. Admin callers (role-based, not
    // magic-string) may read any inbox for operational/debugging purposes.
    let is_admin = auth.roles.contains(&gyre_domain::UserRole::Admin);
    if !is_admin && auth.agent_id != agent_id {
        return Err(ApiError::Forbidden(
            "agents can only read their own inbox".to_string(),
        ));
    }

    let aid = Id::new(&agent_id);

    // Verify agent exists.
    state
        .agents
        .find_by_id(&aid)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("agent {agent_id} not found")))?;

    let limit = params.limit.unwrap_or(100).min(1000);

    let messages = if params.acknowledged.as_deref() == Some("false") {
        // Crash recovery: return all unacked messages.
        state
            .messages
            .list_unacked(&aid, limit)
            .await
            .map_err(ApiError::Internal)?
    } else {
        let after_ts = params.after_ts.unwrap_or(0);
        let after_id = params.after_id.as_deref().map(Id::new);
        state
            .messages
            .list_after(&aid, after_ts, after_id.as_ref(), limit)
            .await
            .map_err(ApiError::Internal)?
    };

    Ok(Json(messages))
}

// ── ack endpoint ──────────────────────────────────────────────────────────────

/// PUT /api/v1/agents/:id/messages/:message_id/ack
pub async fn ack_message(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path((agent_id, message_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    // Agent must be the authenticated caller. Admin callers may ack on behalf
    // of any agent (role-based check, not magic-string).
    let is_admin = auth.roles.contains(&gyre_domain::UserRole::Admin);
    if !is_admin && auth.agent_id != agent_id {
        return Err(ApiError::Forbidden(
            "agents can only ack messages in their own inbox".to_string(),
        ));
    }

    let aid = Id::new(&agent_id);
    let mid = Id::new(&message_id);

    // Idempotent ack.
    state
        .messages
        .acknowledge(&mid, &aid)
        .await
        .map_err(ApiError::Internal)?;

    Ok(StatusCode::OK)
}

// ── workspace query endpoint ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct WorkspaceQueryParams {
    pub kind: Option<String>,
    pub since: Option<u64>,
    pub before_ts: Option<u64>,
    pub before_id: Option<String>,
    pub limit: Option<usize>,
}

/// GET /api/v1/workspaces/:id/messages
pub async fn list_workspace_messages(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    Query(params): Query<WorkspaceQueryParams>,
) -> Result<Json<Vec<Message>>, ApiError> {
    let ws_id = Id::new(&workspace_id);

    // Verify workspace membership for non-Admin callers (role-based, not magic-string).
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        let is_member = if let Some(ref user_id) = auth.user_id {
            state
                .workspace_memberships
                .find_by_user_and_workspace(user_id, &ws_id)
                .await
                .map_err(ApiError::Internal)?
                .is_some()
        } else {
            // Agent token: verify agent is in this workspace.
            let agent = state
                .agents
                .find_by_id(&Id::new(&auth.agent_id))
                .await
                .map_err(ApiError::Internal)?;
            agent.map(|a| a.workspace_id == ws_id).unwrap_or(false)
        };

        if !is_member {
            return Err(ApiError::Forbidden(
                "not a member of this workspace".to_string(),
            ));
        }
    }

    let limit = params.limit.unwrap_or(50).min(500);
    let before_id = params.before_id.as_deref().map(Id::new);

    let messages = state
        .messages
        .list_by_workspace(
            &ws_id,
            params.kind.as_deref(),
            params.since,
            params.before_ts,
            before_id.as_ref(),
            Some(limit),
        )
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(messages))
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_common::{
        message::{Destination, MessageKind, MessageOrigin},
        Id,
    };
    use gyre_domain::Agent;
    #[allow(unused_imports)]
    use gyre_ports::MessageRepository as _;
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    fn auth_header() -> (&'static str, &'static str) {
        ("authorization", "Bearer test-token")
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    async fn create_agent_in_workspace(
        state: std::sync::Arc<crate::AppState>,
        agent_id: &str,
        workspace_id: &str,
    ) {
        // The send path resolves the target agent's tenant from its workspace
        // record (scoping rule 1) and REJECTS Agent-directed sends whose
        // scope cannot be resolved — so tests exercising the send path need
        // a real workspace row. Tenant "default" matches the global test
        // token's system principal tenant. Tests that seed a
        // different-tenant workspace first keep it (idempotent).
        let ws_id = Id::new(workspace_id);
        if state.workspaces.find_by_id(&ws_id).await.unwrap().is_none() {
            let ws = gyre_domain::Workspace::new(
                ws_id.clone(),
                Id::new("default"),
                format!("Workspace {workspace_id}"),
                workspace_id,
                0,
            );
            state.workspaces.create(&ws).await.unwrap();
        }
        let mut agent = Agent::new(Id::new(agent_id), agent_id, 0);
        agent.workspace_id = ws_id;
        state.agents.create(&agent).await.unwrap();
    }

    #[tokio::test]
    async fn send_directed_message_succeeds() {
        let state = test_state();
        create_agent_in_workspace(state.clone(), "agent-recv", "ws-1").await;
        let app = crate::api::api_router().with_state(state.clone());

        let body = serde_json::json!({
            "to": {"agent": "agent-recv"},
            "kind": "task_assignment",
            "payload": {"task_id": "TASK-1"}
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-1/messages")
                    .header("content-type", "application/json")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        assert_eq!(json["kind"], "task_assignment");
        assert!(json["signature"].is_string(), "message should be signed");
        assert!(
            json.get("acknowledged").is_none(),
            "acknowledged should be excluded from send response"
        );
    }

    #[tokio::test]
    async fn send_message_rejects_payload_missing_required_field() {
        // message-bus.md §Payload Schemas: `status_update` requires both `status`
        // and `summary`. An incomplete payload must be rejected with 400 whose body
        // names the missing field, and must not be persisted.
        let state = test_state();
        create_agent_in_workspace(state.clone(), "agent-schema", "ws-schema").await;

        let body = serde_json::json!({
            "to": {"agent": "agent-schema"},
            "kind": "status_update",
            "payload": {"status": "in_progress"}
        });
        let resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-schema/messages")
                    .header("content-type", "application/json")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = body_json(resp).await;
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("summary"),
            "400 body must name the missing field, got: {json}"
        );

        // An absent payload for a kind with required fields is likewise a 400.
        let body = serde_json::json!({
            "to": {"agent": "agent-schema"},
            "kind": "status_update",
        });
        let resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-schema/messages")
                    .header("content-type", "application/json")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // A required field present but wrongly typed is the same 400 class:
        // message-bus.md §Payload Schemas declares wire types per field, so
        // `task_id: false` is an invalid payload, not a valid one.
        let body = serde_json::json!({
            "to": {"agent": "agent-schema"},
            "kind": "task_assignment",
            "payload": {"task_id": false}
        });
        let resp = crate::api::api_router()
            .with_state(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-schema/messages")
                    .header("content-type", "application/json")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let json = body_json(resp).await;
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("task_id"),
            "400 body must name the wrongly-typed field, got: {json}"
        );

        // The same request with every required field present is accepted (201).
        let body = serde_json::json!({
            "to": {"agent": "agent-schema"},
            "kind": "status_update",
            "payload": {"status": "in_progress", "summary": "halfway there"}
        });
        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-schema/messages")
                    .header("content-type", "application/json")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED, "body: {json}");
    }

    #[tokio::test]
    async fn send_message_agent_not_found_returns_404() {
        let app = app();
        let body = serde_json::json!({
            "to": {"agent": "ghost-agent"},
            "kind": "task_assignment",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-x/messages")
                    .header("content-type", "application/json")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn server_only_kind_rejected_for_non_server() {
        // agent_created is server_only — a non-server caller should get 403.
        // Global token has Admin role → Server origin → server_only is allowed.
        // See also: server_only_kind_rejected_for_agent_caller for the reject path.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        let body = serde_json::json!({
            "to": {"workspace": "ws-sys"},
            "kind": "agent_created",
            "payload": {"agent_id": "some-agent"}
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-sys/messages")
                    .header("content-type", "application/json")
                    .header(auth_header().0, auth_header().1) // system token = Server origin
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // System token → Server origin → server_only is allowed
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    /// MB-2: non-Admin agent token attempting a server_only kind must be rejected 403.
    /// This is the complementary rejection test for server_only_kind_rejected_for_non_server.
    #[tokio::test]
    async fn server_only_kind_rejected_for_agent_caller() {
        let state = test_state();
        // Register a regular agent token (Agent role, not Admin).
        state
            .kv_store
            .kv_set("agent_tokens", "agent-mb2", "agent-tok-mb2".to_string())
            .await
            .unwrap();
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "to": {"workspace": "ws-mb2"},
            "kind": "agent_created",
            "payload": {"agent_id": "some-agent"}
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-mb2/messages")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer agent-tok-mb2") // Agent role, not Admin
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // Agent origin → server_only check fires → 403
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn poll_messages_returns_stored() {
        let state = test_state();
        create_agent_in_workspace(state.clone(), "agent-poll", "ws-2").await;

        // Directly store a message.
        use gyre_common::message::Message;
        let msg = Message {
            id: Id::new("msg-poll-1"),
            tenant_id: Id::new("default"),
            from: MessageOrigin::Server,
            workspace_id: Some(Id::new("ws-2")),
            to: Destination::Agent(Id::new("agent-poll")),
            kind: MessageKind::TaskAssignment,
            payload: None,
            created_at: 1_000,
            signature: None,
            key_id: None,
            acknowledged: false,
        };
        state.messages.store(&msg).await.unwrap();

        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/agent-poll/messages?after_ts=0")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "msg-poll-1");
    }

    #[tokio::test]
    async fn poll_is_non_destructive() {
        let state = test_state();
        create_agent_in_workspace(state.clone(), "agent-nd", "ws-3").await;

        use gyre_common::message::Message;
        let msg = Message {
            id: Id::new("nd-msg-1"),
            tenant_id: Id::new("default"),
            from: MessageOrigin::Server,
            workspace_id: Some(Id::new("ws-3")),
            to: Destination::Agent(Id::new("agent-nd")),
            kind: MessageKind::TaskAssignment,
            payload: None,
            created_at: 500,
            signature: None,
            key_id: None,
            acknowledged: false,
        };
        state.messages.store(&msg).await.unwrap();

        let app = crate::api::api_router().with_state(state.clone());
        // Poll twice — both should return the message.
        for _ in 0..2 {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/agents/agent-nd/messages?after_ts=0")
                        .header(auth_header().0, auth_header().1)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            let json = body_json(resp).await;
            assert_eq!(json.as_array().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn ack_message_idempotent() {
        let state = test_state();
        create_agent_in_workspace(state.clone(), "agent-ack", "ws-4").await;

        use gyre_common::message::Message;
        let msg = Message {
            id: Id::new("ack-msg-1"),
            tenant_id: Id::new("default"),
            from: MessageOrigin::Server,
            workspace_id: Some(Id::new("ws-4")),
            to: Destination::Agent(Id::new("agent-ack")),
            kind: MessageKind::TaskAssignment,
            payload: None,
            created_at: 1_000,
            signature: None,
            key_id: None,
            acknowledged: false,
        };
        state.messages.store(&msg).await.unwrap();

        let app = crate::api::api_router().with_state(state);

        // Ack twice — both should return 200.
        for _ in 0..2 {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri("/api/v1/agents/agent-ack/messages/ack-msg-1/ack")
                        .header(auth_header().0, auth_header().1)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn workspace_messages_query_returns_events() {
        let state = test_state();

        use gyre_common::message::Message;
        let msg = Message {
            id: Id::new("ws-evt-1"),
            tenant_id: Id::new("default"),
            from: MessageOrigin::Server,
            workspace_id: Some(Id::new("ws-q")),
            to: Destination::Workspace(Id::new("ws-q")),
            kind: MessageKind::TaskCreated,
            payload: Some(serde_json::json!({"task_id": "TASK-99"})),
            created_at: 5_000,
            signature: None,
            key_id: None,
            acknowledged: false,
        };
        state.messages.store(&msg).await.unwrap();

        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/workspaces/ws-q/messages")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "ws-evt-1");
    }

    #[tokio::test]
    async fn cross_workspace_agent_rejected() {
        let state = test_state();
        // agent is in ws-other, not ws-send
        create_agent_in_workspace(state.clone(), "agent-cross", "ws-other").await;
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "to": {"agent": "agent-cross"},
            "kind": "task_assignment",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-send/messages")
                    .header("content-type", "application/json")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // agent is not in ws-send → 400 Bad Request
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn directed_tier_requires_agent_destination() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state);

        let body = serde_json::json!({
            "to": {"workspace": "ws-d"},
            "kind": "task_assignment",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-d/messages")
                    .header("content-type", "application/json")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn queue_depth_enforcement_429() {
        let state = test_state();
        create_agent_in_workspace(state.clone(), "agent-full", "ws-full").await;

        // Manually fill the inbox to the limit.
        use gyre_common::message::Message;
        let limit = state.agent_inbox_max;
        for i in 0..limit {
            let msg = Message {
                id: Id::new(format!("fill-{i}")),
                tenant_id: Id::new("default"),
                from: MessageOrigin::Server,
                workspace_id: Some(Id::new("ws-full")),
                to: Destination::Agent(Id::new("agent-full")),
                kind: MessageKind::TaskAssignment,
                payload: None,
                created_at: i,
                signature: None,
                key_id: None,
                acknowledged: false,
            };
            state.messages.store(&msg).await.unwrap();
        }

        let app = crate::api::api_router().with_state(state);
        let body = serde_json::json!({
            "to": {"agent": "agent-full"},
            "kind": "task_assignment",
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-full/messages")
                    .header("content-type", "application/json")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn telemetry_not_persisted() {
        // Telemetry-tier messages should NOT be stored in MessageRepository.
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        let body = serde_json::json!({
            "to": {"workspace": "ws-tel"},
            "kind": "tool_call_start",
            "payload": {"agent_id": "agent-x", "tool_name": "Read"}
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/ws-tel/messages")
                    .header("content-type", "application/json")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let json = body_json(resp).await;
        let msg_id = json["id"].as_str().unwrap().to_string();

        // Message should NOT be in the persistent store.
        let found = state
            .messages
            .find_by_id(&gyre_common::Id::new(&msg_id))
            .await
            .unwrap();
        assert!(found.is_none(), "Telemetry message must not be persisted");
    }

    /// message-bus.md §Broadcast-only kinds: `DataSeeded` and `QueueUpdated`
    /// are storage-exempt — "never hitting `MessageRepository::store()`".
    /// The send path must short-circuit storage for these kinds BY KIND, not
    /// just by destination: a server-originated broadcast-only kind forced to
    /// a Workspace destination must still not be stored, while the broadcast
    /// channel delivers the message to subscribers. Against the prior
    /// destination-only guard this test fails (the row is persisted).
    #[tokio::test]
    async fn broadcast_only_kinds_not_persisted_even_forced_to_workspace() {
        let state = test_state();
        let mut rx = state.message_broadcast_tx.subscribe();
        let app = crate::api::api_router().with_state(state.clone());

        for kind in ["data_seeded", "queue_updated"] {
            let body = serde_json::json!({
                "to": {"workspace": "ws-bo"},
                "kind": kind,
            });
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/workspaces/ws-bo/messages")
                        .header("content-type", "application/json")
                        .header(auth_header().0, auth_header().1) // global token = Server origin
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::CREATED, "kind={kind}");
            let json = body_json(resp).await;
            let msg_id = json["id"].as_str().unwrap().to_string();

            // Not stored: neither by id nor in the workspace listing.
            assert!(
                state
                    .messages
                    .find_by_id(&Id::new(&msg_id))
                    .await
                    .unwrap()
                    .is_none(),
                "{kind} must never hit MessageRepository::store()"
            );
            let listed = state
                .messages
                .list_by_workspace(&Id::new("ws-bo"), None, None, None, None, Some(100))
                .await
                .unwrap();
            assert!(
                listed.is_empty(),
                "{kind} must never hit MessageRepository::store() (list_by_workspace found rows)"
            );

            // The broadcast channel DID deliver the message to subscribers.
            let msg = rx
                .try_recv()
                .unwrap_or_else(|e| panic!("{kind} must reach broadcast channel subscribers: {e}"));
            assert_eq!(msg.kind.as_str(), kind);
            assert!(matches!(msg.from, MessageOrigin::Server));
        }
    }

    #[tokio::test]
    async fn crash_recovery_acknowledged_false() {
        let state = test_state();
        create_agent_in_workspace(state.clone(), "agent-cr", "ws-cr").await;

        use gyre_common::message::Message;
        let msg = Message {
            id: Id::new("cr-msg-1"),
            tenant_id: Id::new("default"),
            from: MessageOrigin::Server,
            workspace_id: Some(Id::new("ws-cr")),
            to: Destination::Agent(Id::new("agent-cr")),
            kind: MessageKind::TaskAssignment,
            payload: None,
            created_at: 1_000,
            signature: None,
            key_id: None,
            acknowledged: false,
        };
        state.messages.store(&msg).await.unwrap();

        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/agent-cr/messages?acknowledged=false")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    // ── Origin table + scoping rules (message-bus.md §Origin and tenant
    // resolution from auth context, §Scoping Rules 1/3/6) ─────────────────

    /// Seed a user + API key so the caller authenticates through the real
    /// API-key extractor path (mechanism → User origin even for Admin role).
    async fn seed_api_key_user(
        state: &std::sync::Arc<crate::AppState>,
        user_id: &str,
        raw_key: &str,
        roles: Vec<gyre_domain::UserRole>,
        tenant: &str,
    ) {
        let mut user = gyre_domain::User::new(
            Id::new(user_id),
            format!("ext-{user_id}"),
            format!("user-{user_id}"),
            1000,
        );
        user.roles = roles;
        user.tenant_id = Some(Id::new(tenant));
        state.users.create(&user).await.unwrap();
        state
            .api_keys
            .create(
                &crate::auth::hash_api_key(raw_key),
                &user.id,
                "messages-test",
            )
            .await
            .unwrap();
    }

    async fn create_workspace(state: &std::sync::Arc<crate::AppState>, ws_id: &str, tenant: &str) {
        let ws = gyre_domain::Workspace::new(
            Id::new(ws_id),
            Id::new(tenant),
            format!("Workspace {ws_id}"),
            ws_id,
            0,
        );
        state.workspaces.create(&ws).await.unwrap();
    }

    async fn post_message(
        app: Router,
        token: &str,
        workspace: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/workspaces/{workspace}/messages"))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        (status, body_json(resp).await)
    }

    /// Origin table, API-key row: an Admin-role API-key caller resolves to
    /// User(<user_id>) origin — the server_only() 403 gate applies to it.
    /// This is the origin-forging probe: with the old role-based
    /// Admin→Server mapping this request returned 201.
    #[tokio::test]
    async fn admin_api_key_cannot_forge_server_only_kind() {
        let state = test_state();
        seed_api_key_user(
            &state,
            "u-admin-key",
            "gyre-admin-key-1",
            vec![gyre_domain::UserRole::Admin],
            "default",
        )
        .await;
        let app = crate::api::api_router().with_state(state.clone());

        let (status, json) = post_message(
            app,
            "gyre-admin-key-1",
            "ws-ot",
            serde_json::json!({
                "to": {"workspace": "ws-ot"},
                "kind": "agent_created",
                "payload": {"agent_id": "some-agent"}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "body: {json}");
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("only be sent by the server"),
            "403 must come from the server_only origin gate, got: {json}"
        );

        // Nothing was stored.
        let stored = state
            .messages
            .list_by_workspace(&Id::new("ws-ot"), None, None, None, None, None)
            .await
            .unwrap();
        assert!(stored.is_empty(), "rejected forge must not be persisted");
    }

    /// Origin table, Keycloak/OIDC row: an Admin-role OIDC caller resolves
    /// to User(<user_id>) origin — the server_only() 403 gate applies to it.
    #[tokio::test]
    async fn admin_oidc_user_cannot_forge_server_only_kind() {
        let state = crate::auth::test_helpers::make_test_state_with_jwt();
        let token = crate::auth::test_helpers::sign_test_jwt(
            &serde_json::json!({
                "sub": "admin-oidc-1",
                "preferred_username": "admin-oidc-1",
                "realm_access": {"roles": ["Admin"]}
            }),
            600,
        );
        let app = crate::api::api_router().with_state(state.clone());

        let (status, json) = post_message(
            app,
            &token,
            "ws-ot-oidc",
            serde_json::json!({
                "to": {"workspace": "ws-ot-oidc"},
                "kind": "agent_created",
                "payload": {"agent_id": "some-agent"}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "body: {json}");
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("only be sent by the server"),
            "403 must come from the server_only origin gate, got: {json}"
        );
    }

    /// Origin-table consequence (rule 3): Admin-role API-key callers now
    /// resolve to User origin, so the User-origin workspace-membership check
    /// on Directed sends applies to them — a Directed send to an agent in a
    /// workspace the user is NOT a member of is rejected 403, and succeeds
    /// once membership exists.
    #[tokio::test]
    async fn admin_api_key_directed_send_requires_workspace_membership() {
        let state = test_state();
        create_workspace(&state, "ws-r3", "default").await;
        create_agent_in_workspace(state.clone(), "agent-r3", "ws-r3").await;
        seed_api_key_user(
            &state,
            "u-admin-r3",
            "gyre-admin-key-r3",
            vec![gyre_domain::UserRole::Admin],
            "default",
        )
        .await;

        // Not a member → 403 via the rule-3 membership check (origin User).
        let app = crate::api::api_router().with_state(state.clone());
        let (status, json) = post_message(
            app,
            "gyre-admin-key-r3",
            "ws-r3",
            serde_json::json!({
                "to": {"agent": "agent-r3"},
                "kind": "task_assignment",
                "payload": {"task_id": "TASK-R3"}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "body: {json}");
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("not a member"),
            "403 must come from the rule-3 membership check, got: {json}"
        );
        assert!(
            state
                .messages
                .list_unacked(&Id::new("agent-r3"), 10)
                .await
                .unwrap()
                .is_empty(),
            "rejected Directed send must not be persisted"
        );

        // Same caller, same workspace, membership added → 201 (the check is
        // keyed on origin, not a blanket user denial).
        let membership = gyre_domain::WorkspaceMembership::new(
            Id::new("m-r3"),
            Id::new("u-admin-r3"),
            Id::new("ws-r3"),
            gyre_domain::WorkspaceRole::Developer,
            Id::new("u-admin-r3"),
            0,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();
        let app = crate::api::api_router().with_state(state.clone());
        let (status, json) = post_message(
            app,
            "gyre-admin-key-r3",
            "ws-r3",
            serde_json::json!({
                "to": {"agent": "agent-r3"},
                "kind": "task_assignment",
                "payload": {"task_id": "TASK-R3"}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "body: {json}");
        assert_eq!(
            state
                .messages
                .list_unacked(&Id::new("agent-r3"), 10)
                .await
                .unwrap()
                .len(),
            1,
            "accepted Directed send must be persisted"
        );
    }

    /// Origin table, global-token row (rule 6 forge authority preserved):
    /// the global GYRE_AUTH_TOKEN still resolves to Server origin — it can
    /// send server_only kinds AND target Broadcast.
    #[tokio::test]
    async fn global_token_keeps_server_origin_and_broadcast_authority() {
        let state = test_state();
        let app = crate::api::api_router().with_state(state.clone());

        // server_only kind → allowed (Server origin).
        let (status, json) = post_message(
            app.clone(),
            "test-token",
            "ws-gt",
            serde_json::json!({
                "to": {"workspace": "ws-gt"},
                "kind": "agent_created",
                "payload": {"agent_id": "some-agent"}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "body: {json}");

        // Broadcast destination → allowed (Server origin, rule 6).
        let (status, json) = post_message(
            app,
            "test-token",
            "ws-gt",
            serde_json::json!({
                "to": "broadcast",
                "kind": "task_created",
                "payload": {"task_id": "TASK-BC-GT"}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "body: {json}");
        // Broadcast messages are never persisted.
        let msg_id = json["id"].as_str().unwrap();
        assert!(
            state
                .messages
                .find_by_id(&Id::new(msg_id))
                .await
                .unwrap()
                .is_none(),
            "Broadcast messages must never be stored"
        );
    }

    /// Scoping rule 6 preserved for Admin API keys: an Admin-role API-key
    /// caller can still send a Broadcast message (separate role check),
    /// while a non-Admin API-key caller cannot (403).
    #[tokio::test]
    async fn admin_api_key_broadcast_allowed_non_admin_rejected() {
        let state = test_state();
        create_workspace(&state, "ws-b6", "default").await;
        seed_api_key_user(
            &state,
            "u-admin-b6",
            "gyre-admin-key-b6",
            vec![gyre_domain::UserRole::Admin],
            "default",
        )
        .await;
        // Non-Admin caller that IS a workspace member — ABAC allows it, so
        // the 403 below can only come from the Broadcast origin/role guard.
        seed_api_key_user(
            &state,
            "u-dev-b6",
            "gyre-dev-key-b6",
            vec![gyre_domain::UserRole::Developer],
            "default",
        )
        .await;
        let membership = gyre_domain::WorkspaceMembership::new(
            Id::new("m-b6"),
            Id::new("u-dev-b6"),
            Id::new("ws-b6"),
            gyre_domain::WorkspaceRole::Developer,
            Id::new("u-admin-b6"),
            0,
        );
        state
            .workspace_memberships
            .create(&membership)
            .await
            .unwrap();

        // Admin API key → Broadcast allowed (rule 6 role check).
        //
        // The probe kind MUST be a non-server_only Event-tier kind (a
        // Custom kind): the server_only() 403 gate runs BEFORE the Broadcast
        // guard, so a server_only probe (e.g. task_created) would be
        // rejected there for the Admin API-key caller — which resolves to
        // User origin under the auth-mechanism origin table — and the test
        // would exercise the wrong gate. Custom kinds are Event tier with
        // server_only() == false, so the only gate that can reject the
        // non-Admin probe below is the Broadcast origin/role guard.
        let app = crate::api::api_router().with_state(state.clone());
        let (status, json) = post_message(
            app.clone(),
            "gyre-admin-key-b6",
            "ws-b6",
            serde_json::json!({
                "to": "broadcast",
                "kind": "rule6_broadcast_probe",
                "payload": {"probe": "rule6-admin"}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "body: {json}");

        // Non-Admin API key (member) → Broadcast rejected by the guard.
        let (status, json) = post_message(
            app,
            "gyre-dev-key-b6",
            "ws-b6",
            serde_json::json!({
                "to": "broadcast",
                "kind": "rule6_broadcast_probe",
                "payload": {"probe": "rule6-non-admin"}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "body: {json}");
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("Broadcast destination requires Server origin or Admin role"),
            "403 must come from the Broadcast guard, got: {json}"
        );
    }

    /// Scoping rule 1 (tenant isolation): a global-token (Server-origin)
    /// caller whose tenant is "default" sending a Directed message to an
    /// agent in a tenant-2 workspace is rejected 403 and nothing is stored
    /// or broadcast; a same-tenant send still succeeds. This is the
    /// cross-tenant Directed leak probe — with the tenant comparison
    /// removed, the cross-tenant request returns 201.
    #[tokio::test]
    async fn cross_tenant_directed_send_rejected_for_server_origin() {
        let state = test_state();
        // Caller (global token) tenant: "default" (system principal tenant).
        create_workspace(&state, "ws-t1", "default").await;
        create_agent_in_workspace(state.clone(), "agent-t1", "ws-t1").await;
        // Target workspace in ANOTHER tenant.
        create_workspace(&state, "ws-t2", "tenant-2").await;
        create_agent_in_workspace(state.clone(), "agent-t2", "ws-t2").await;

        let mut bus_rx = state.message_broadcast_tx.subscribe();
        let app = crate::api::api_router().with_state(state.clone());

        // Cross-tenant Directed send → 403, nothing stored or broadcast.
        let (status, json) = post_message(
            app.clone(),
            "test-token",
            "ws-t2",
            serde_json::json!({
                "to": {"agent": "agent-t2"},
                "kind": "task_assignment",
                "payload": {"task_id": "TASK-X-TENANT"}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "body: {json}");
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("different tenant"),
            "403 must come from the rule-1 tenant isolation check, got: {json}"
        );
        assert!(
            state
                .messages
                .list_unacked(&Id::new("agent-t2"), 10)
                .await
                .unwrap()
                .is_empty(),
            "cross-tenant Directed send must not be persisted"
        );
        assert!(
            bus_rx.try_recv().is_err(),
            "cross-tenant Directed send must not be broadcast"
        );

        // Same-tenant Directed send → 201 (the check discriminates, it is
        // not a blanket rejection).
        let (status, json) = post_message(
            app,
            "test-token",
            "ws-t1",
            serde_json::json!({
                "to": {"agent": "agent-t1"},
                "kind": "task_assignment",
                "payload": {"task_id": "TASK-SAME-TENANT"}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "body: {json}");
        assert_eq!(
            state
                .messages
                .list_unacked(&Id::new("agent-t1"), 10)
                .await
                .unwrap()
                .len(),
            1,
            "same-tenant Directed send must be persisted"
        );
        let bus_msg = bus_rx
            .try_recv()
            .expect("same-tenant send must be broadcast");
        assert_eq!(
            bus_msg.to,
            gyre_common::message::Destination::Agent(Id::new("agent-t1"))
        );
    }

    /// Scoping rule 1 is fail-closed on an unresolvable target workspace
    /// (review round-4 repair): workspace deletes do not cascade agents, so
    /// a dangling agent whose workspace record is gone is a real
    /// inconsistent-state leak vector. The old code logged a warning and
    /// proceeded with the send; the tenant scope being indeterminable must
    /// REJECT the send (403), not allow it through — with the skip-and-log
    /// behavior restored, this request returns 201 and the message reaches
    /// the dangling agent's inbox.
    #[tokio::test]
    async fn unresolvable_target_workspace_rejects_send() {
        let state = test_state();
        // Dangling agent: its workspace record is deliberately NOT created
        // (the state after DELETE /api/v1/workspaces/W — mem deletes only
        // the workspace row, leaving the agent).
        let mut agent = Agent::new(Id::new("agent-dangling"), "agent-dangling", 0);
        agent.workspace_id = Id::new("ws-gone");
        state.agents.create(&agent).await.unwrap();

        let mut bus_rx = state.message_broadcast_tx.subscribe();
        let app = crate::api::api_router().with_state(state.clone());

        let (status, json) = post_message(
            app,
            "test-token",
            "ws-gone",
            serde_json::json!({
                "to": {"agent": "agent-dangling"},
                "kind": "task_assignment",
                "payload": {"task_id": "TASK-DANGLING"}
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "body: {json}");
        assert!(
            json["error"]
                .as_str()
                .unwrap_or_default()
                .contains("cannot be resolved"),
            "403 must name the unresolvable tenant scope, got: {json}"
        );
        assert!(
            state
                .messages
                .list_unacked(&Id::new("agent-dangling"), 10)
                .await
                .unwrap()
                .is_empty(),
            "a send with an unresolvable target tenant scope must not be persisted"
        );
        assert!(
            bus_rx.try_recv().is_err(),
            "a send with an unresolvable target tenant scope must not be broadcast"
        );
    }

    /// Crash-recovery contract: GET inbox responses still INCLUDE the
    /// `acknowledged` field (it is excluded only from POST responses and
    /// WebSocket pushes).
    #[tokio::test]
    async fn get_inbox_still_includes_acknowledged() {
        let state = test_state();
        create_agent_in_workspace(state.clone(), "agent-keep-ack", "ws-ka").await;
        let msg = gyre_common::message::Message {
            id: Id::new("msg-ka"),
            tenant_id: Id::new("default"),
            from: MessageOrigin::Server,
            workspace_id: Some(Id::new("ws-ka")),
            to: Destination::Agent(Id::new("agent-keep-ack")),
            kind: MessageKind::TaskAssignment,
            payload: None,
            created_at: 1_000,
            signature: None,
            key_id: None,
            acknowledged: false,
        };
        state.messages.store(&msg).await.unwrap();

        let app = crate::api::api_router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/agent-keep-ack/messages?after_ts=0")
                    .header(auth_header().0, auth_header().1)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "msg-ka");
        assert!(
            arr[0].get("acknowledged").is_some_and(|v| v.is_boolean()),
            "GET inbox must include `acknowledged` (crash-recovery contract), got: {arr:?}"
        );
    }
}
