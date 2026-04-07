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
/// Uses role-based check (Admin → Server) rather than magic-string comparison
/// on `agent_id`. The string "system" is NOT a reliable signal: API key and
/// OIDC paths set agent_id from user.display_name / preferred_username, which
/// an attacker could set to "system" without holding Admin role.
fn origin_from_auth(auth: &AuthenticatedAgent) -> MessageOrigin {
    if auth.roles.contains(&gyre_domain::UserRole::Admin) {
        // Admin callers (global token, Admin-role users) → Server origin.
        MessageOrigin::Server
    } else if let Some(user_id) = &auth.user_id {
        // API key or Keycloak JWT with non-Admin role → User
        MessageOrigin::User(user_id.clone())
    } else {
        // Per-agent token → Agent
        MessageOrigin::Agent(Id::new(&auth.agent_id))
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

        // Target must be in this workspace (enforces tenant isolation since workspace IDs are
        // globally unique and each workspace belongs to exactly one tenant).
        if target_agent.workspace_id != ws_id {
            return Err(ApiError::BadRequest(format!(
                "target agent {} is not in workspace {}",
                target_agent_id, workspace_id
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

    // Persist Directed and Event tier only (Telemetry is in-memory, Broadcast is not stored).
    if effective_tier != MessageTier::Telemetry && !matches!(msg.to, Destination::Broadcast) {
        state
            .messages
            .store(&msg)
            .await
            .map_err(ApiError::Internal)?;
    }

    // Dispatch to consumers (best-effort, non-blocking).
    let _ = state.message_dispatch_tx.try_send(msg.clone());

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
        let mut agent = Agent::new(Id::new(agent_id), agent_id, 0);
        agent.workspace_id = Id::new(workspace_id);
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
}
