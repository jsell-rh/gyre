use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

use gyre_common::message::{Message, MessageOrigin};
use gyre_common::{ActivityEventData, Id};

use crate::auth::AuthenticatedAgent;
use crate::AppState;

#[derive(Deserialize)]
pub struct ActivityQueryParams {
    pub since: Option<u64>,
    pub limit: Option<usize>,
}

/// GET /api/v1/activity — unified activity feed.
///
/// Synthesizes activity events from notifications (which are populated for all
/// key lifecycle events: agent completions, gate failures, spec approvals, MR
/// merges, etc.). Returns a generic JSON array consumed by the dashboard.
///
/// `since` (epoch SECONDS) is the time-period query (activity-dashboard.md
/// §"over any time period"): when present, only durable notifications with
/// `created_at >= since` are returned, newest-first. The filter executes in
/// the repository layer ([`NotificationRepository::list_since`]) — never as
/// post-fetch truncation of a `list_recent` window, which would silently
/// return the globally newest rows outside the requested period.
pub async fn activity_handler(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedAgent,
    Query(params): Query<ActivityQueryParams>,
) -> Json<Vec<serde_json::Value>> {
    let lim = params.limit.unwrap_or(30);

    let notifs = match params.since {
        Some(since) => state.notifications.list_since(since as i64, lim).await,
        None => state.notifications.list_recent(lim).await,
    }
    .unwrap_or_default();

    let events: Vec<serde_json::Value> = notifs
        .into_iter()
        .map(|n| {
            let body: serde_json::Value = n
                .body
                .as_ref()
                .and_then(|b| serde_json::from_str(b).ok())
                .unwrap_or(serde_json::Value::Null);
            let event_type = match n.notification_type.as_str() {
                "AgentCompleted" => "agent_completed",
                "AgentFailed" => "agent_failed",
                "MrMerged" => "merged",
                "MrCreated" => "mr_created",
                "SpecApproved" => "spec_approved",
                "SpecRejected" => "spec_rejected",
                "GateFailure" => "gate_failed",
                "TaskCreated" => "task_created",
                other => other,
            };
            serde_json::json!({
                "event_type": event_type,
                "title": n.title,
                "description": n.body,
                "entity_type": if n.source.entity_type != "system" { Some(n.source.entity_type.as_str()) }
                    else if body.get("mr_id").is_some() { Some("mr") }
                    else if body.get("agent_id").is_some() { Some("agent") }
                    else if body.get("spec_path").is_some() { Some("spec") }
                    else if body.get("task_id").is_some() { Some("task") }
                    else { None },
                "entity_id": body.get("mr_id")
                    .or_else(|| body.get("agent_id"))
                    .or_else(|| body.get("spec_path"))
                    .or_else(|| body.get("task_id"))
                    .and_then(|v| v.as_str())
                    .or(Some(n.source.entity_id.as_str()))
                    .filter(|s| !s.is_empty()),
                "entity_name": body.get("mr_title")
                    .or_else(|| body.get("agent_name"))
                    .and_then(|v| v.as_str()),
                "timestamp": n.created_at,
                "agent_id": body.get("agent_id").and_then(|v| v.as_str()),
                "mr_id": body.get("mr_id").and_then(|v| v.as_str()),
                "task_id": body.get("task_id").and_then(|v| v.as_str()),
                "spec_path": body.get("spec_path").and_then(|v| v.as_str()),
                "repo_id": n.source.repo_id.as_ref().map(|r| r.as_str()),
                // Artifact refs the notification body carries (activity-dashboard.md
                // §"Each activity entry links to the relevant artifact"): surfaced so
                // feed consumers can deep-link gate failures to the MR's gate output
                // and commits/branches to their repo views. No feed entry with a
                // resolvable artifact is a dead end.
                "gate_name": body
                    .get("gate_name")
                    .or_else(|| body.get("gate"))
                    .and_then(|v| v.as_str()),
                "commit_sha": body
                    .get("commit_sha")
                    .or_else(|| body.get("merge_commit_sha"))
                    .or_else(|| body.get("sha"))
                    .and_then(|v| v.as_str()),
                "branch": body
                    .get("branch")
                    .or_else(|| body.get("source_branch"))
                    .and_then(|v| v.as_str()),
                "workspace_id": n.workspace_id.as_str(),
            })
        })
        .collect();

    Json(events)
}

/// GET /api/v1/workspaces/:workspace_id/activity — workspace-scoped telemetry
/// (message-bus.md §API > Telemetry).
///
/// Queries the in-memory `TelemetryBuffer` for the addressed workspace.
/// `since` is epoch MILLISECONDS (matching the message bus, not the legacy
/// seconds convention); entries with `created_at <= since` are excluded.
/// The handler verifies workspace membership before returning results — a
/// non-member caller receives 403 (the same per-handler authorization
/// pattern as the sibling workspace-scoped message routes; the ABAC
/// middleware's builtin `workspace-membership-required` deny is the first
/// line, this check is the handler-side enforcement).
///
/// Response: a bare JSON array of `ActivityEventData` (backwards-compatible
/// field mapping per the spec table: event_id ← message.id, agent_id ←
/// payload["agent_id"] or the Agent origin, event_type ← kind mapped to its
/// AgEventType string, description ← payload["tool_name"] or the kind name,
/// timestamp ← message.created_at).
pub async fn workspace_activity_handler(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Path(workspace_id): Path<String>,
    Query(params): Query<ActivityQueryParams>,
) -> Result<Json<Vec<ActivityEventData>>, crate::api::error::ApiError> {
    let ws_id = Id::new(&workspace_id);

    // Verify workspace membership for non-Admin callers (role-based, not
    // magic-string) — mirrors list_workspace_messages.
    if !auth.roles.contains(&gyre_domain::UserRole::Admin) {
        let is_member = if let Some(ref user_id) = auth.user_id {
            state
                .workspace_memberships
                .find_by_user_and_workspace(user_id, &ws_id)
                .await
                .map_err(crate::api::error::ApiError::Internal)?
                .is_some()
        } else {
            // Agent token: verify agent is in this workspace.
            let agent = state
                .agents
                .find_by_id(&Id::new(&auth.agent_id))
                .await
                .map_err(crate::api::error::ApiError::Internal)?;
            agent.map(|a| a.workspace_id == ws_id).unwrap_or(false)
        };

        if !is_member {
            return Err(crate::api::error::ApiError::Forbidden(
                "not a member of this workspace".to_string(),
            ));
        }
    }

    let since_ms = params.since.unwrap_or(0);
    let limit = params.limit.unwrap_or(100);
    let messages = state.telemetry_buffer.list_since(&ws_id, since_ms, limit);

    let events = messages.iter().map(telemetry_to_activity_event).collect();

    Ok(Json(events))
}

/// Map a telemetry-tier `Message` to `ActivityEventData` per the
/// message-bus.md §API > Telemetry field table.
fn telemetry_to_activity_event(m: &Message) -> ActivityEventData {
    let agent_id = m
        .payload
        .as_ref()
        .and_then(|p| p.get("agent_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| match &m.from {
            MessageOrigin::Agent(id) => Some(id.to_string()),
            _ => None,
        })
        .unwrap_or_default();
    let description = m
        .payload
        .as_ref()
        .and_then(|p| p.get("tool_name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| m.kind.as_str().to_string());
    ActivityEventData {
        event_id: m.id.to_string(),
        agent_id,
        event_type: m.kind.ag_event_type(),
        description,
        timestamp: m.created_at,
    }
}

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_common::message::{Destination, Message, MessageKind, MessageOrigin};
    use gyre_common::Id;
    use gyre_domain::{
        Agent, Notification, NotificationType, User, Workspace, WorkspaceMembership, WorkspaceRole,
    };
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app(state: std::sync::Arc<crate::AppState>) -> Router {
        crate::api::api_router().with_state(state)
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Seed a workspace in tenant "default" plus a user with an API key,
    /// optionally granting membership in the workspace. Returns the raw key.
    async fn seed_user(
        state: &std::sync::Arc<crate::AppState>,
        workspace_id: &str,
        user_id: &str,
        member: bool,
    ) -> String {
        let ws = Workspace::new(
            Id::new(workspace_id),
            Id::new("default"),
            workspace_id,
            workspace_id,
            0,
        );
        state.workspaces.create(&ws).await.unwrap();

        let user = User {
            tenant_id: Some(Id::new("default")),
            ..User::new(Id::new(user_id), format!("ext-{user_id}"), user_id, 0)
        };
        state.users.create(&user).await.unwrap();
        if member {
            let membership = WorkspaceMembership::new(
                Id::new(format!("mem-{user_id}-{workspace_id}")),
                user.id.clone(),
                ws.id.clone(),
                WorkspaceRole::Viewer,
                Id::new("test-inviter"),
                0,
            );
            state
                .workspace_memberships
                .create(&membership)
                .await
                .unwrap();
        }
        let raw_key = format!("key-{user_id}");
        state
            .api_keys
            .create(&crate::auth::hash_api_key(&raw_key), &user.id, "test-key")
            .await
            .unwrap();
        raw_key
    }

    fn telemetry_message(
        id: &str,
        workspace_id: &str,
        from: MessageOrigin,
        kind: MessageKind,
        payload: Option<serde_json::Value>,
        created_at: u64,
    ) -> Message {
        Message {
            id: Id::new(id),
            tenant_id: Id::new("default"),
            from,
            workspace_id: Some(Id::new(workspace_id)),
            to: Destination::Workspace(Id::new(workspace_id)),
            kind,
            payload,
            created_at,
            signature: None,
            key_id: None,
            acknowledged: false,
        }
    }

    async fn get_activity(app: &Router, uri: &str, token: &str) -> (StatusCode, serde_json::Value) {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        (status, body_json(resp).await)
    }

    #[tokio::test]
    async fn workspace_activity_maps_fields_per_spec() {
        // Happy path (task AC): a member of W GETs the activity endpoint and
        // receives ActivityEventData with the exact spec field mapping —
        // event_id ← message.id, agent_id ← payload["agent_id"] with the
        // Agent-origin fallback, event_type ← kind's AgEventType string,
        // description ← payload["tool_name"] with the kind-name fallback,
        // timestamp ← created_at.
        let state = test_state();
        let member_key = seed_user(&state, "ws-act", "u-act-member", true).await;

        // agent_id from payload takes precedence over the Agent origin.
        state.telemetry_buffer.push(telemetry_message(
            "tel-1",
            "ws-act",
            MessageOrigin::Agent(Id::new("agent-origin")),
            MessageKind::ToolCallStart,
            Some(serde_json::json!({
                "agent_id": "agent-payload",
                "tool_name": "Read",
            })),
            5_000,
        ));
        // No payload agent_id → falls back to the Agent origin.
        state.telemetry_buffer.push(telemetry_message(
            "tel-2",
            "ws-act",
            MessageOrigin::Agent(Id::new("agent-origin")),
            MessageKind::ToolCallEnd,
            Some(serde_json::json!({ "tool_name": "grep" })),
            5_001,
        ));
        // Server origin, no payload → empty agent_id, kind-name description.
        state.telemetry_buffer.push(telemetry_message(
            "tel-3",
            "ws-act",
            MessageOrigin::Server,
            MessageKind::StateChanged,
            None,
            5_002,
        ));

        let (status, json) = get_activity(
            &app(state),
            "/api/v1/workspaces/ws-act/activity?since=0",
            &member_key,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {json}");
        let arr = json.as_array().expect("bare JSON array");
        assert_eq!(arr.len(), 3, "body: {json}");

        assert_eq!(arr[0]["event_id"], "tel-1");
        assert_eq!(arr[0]["agent_id"], "agent-payload");
        assert_eq!(arr[0]["event_type"], "TOOL_CALL_START");
        assert_eq!(arr[0]["description"], "Read");
        assert_eq!(arr[0]["timestamp"], 5_000);

        assert_eq!(arr[1]["event_id"], "tel-2");
        assert_eq!(arr[1]["agent_id"], "agent-origin");
        assert_eq!(arr[1]["event_type"], "TOOL_CALL_END");
        assert_eq!(arr[1]["description"], "grep");
        assert_eq!(arr[1]["timestamp"], 5_001);

        assert_eq!(arr[2]["event_id"], "tel-3");
        assert_eq!(arr[2]["agent_id"], "");
        assert_eq!(arr[2]["event_type"], "STATE_CHANGED");
        assert_eq!(arr[2]["description"], "state_changed");
        assert_eq!(arr[2]["timestamp"], 5_002);
    }

    #[tokio::test]
    async fn workspace_activity_since_boundary_and_limit() {
        // Boundary (task AC): entries with created_at <= since are excluded,
        // entries 1ms newer are included, and limit is honored.
        let state = test_state();
        let member_key = seed_user(&state, "ws-bound", "u-bound-member", true).await;

        for (i, created_at) in [1_000u64, 1_001, 1_002, 1_003].into_iter().enumerate() {
            state.telemetry_buffer.push(telemetry_message(
                &format!("tel-bound-{i}"),
                "ws-bound",
                MessageOrigin::Agent(Id::new("agent-bound")),
                MessageKind::RunStarted,
                Some(serde_json::json!({ "agent_id": "agent-bound" })),
                created_at,
            ));
        }

        let app = app(state);

        // since=1001 excludes created_at 1000 AND 1001 (<= since), keeps the
        // 1ms-newer 1002 and 1003.
        let (status, json) = get_activity(
            &app,
            "/api/v1/workspaces/ws-bound/activity?since=1001",
            &member_key,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let arr = json.as_array().unwrap();
        assert_eq!(
            arr.iter()
                .map(|e| e["timestamp"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![1_002, 1_003],
            "entries with created_at <= since must be excluded; 1ms newer included"
        );

        // limit is honored (oldest-first among the matching entries).
        let (status, json) = get_activity(
            &app,
            "/api/v1/workspaces/ws-bound/activity?since=1001&limit=1",
            &member_key,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["timestamp"], 1_002);
        assert_eq!(arr[0]["event_id"], "tel-bound-2");
    }

    #[tokio::test]
    async fn workspace_activity_rejects_same_tenant_non_member() {
        // Enforcement (task AC): a same-tenant non-member caller receives
        // 403 and no telemetry rows. The member user in the same tenant
        // gets 200, proving the 403 is the membership check, not the tenant.
        let state = test_state();
        let member_key = seed_user(&state, "ws-enf", "u-enf-member", true).await;
        let outsider_key = seed_user(&state, "ws-enf", "u-enf-outsider", false).await;

        state.telemetry_buffer.push(telemetry_message(
            "tel-enf",
            "ws-enf",
            MessageOrigin::Agent(Id::new("agent-enf")),
            MessageKind::ToolCallStart,
            Some(serde_json::json!({ "agent_id": "agent-enf", "tool_name": "Read" })),
            9_000,
        ));

        let app = app(state);

        let (status, json) = get_activity(
            &app,
            "/api/v1/workspaces/ws-enf/activity?since=0",
            &outsider_key,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "body: {json}");
        assert!(
            json.get("events").is_none(),
            "non-member must receive no telemetry rows"
        );

        let (status, json) = get_activity(
            &app,
            "/api/v1/workspaces/ws-enf/activity?since=0",
            &member_key,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {json}");
        assert_eq!(json.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn workspace_activity_agent_token_scoped_to_own_workspace() {
        // Agent-token path of the handler check: the builtin ABAC
        // workspace-membership deny keys on subject.type == "user", so an
        // agent token reaches the handler — the handler's own
        // agent-record check must be what rejects a foreign workspace.
        let state = test_state();
        // Workspace under test + the agent's own (different) workspace.
        for ws_id in ["ws-agent-foreign", "ws-agent-home"] {
            let ws = Workspace::new(Id::new(ws_id), Id::new("default"), ws_id, ws_id, 0);
            state.workspaces.create(&ws).await.unwrap();
        }
        let mut agent = Agent::new(Id::new("agent-home-1"), "agent-home-1", 0);
        agent.workspace_id = Id::new("ws-agent-home");
        state.agents.create(&agent).await.unwrap();
        state
            .kv_store
            .kv_set(
                "agent_tokens",
                "agent-home-1",
                "agent-home-token".to_string(),
            )
            .await
            .unwrap();

        state.telemetry_buffer.push(telemetry_message(
            "tel-agent",
            "ws-agent-foreign",
            MessageOrigin::Agent(Id::new("agent-foreign")),
            MessageKind::RunStarted,
            Some(serde_json::json!({ "agent_id": "agent-foreign" })),
            7_000,
        ));

        let app = app(state);

        // Foreign workspace: 403, no rows.
        let (status, json) = get_activity(
            &app,
            "/api/v1/workspaces/ws-agent-foreign/activity?since=0",
            "agent-home-token",
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "body: {json}");

        // Own workspace: 200 (empty buffer is fine — the agent is a member
        // by agent record).
        let (status, json) = get_activity(
            &app,
            "/api/v1/workspaces/ws-agent-home/activity?since=0",
            "agent-home-token",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body: {json}");
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn unified_activity_honors_since_against_durable_notifications() {
        // Time-period query (task AC, defect (b)): seed one durable
        // notification older than T and two newer, then GET
        // /api/v1/activity?since=T — the stale entry must be absent, the
        // newer ones present, newest-first. The filter must run in the
        // repository (list_since), not as post-fetch truncation of a
        // recency window.
        let state = test_state();
        let make = |id: &str, created_at: i64| {
            Notification::new(
                Id::new(id),
                Id::new("ws-since"),
                Id::new("user-1"),
                NotificationType::MrMerged,
                format!("n {id}"),
                "default",
                created_at,
            )
        };
        // Deliberately more rows than the request limit so a
        // post-fetch-truncation implementation would ALSO fail by paging
        // the globally newest rows into the window.
        state
            .notifications
            .create(&make("old-1", 900))
            .await
            .unwrap();
        state
            .notifications
            .create(&make("old-2", 950))
            .await
            .unwrap();
        state
            .notifications
            .create(&make("new-1", 1_100))
            .await
            .unwrap();
        state
            .notifications
            .create(&make("new-2", 1_200))
            .await
            .unwrap();

        let app = app(state);

        // since=T excludes created_at < T, newest first.
        let (status, json) = get_activity(&app, "/api/v1/activity?since=1000", "test-token").await;
        assert_eq!(status, StatusCode::OK, "body: {json}");
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 2, "stale entries must be absent: {json}");
        assert_eq!(arr[0]["timestamp"], 1_200);
        assert_eq!(arr[1]["timestamp"], 1_100);

        // Without since: the plain recency window still works.
        let (status, json) = get_activity(&app, "/api/v1/activity?limit=3", "test-token").await;
        assert_eq!(status, StatusCode::OK, "body: {json}");
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["timestamp"], 1_200);

        // since older than everything: all rows return, newest first.
        let (status, json) = get_activity(&app, "/api/v1/activity?since=0", "test-token").await;
        assert_eq!(status, StatusCode::OK, "body: {json}");
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 4);
        assert_eq!(arr[0]["timestamp"], 1_200);
        assert_eq!(arr[3]["timestamp"], 900);
    }
}
