use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use gyre_common::message::{Destination, MessageKind};
use gyre_common::WsMessage;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tracing::{info, instrument, warn};

use crate::auth::AuthenticatedAgent;
use crate::{AppState, PresenceEntry};

/// Serialize a bus Message for WebSocket delivery.
///
/// message-bus.md §Message Envelope (`acknowledged` field contract): the
/// delivery-state field is excluded from send (POST) responses AND WebSocket
/// pushes via conditional serialization — the field IS included in GET inbox
/// responses (crash recovery). Same manual-removal pattern as `send_response`
/// in api/messages.rs.
fn ws_message_payload(msg: &gyre_common::message::Message) -> String {
    let mut v = serde_json::to_value(msg).unwrap_or_default();
    if let Some(obj) = v.as_object_mut() {
        obj.remove("acknowledged");
    }
    v.to_string()
}

/// GET /ws - WebSocket upgrade endpoint.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

#[instrument(skip(socket, state))]
async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    info!("WebSocket connection opened");
    let (mut sender, mut receiver) = socket.split();

    // Expect first message to be Auth.
    let caller = match receiver.next().await {
        Some(Ok(Message::Text(text))) => {
            match authenticate(&text, &state, &mut sender).await {
                Some(auth) => auth,
                None => return, // auth failed — authenticate() already sent AuthResult
            }
        }
        Some(Ok(Message::Close(_))) | None => {
            info!("connection closed before auth");
            return;
        }
        other => {
            warn!(?other, "unexpected first message, closing");
            return;
        }
    };

    // Assign a unique connection ID for targeted delivery (e.g. PresenceEvicted).
    let connection_id = state.ws_connection_counter.fetch_add(1, Ordering::SeqCst);

    // Per-connection mpsc channel: server → this WS sender (for targeted messages).
    let (targeted_tx, mut targeted_rx) = tokio::sync::mpsc::channel::<String>(32);
    state
        .ws_connections
        .write()
        .await
        .insert(connection_id, targeted_tx);

    // Record the connection's verified user identity for user-targeted
    // pushes (identity-security.md §Impersonation). Connections without a
    // user identity (shared token, agent tokens) are not registered.
    if let Some(uid) = &caller.user_id {
        state
            .ws_connection_users
            .write()
            .await
            .insert(connection_id, uid.as_str().to_string());
    }

    // Subscribed workspace IDs authorized for this connection.
    let mut subscribed_workspaces: Vec<gyre_common::Id> = vec![];

    // session_id for this connection (set via Subscribe message).
    let mut connection_session_id: Option<String> = None;

    let mut bus_rx = state.message_broadcast_tx.subscribe();

    // Main message loop.
    loop {
        tokio::select! {
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                            match ws_msg {
                                WsMessage::Ping { timestamp } => {
                                    let pong = WsMessage::Pong { timestamp };
                                    let payload = serde_json::to_string(&pong).unwrap();
                                    if sender.send(Message::Text(payload)).await.is_err() {
                                        break;
                                    }
                                }
                                WsMessage::Subscribe { scopes, last_seen, session_id } => {
                                    // Record session_id for this connection (presence tracking).
                                    connection_session_id = session_id;

                                    // Validate each requested workspace belongs to caller's tenant.
                                    let mut authorized_workspaces: Vec<gyre_common::Id> = vec![];
                                    let mut rejected_workspaces: Vec<String> = vec![];

                                    for scope in &scopes {
                                        let ws_id = &scope.workspace_id;
                                        match state.workspaces.find_by_id(ws_id).await {
                                            Ok(Some(workspace)) => {
                                                // Admin bypass (global token): allow all workspaces.
                                                let tenant_match = caller.roles.contains(&gyre_domain::UserRole::Admin)
                                                    || workspace.tenant_id.as_str() == caller.tenant_id;
                                                if tenant_match {
                                                    authorized_workspaces.push(ws_id.clone());
                                                } else {
                                                    warn!(
                                                        workspace_id = %ws_id,
                                                        caller_tenant = %caller.tenant_id,
                                                        "Subscribe: workspace belongs to different tenant, rejecting"
                                                    );
                                                    rejected_workspaces.push(ws_id.to_string());
                                                }
                                            }
                                            Ok(None) => {
                                                warn!(workspace_id = %ws_id, "Subscribe: workspace not found");
                                                rejected_workspaces.push(ws_id.to_string());
                                            }
                                            Err(e) => {
                                                warn!(workspace_id = %ws_id, error = %e, "Subscribe: lookup error");
                                                rejected_workspaces.push(ws_id.to_string());
                                            }
                                        }
                                    }

                                    if !rejected_workspaces.is_empty() {
                                        let err_msg = WsMessage::Unknown; // closest available; client will see the raw JSON
                                        // Send structured error as raw JSON since WsMessage has no Error variant.
                                        let err_json = serde_json::json!({
                                            "type": "SubscribeError",
                                            "rejected_workspaces": rejected_workspaces,
                                            "message": "Unauthorized or unknown workspace IDs",
                                        });
                                        let _ = err_msg; // suppress unused warning
                                        let payload = serde_json::to_string(&err_json).unwrap();
                                        if sender.send(Message::Text(payload)).await.is_err() {
                                            break;
                                        }
                                    }

                                    subscribed_workspaces = authorized_workspaces;
                                    // Update per-connection workspace index for targeted broadcasts.
                                    state.ws_connection_workspaces.write().await
                                        .insert(connection_id, subscribed_workspaces.clone());

                                    // Replay Event-tier messages strictly newer than
                                    // last_seen (message-bus.md §Delivery/WebSocket):
                                    // the NEWEST 1000 qualifying messages across ALL
                                    // subscribed workspaces combined, delivered
                                    // oldest-first, plus a single ReplayCatchUp
                                    // signal exactly when more than 1000 qualified.
                                    const REPLAY_LIMIT: usize = 1000;

                                    // Strictly-greater boundary: the spec requires
                                    // created_at > last_seen, but list_by_workspace's
                                    // `since` filter is an inclusive lower bound (the
                                    // documented REST workspace-query semantics, kept
                                    // intact for that endpoint). The replay path
                                    // therefore shifts the bound by 1ms: for integer
                                    // epoch milliseconds, created_at >= last_seen + 1
                                    // <=> created_at > last_seen, so a message created
                                    // exactly at the client's last_seen is NOT
                                    // re-delivered on reconnect. Both storage
                                    // backends see the same shifted bound, so the
                                    // observable replay contract is identical on mem
                                    // and SQLite/Postgres.
                                    let since_ms = match last_seen {
                                        Some(ts) => ts.saturating_add(1),
                                        None => 0,
                                    };

                                    // Fetch the newest REPLAY_LIMIT+1 qualifying
                                    // messages per workspace (newest-first, as the
                                    // adapters return them). min(qualifying, LIMIT+1)
                                    // per workspace is sufficient both to decide
                                    // whether more than LIMIT qualified in total and
                                    // to select the newest LIMIT across all workspaces
                                    // combined: the combined newest-LIMIT window can
                                    // draw at most LIMIT messages from any single
                                    // workspace, and LIMIT+1 >= LIMIT.
                                    let mut pool: Vec<gyre_common::message::Message> = Vec::new();
                                    for ws_id in &subscribed_workspaces {
                                        if let Ok(messages) = state.messages.list_by_workspace(
                                            ws_id,
                                            None,
                                            Some(since_ms),
                                            None,
                                            None,
                                            Some(REPLAY_LIMIT + 1),
                                        )
                                        .await
                                        {
                                            pool.extend(messages);
                                        }
                                    }

                                    // Newest-first across all subscribed workspaces
                                    // combined, ties broken by id — the same
                                    // composite (created_at DESC, id DESC) order the
                                    // adapters use within a workspace.
                                    pool.sort_by(|a, b| {
                                        b.created_at
                                            .cmp(&a.created_at)
                                            .then_with(|| b.id.as_str().cmp(a.id.as_str()))
                                    });
                                    let truncated = pool.len() > REPLAY_LIMIT;
                                    if truncated {
                                        // Drain in place rather than
                                        // Vec::truncate — the byte-slice lint
                                        // textually flags *.truncate(CONST).
                                        pool.drain(REPLAY_LIMIT..);
                                    }
                                    // Deliver the selected window oldest-first.
                                    // `acknowledged` is stripped here too: the
                                    // field's contract excludes it from ALL
                                    // WebSocket pushes (live and replay), while
                                    // GET inbox responses keep it.
                                    pool.reverse();
                                    for m in pool {
                                        let payload = ws_message_payload(&m);
                                        if sender.send(Message::Text(payload)).await.is_err() {
                                            return;
                                        }
                                    }

                                    if truncated {
                                        let catchup = WsMessage::ReplayCatchUp { truncated: true };
                                        let payload = serde_json::to_string(&catchup).unwrap();
                                        if sender.send(Message::Text(payload)).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                                WsMessage::UserPresence {
                                    user_id: _client_user_id, // ignored — server uses verified identity
                                    session_id: presence_session_id,
                                    workspace_id,
                                    view,
                                    timestamp,
                                    editing_entity,
                                } => {
                                    // Only track presence for connections with verified user identity.
                                    // Shared-token (GYRE_AUTH_TOKEN) and agent-token connections have
                                    // user_id: None and are excluded from presence tracking.
                                    if let Some(verified_user_id) = &caller.user_id {
                                        let verified_user_str = verified_user_id.to_string();

                                        // Validate that UserPresence.session_id matches the session_id
                                        // established during Subscribe. Mismatches indicate a protocol
                                        // error or a misbehaving client — reject silently.
                                        let canonical_session_id = match &connection_session_id {
                                            Some(s) if s == &presence_session_id => s.clone(),
                                            Some(_) => {
                                                warn!(
                                                    "UserPresence session_id mismatch with Subscribe session_id — ignoring"
                                                );
                                                continue; // match is inside the select! loop — continue the next iteration
                                            }
                                            None => presence_session_id.clone(),
                                        };

                                        // Server-side timestamp for eviction (immune to client manipulation).
                                        let server_now_ms = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_millis() as u64;

                                        // Graceful disconnect: client sends view="disconnected" on beforeunload.
                                        // Remove the map entry and rebroadcast the departure to other
                                        // workspace subscribers (HSI §7 Presence Awareness).
                                        if view == "disconnected" {
                                            state
                                                .presence
                                                .write()
                                                .await
                                                .remove(&(verified_user_str.clone(), canonical_session_id.clone()));
                                            broadcast_presence_departure(
                                                &state,
                                                &verified_user_str,
                                                &canonical_session_id,
                                                &workspace_id.to_string(),
                                            )
                                            .await;
                                        } else {
                                            // Update presence map.
                                            {
                                                let mut map = state.presence.write().await;
                                                map.insert(
                                                    (verified_user_str.clone(), canonical_session_id.clone()),
                                                    PresenceEntry {
                                                        workspace_id: workspace_id.to_string(),
                                                        view: view.clone(),
                                                        editing_entity: editing_entity.clone(),
                                                        timestamp,
                                                        server_last_seen: server_now_ms,
                                                        connection_id,
                                                    },
                                                );

                                                // Enforce 5-session cap per user: evict oldest if exceeded.
                                                // The departure is rebroadcast to other workspace subscribers.
                                                let user_sessions: Vec<_> = map
                                                    .iter()
                                                    .filter(|((uid, _), _)| uid == &verified_user_str)
                                                    .map(|((_, sid), entry)| {
                                                        (sid.clone(), entry.server_last_seen, entry.connection_id, entry.workspace_id.clone())
                                                    })
                                                    .collect();

                                                if user_sessions.len() > 5 {
                                                    // Find oldest by server_last_seen.
                                                    if let Some((evict_sid, _, evict_conn_id, evict_ws_id)) =
                                                        user_sessions.iter().min_by_key(|(_, ts, _, _)| ts)
                                                    {
                                                        let evict_conn_id = *evict_conn_id;
                                                        let evict_sid = evict_sid.clone();
                                                        let evict_ws_id = evict_ws_id.clone();
                                                        map.remove(&(verified_user_str.clone(), evict_sid.clone()));
                                                        drop(map); // release lock before async work

                                                        // Send PresenceEvicted to the evicted connection.
                                                        let evict_msg = WsMessage::PresenceEvicted {
                                                            session_id: evict_sid.clone(),
                                                        };
                                                        if let Ok(payload) = serde_json::to_string(&evict_msg) {
                                                            let conns = state.ws_connections.read().await;
                                                            if let Some(tx) = conns.get(&evict_conn_id) {
                                                                let _ = tx.try_send(payload);
                                                            }
                                                        }

                                                        // Notify other workspace subscribers of the departure.
                                                        broadcast_presence_departure(
                                                            &state,
                                                            &verified_user_str,
                                                            &evict_sid,
                                                            &evict_ws_id,
                                                        )
                                                        .await;
                                                    } else {
                                                        drop(map);
                                                    }
                                                }
                                            }

                                            // Rebroadcast UserPresence (with verified user_id) to
                                            // connections subscribed to this workspace only.
                                            let broadcast_msg = WsMessage::UserPresence {
                                                user_id: verified_user_id.clone(),
                                                session_id: canonical_session_id,
                                                workspace_id: workspace_id.clone(),
                                                view,
                                                timestamp,
                                                editing_entity,
                                            };
                                            if let Ok(payload) = serde_json::to_string(&broadcast_msg) {
                                                let ws_id = &workspace_id;
                                                let conn_workspaces = state.ws_connection_workspaces.read().await;
                                                let conns = state.ws_connections.read().await;
                                                for (conn_id, workspaces) in conn_workspaces.iter() {
                                                    if workspaces.contains(ws_id) {
                                                        if let Some(tx) = conns.get(conn_id) {
                                                            let _ = tx.try_send(payload.clone());
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                WsMessage::ActivityEvent {
                                    event_id,
                                    agent_id,
                                    event_type,
                                    description,
                                    timestamp,
                                } => {
                                    // Scope telemetry to the caller's first subscribed
                                    // workspace, or the tenant's default (earliest-created)
                                    // workspace when the connection has subscribed to none.
                                    // The fallback must be a real workspace record:
                                    // emit_telemetry resolves the tenant scope from the
                                    // workspace and skips messages addressed to ids that
                                    // do not exist, so a fabricated `default-{tenant}` id
                                    // would silently drop the event.
                                    let ws_id = match subscribed_workspaces.first() {
                                        Some(ws_id) => ws_id.clone(),
                                        None => {
                                            match tenant_default_workspace(&state, &caller.tenant_id).await {
                                                Some(ws_id) => ws_id,
                                                None => continue,
                                            }
                                        }
                                    };
                                    state.emit_telemetry(
                                        ws_id,
                                        MessageKind::StateChanged,
                                        Some(serde_json::json!({
                                            "event_id": event_id,
                                            "agent_id": agent_id,
                                            "event_type": event_type,
                                            "description": description,
                                            "timestamp": timestamp,
                                        })),
                                    ).await;
                                }
                                WsMessage::ActivityQuery { since, limit } => {
                                    // Query TelemetryBuffer scoped to subscribed workspaces.
                                    let since_ms = since.unwrap_or(0);
                                    let lim = limit.unwrap_or(100);
                                    let mut events: Vec<gyre_common::ActivityEventData> = vec![];

                                    let scoped_workspaces: Vec<_> = if subscribed_workspaces.is_empty() {
                                        // No subscription yet: fall back to the tenant's default
                                        // workspace (same real-record resolution as the
                                        // ActivityEvent path above).
                                        tenant_default_workspace(&state, &caller.tenant_id)
                                            .await
                                            .map(|ws_id| vec![ws_id])
                                            .unwrap_or_default()
                                    } else {
                                        subscribed_workspaces.clone()
                                    };

                                    for ws_id in &scoped_workspaces {
                                        let msgs = state.telemetry_buffer.list_since(ws_id, since_ms, lim.saturating_sub(events.len()));
                                        for m in msgs {
                                            if let Some(p) = &m.payload {
                                                if let Ok(ev) = serde_json::from_value::<gyre_common::ActivityEventData>(p.clone()) {
                                                    events.push(ev);
                                                }
                                            }
                                        }
                                        if events.len() >= lim {
                                            break;
                                        }
                                    }

                                    let response = WsMessage::ActivityResponse { events };
                                    let payload = serde_json::to_string(&response).unwrap();
                                    if sender.send(Message::Text(payload)).await.is_err() {
                                        break;
                                    }
                                }
                                other => {
                                    warn!(?other, "unexpected message type after auth");
                                }
                            }
                        } else {
                            warn!(%text, "failed to parse WebSocket message");
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => {
                        warn!(%e, "WebSocket error");
                        break;
                    }
                    _ => {}
                }
            }
            bus_msg = bus_rx.recv() => {
                match bus_msg {
                    Ok(msg) => {
                        // Filter by destination and subscription, plus tenant isolation.
                        let deliver = match &msg.to {
                            Destination::Broadcast => {
                                // Broadcast: only deliver if tenant matches or caller is admin.
                                caller.roles.contains(&gyre_domain::UserRole::Admin)
                                    || msg.workspace_id.as_ref().map(|ws_tid| {
                                        // Use workspace_id to infer tenant — check via subscribed list.
                                        subscribed_workspaces.contains(ws_tid)
                                    }).unwrap_or(true) // no workspace_id = global broadcast
                            }
                            Destination::Workspace(ws_id) => {
                                // Must be subscribed AND workspace must be in caller's tenant.
                                subscribed_workspaces.contains(ws_id)
                            }
                            Destination::Agent(aid) => {
                                // Agent-directed messages reach the WS only on
                                // the agent's OWN live session (agent-gates.md
                                // §Gate Failure Feedback: "For live sessions:
                                // also broadcast via WebSocket for immediate
                                // reaction"). Identity, not subscription,
                                // gates delivery: the caller was verified
                                // against the per-agent token store, so a
                                // match on the verified agent id IS the
                                // destination. User identities (user_id set)
                                // and the global token (Admin role,
                                // agent_id "system") never match — they see
                                // agent inboxes through the REST/MCP read
                                // paths, not the bus.
                                caller.user_id.is_none()
                                    && !caller
                                        .roles
                                        .contains(&gyre_domain::UserRole::Admin)
                                    && caller.agent_id == aid.as_str()
                            }
                        };
                        if deliver {
                            let payload = ws_message_payload(&msg);
                            if sender.send(Message::Text(payload)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(n, "message bus receiver lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            Some(targeted_payload) = targeted_rx.recv() => {
                // Targeted message for this specific connection (e.g. PresenceEvicted).
                if sender.send(Message::Text(targeted_payload)).await.is_err() {
                    break;
                }
            }
        }
    }

    // Cleanup: deregister connection and remove presence entries for this session.
    // The departure is broadcast BEFORE deregistering this connection so other
    // workspace subscribers learn of it (HSI §7 Presence Awareness).
    if let (Some(user_id), Some(session_id)) = (&caller.user_id, &connection_session_id) {
        let removed = state
            .presence
            .write()
            .await
            .remove(&(user_id.to_string(), session_id.clone()));
        if let Some(entry) = removed {
            broadcast_presence_departure(
                &state,
                &user_id.to_string(),
                session_id,
                &entry.workspace_id,
            )
            .await;
        }
    }
    state.ws_connections.write().await.remove(&connection_id);
    state
        .ws_connection_users
        .write()
        .await
        .remove(&connection_id);
    state
        .ws_connection_workspaces
        .write()
        .await
        .remove(&connection_id);

    info!("WebSocket connection closed");
}

/// Resolve the fallback telemetry scope for a subscription-less connection on
/// the legacy Activity path: the caller's tenant's earliest-created workspace.
/// Returns `None` (skip-and-log, never a fabricated id) when the tenant has no
/// workspaces or the lookup fails — `emit_telemetry` derives the tenant scope
/// from the workspace record, so an id that does not resolve would silently
/// drop the message (task-099 F1 regression class).
async fn tenant_default_workspace(state: &AppState, tenant_id: &str) -> Option<gyre_common::Id> {
    let tenant = gyre_common::Id::new(tenant_id);
    match state.workspaces.list_by_tenant(&tenant).await {
        Ok(mut workspaces) if !workspaces.is_empty() => {
            // Deterministic default: earliest created, ties broken by id.
            workspaces.sort_by(|a, b| {
                a.created_at
                    .cmp(&b.created_at)
                    .then_with(|| a.id.as_str().cmp(b.id.as_str()))
            });
            Some(workspaces.swap_remove(0).id)
        }
        Ok(_) => {
            warn!(
                tenant = tenant_id,
                "Activity: caller's tenant has no workspaces; skipping telemetry (no scope to attribute it to)"
            );
            None
        }
        Err(e) => {
            warn!(
                tenant = tenant_id,
                error = %e,
                "Activity: failed to list tenant workspaces; skipping telemetry"
            );
            None
        }
    }
}

/// Broadcast a presence departure to every other subscriber of the workspace.
///
/// HSI §7 Presence Awareness: every presence-removal path (graceful disconnect,
/// socket close, 5-session cap eviction, idle sweeper) must notify other
/// workspace subscribers, not only the "update" branch. We synthesize a
/// `UserPresence { view: "disconnected" }` from the removed entry so clients
/// (ConcurrentEditBanner, PresenceAvatars) drop it from their live views.
pub(crate) async fn broadcast_presence_departure(
    state: &Arc<AppState>,
    user_id: &str,
    session_id: &str,
    workspace_id: &str,
) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let msg = WsMessage::UserPresence {
        user_id: gyre_common::Id::new(user_id),
        session_id: session_id.to_string(),
        workspace_id: gyre_common::Id::new(workspace_id),
        view: "disconnected".to_string(),
        timestamp: now_ms,
        editing_entity: None,
    };
    if let Ok(payload) = serde_json::to_string(&msg) {
        let ws_id = gyre_common::Id::new(workspace_id);
        let conn_workspaces = state.ws_connection_workspaces.read().await;
        let conns = state.ws_connections.read().await;
        for (conn_id, workspaces) in conn_workspaces.iter() {
            if workspaces.contains(&ws_id) {
                if let Some(tx) = conns.get(conn_id) {
                    let _ = tx.try_send(payload.clone());
                }
            }
        }
    }
}

/// Validate the Auth message. Returns `Some(AuthenticatedAgent)` on success.
/// Sends `AuthResult` over the socket in both cases.
#[instrument(skip(token_json, sender, state))]
async fn authenticate(
    token_json: &str,
    state: &Arc<AppState>,
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
) -> Option<AuthenticatedAgent> {
    let token = match serde_json::from_str::<WsMessage>(token_json) {
        Ok(WsMessage::Auth { token }) => token,
        _ => {
            warn!("expected Auth message, got something else");
            return None;
        }
    };

    match crate::auth::authenticate_token(&token, state).await {
        Ok(auth) => {
            // Identity-security.md §Impersonation: a WebSocket connection
            // opened with an impersonation token is an action taken during
            // impersonation — attribute it to the impersonator.
            if let Some(imp) = &auth.impersonation {
                crate::impersonation::spawn_record_impersonated_action(
                    state,
                    imp,
                    "WS",
                    "/ws",
                    true,
                    "websocket",
                );
            }
            let result = WsMessage::AuthResult {
                success: true,
                message: "authenticated".to_string(),
            };
            let payload = serde_json::to_string(&result).unwrap();
            let _ = sender.send(Message::Text(payload)).await;
            Some(auth)
        }
        Err(reason) => {
            warn!(reason, "WebSocket authentication failed");
            let result = WsMessage::AuthResult {
                success: false,
                message: reason.to_string(),
            };
            let payload = serde_json::to_string(&result).unwrap();
            let _ = sender.send(Message::Text(payload)).await;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_router;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite;

    async fn start_test_server(auth_token: &str) -> (String, Arc<AppState>) {
        let mut state = (*crate::mem::test_state()).clone();
        state.auth_token = auth_token.to_string();
        let state = Arc::new(state);
        let app = build_router(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("ws://127.0.0.1:{}/ws", addr.port()), state)
    }

    async fn auth_ws(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        token: &str,
    ) {
        let auth = WsMessage::Auth {
            token: token.to_string(),
        };
        ws.send(tungstenite::Message::Text(
            serde_json::to_string(&auth).unwrap(),
        ))
        .await
        .unwrap();
        ws.next().await.unwrap().unwrap(); // consume AuthResult
    }

    #[tokio::test]
    async fn ws_valid_auth_succeeds() {
        let (url, _state) = start_test_server("test-token").await;
        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();

        let auth = WsMessage::Auth {
            token: "test-token".to_string(),
        };
        ws.send(tungstenite::Message::Text(
            serde_json::to_string(&auth).unwrap(),
        ))
        .await
        .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        if let tungstenite::Message::Text(text) = msg {
            let result: WsMessage = serde_json::from_str(&text).unwrap();
            assert!(matches!(
                result,
                WsMessage::AuthResult { success: true, .. }
            ));
        } else {
            panic!("expected text message");
        }
    }

    #[tokio::test]
    async fn ws_invalid_auth_fails() {
        let (url, _state) = start_test_server("real-token").await;
        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();

        let auth = WsMessage::Auth {
            token: "wrong-token".to_string(),
        };
        ws.send(tungstenite::Message::Text(
            serde_json::to_string(&auth).unwrap(),
        ))
        .await
        .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        if let tungstenite::Message::Text(text) = msg {
            let result: WsMessage = serde_json::from_str(&text).unwrap();
            assert!(matches!(
                result,
                WsMessage::AuthResult { success: false, .. }
            ));
        } else {
            panic!("expected text message");
        }
    }

    // ── Agent-directed WS delivery (agent-gates.md §Gate Failure Feedback:
    // "For live sessions: also broadcast via WebSocket for immediate
    // reaction") ────────────────────────────────────────────────────────

    /// Seed a per-agent token + a workspace so a live agent session can
    /// authenticate over WS and the bus emission resolves its tenant scope.
    async fn seed_agent_ws_identity(state: &Arc<AppState>, agent_id: &str, token: &str) {
        let ws = gyre_domain::Workspace::new(
            gyre_common::Id::new("ws-live-agents"),
            gyre_common::Id::new("default"),
            "live-agents",
            "live-agents",
            now_secs(),
        );
        state.workspaces.create(&ws).await.unwrap();
        state
            .kv_store
            .kv_set("agent_tokens", agent_id, token.to_string())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn ws_agent_directed_message_reaches_own_live_session_only() {
        let (url, state) = start_test_server("tok").await;
        seed_agent_ws_identity(&state, "agent-ws-live", "live-agent-token").await;
        seed_agent_ws_identity(&state, "agent-ws-other", "other-agent-token").await;

        let (mut ws_live, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws_live, "live-agent-token").await;
        let (mut ws_other, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws_other, "other-agent-token").await;

        // The forge emits a GateResult addressed to the live agent's inbox
        // (the exact §Gate Failure Feedback message shape).
        state
            .emit_event(
                gyre_common::Id::new("ws-live-agents"),
                gyre_common::message::Destination::Agent(gyre_common::Id::new("agent-ws-live")),
                gyre_common::message::MessageKind::GateResult,
                Some(serde_json::json!({
                    "mr_id": "mr-42",
                    "gate_name": "security-review",
                    "gate_type": "AgentReview",
                    "status": "Failed",
                    "output": "ChangesRequested: SQL query uses string interpolation",
                    "spec_ref": "specs/system/identity-security@abc123",
                    "attempt": 2,
                    "gate_agent_id": "security-gate-7",
                })),
            )
            .await;

        // The agent's own live session receives it immediately.
        let msg = ws_live.next().await.unwrap().unwrap();
        if let tungstenite::Message::Text(text) = msg {
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(v["kind"], "gate_result");
            assert_eq!(v["to"], serde_json::json!({"agent": "agent-ws-live"}));
            assert_eq!(v["payload"]["mr_id"], "mr-42");
            assert_eq!(v["payload"]["attempt"], 2);
            assert_eq!(v["payload"]["gate_name"], "security-review");
        } else {
            panic!("expected text message");
        }

        // A different agent's live session does not receive it: identity,
        // not subscription, gates agent-directed delivery.
        let got =
            tokio::time::timeout(std::time::Duration::from_millis(300), ws_other.next()).await;
        // Pass on timeout (Err) or clean stream close (Ok(None)); any
        // delivered message means agent-directed delivery leaked.
        let leaked = got.ok().flatten().is_some();
        assert!(
            !leaked,
            "agent-directed message must not reach another agent's session"
        );
    }

    #[tokio::test]
    async fn ws_ping_pong() {
        let (url, _state) = start_test_server("tok").await;
        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws, "tok").await;

        let ping = WsMessage::Ping { timestamp: 42 };
        ws.send(tungstenite::Message::Text(
            serde_json::to_string(&ping).unwrap(),
        ))
        .await
        .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        if let tungstenite::Message::Text(text) = msg {
            let result: WsMessage = serde_json::from_str(&text).unwrap();
            assert!(matches!(result, WsMessage::Pong { timestamp: 42 }));
        } else {
            panic!("expected text message");
        }
    }

    // ── Subscribe replay window (message-bus.md §Delivery/WebSocket) ──────
    // The replay contract: persisted Event-tier messages with
    // created_at > last_seen (strictly greater — no re-delivery on
    // reconnect), the NEWEST 1000 across all subscribed workspaces
    // combined, delivered oldest-first, plus exactly one
    // ReplayCatchUp{truncated:true} when more than 1000 qualified. These
    // tests run against the mem-backed test AppState;
    // tests/message_persistence.rs proves the same contract on the wired
    // SQLite adapter (task-250).

    type WsConn = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    /// An Event-tier workspace message for replay seeding (created_at is
    /// caller-controlled so the window/boundary behavior is deterministic).
    fn replay_event(
        id: &str,
        workspace_id: &str,
        created_at: u64,
    ) -> gyre_common::message::Message {
        gyre_common::message::Message {
            id: gyre_common::Id::new(id),
            tenant_id: gyre_common::Id::new("default"),
            from: gyre_common::message::MessageOrigin::Server,
            workspace_id: Some(gyre_common::Id::new(workspace_id)),
            to: gyre_common::message::Destination::Workspace(gyre_common::Id::new(workspace_id)),
            kind: MessageKind::TaskCreated,
            payload: None,
            created_at,
            signature: None,
            key_id: None,
            acknowledged: false,
        }
    }

    /// A Directed (agent-inbox) message in the same workspace — must never
    /// be replayed (replay is Event-tier only).
    fn replay_directed(
        id: &str,
        workspace_id: &str,
        agent_id: &str,
        created_at: u64,
    ) -> gyre_common::message::Message {
        gyre_common::message::Message {
            to: gyre_common::message::Destination::Agent(gyre_common::Id::new(agent_id)),
            ..replay_event(id, workspace_id, created_at)
        }
    }

    /// Create a workspace in the shared-token caller's tenant so Subscribe's
    /// per-workspace tenant validation accepts it.
    async fn seed_replay_workspace(state: &Arc<AppState>, workspace_id: &str) {
        let ws = gyre_domain::Workspace::new(
            gyre_common::Id::new(workspace_id),
            gyre_common::Id::new("default"),
            workspace_id,
            workspace_id,
            now_secs(),
        );
        state.workspaces.create(&ws).await.unwrap();
    }

    /// Subscribe with an explicit last_seen (the replay boundary under test).
    async fn subscribe_with_last_seen(
        ws: &mut WsConn,
        workspace_ids: &[&str],
        last_seen: Option<u64>,
    ) {
        let sub = WsMessage::Subscribe {
            scopes: workspace_ids
                .iter()
                .map(|w| gyre_common::SubscribeScope {
                    workspace_id: gyre_common::Id::new(*w),
                })
                .collect(),
            last_seen,
            session_id: None,
        };
        ws.send(tungstenite::Message::Text(
            serde_json::to_string(&sub).unwrap(),
        ))
        .await
        .unwrap();
    }

    /// Ping/Pong fence: the socket message loop handles one client message
    /// at a time, and the Subscribe handler emits the whole replay window
    /// (plus the catch-up signal) synchronously before returning to the
    /// loop. A Ping sent after the Subscribe therefore fences the complete
    /// replay response: every frame received before the Pong is the entire
    /// Subscribe output, nothing more follows it.
    async fn fenced_replay_frames(ws: &mut WsConn) -> Vec<serde_json::Value> {
        let ping = WsMessage::Ping { timestamp: 0 };
        ws.send(tungstenite::Message::Text(
            serde_json::to_string(&ping).unwrap(),
        ))
        .await
        .unwrap();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut frames = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            assert!(
                !remaining.is_zero(),
                "timed out waiting for the Pong fence — incomplete replay response"
            );
            let msg = tokio::time::timeout(remaining, ws.next())
                .await
                .expect("timed out waiting for the Pong fence")
                .expect("socket must stay open")
                .expect("frame must be ok");
            let tungstenite::Message::Text(text) = msg else {
                continue;
            };
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            if v.get("type").and_then(|t| t.as_str()) == Some("Pong") {
                return frames;
            }
            frames.push(v);
        }
    }

    /// Split a fenced replay response into (message frames, catch-up
    /// frames). Replayed `Message` envelopes have no `type` key; control
    /// frames (AuthResult/Pong/SubscribeError) and ReplayCatchUp do.
    fn split_replay_frames(
        frames: Vec<serde_json::Value>,
    ) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
        let mut messages = Vec::new();
        let mut catchups = Vec::new();
        for f in frames {
            match f.get("type").and_then(|t| t.as_str()) {
                Some("ReplayCatchUp") => catchups.push(f),
                Some(other) => panic!("unexpected control frame in replay response: {other}"),
                None => messages.push(f),
            }
        }
        (messages, catchups)
    }

    #[tokio::test]
    async fn ws_replay_delivers_newest_1000_with_single_truncation_signal() {
        let (url, state) = start_test_server("tok").await;
        seed_replay_workspace(&state, "ws-replay").await;

        // 1001 Event-tier messages with distinct created_at. The newest
        // (replay-m-1000) is the one the old oldest-1000-of-newest-1001
        // window silently dropped.
        let base_ts = 1_700_000_000u64;
        for i in 0..=1000u64 {
            state
                .messages
                .store(&replay_event(
                    &format!("replay-m-{i:04}"),
                    "ws-replay",
                    base_ts + i,
                ))
                .await
                .unwrap();
        }
        // A Directed (agent-inbox) message, newer than everything else —
        // replay is Event-tier only, so it must never appear.
        state
            .messages
            .store(&replay_directed(
                "replay-directed",
                "ws-replay",
                "replay-agent",
                base_ts + 5_000,
            ))
            .await
            .unwrap();

        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws, "tok").await;
        subscribe_with_last_seen(&mut ws, &["ws-replay"], Some(0)).await;
        let (messages, catchups) = split_replay_frames(fenced_replay_frames(&mut ws).await);

        // Exactly 1000 messages qualify for the window (1001 exist) and
        // exactly one truncation signal, emitted after the window.
        assert_eq!(
            messages.len(),
            1000,
            "replay must deliver exactly 1000 messages"
        );
        assert_eq!(
            catchups.len(),
            1,
            "more than 1000 qualified: exactly one ReplayCatchUp must be sent"
        );
        assert_eq!(
            catchups[0]["truncated"],
            serde_json::json!(true),
            "the catch-up signal must carry truncated:true"
        );

        // The delivered set is the NEWEST 1000: the single newest message
        // IS delivered, the oldest one is the message dropped.
        let ids: Vec<&str> = messages
            .iter()
            .map(|m| m["id"].as_str().expect("message id"))
            .collect();
        assert!(
            ids.contains(&"replay-m-1000"),
            "the NEWEST message must be inside the replay window"
        );
        assert!(
            !ids.contains(&"replay-m-0000"),
            "the oldest message must be the one dropped when truncating"
        );
        assert_eq!(
            ids.iter().filter(|id| **id == "replay-directed").count(),
            0,
            "Directed (agent-inbox) messages must be excluded from replay"
        );
        // The window is exactly the newest 1000 of the 1001 seeded events.
        let expected: Vec<String> = (1..=1000).map(|i| format!("replay-m-{i:04}")).collect();
        assert_eq!(
            ids.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            expected,
            "the window must be the newest 1000 messages, delivered oldest-first"
        );

        // Delivery ordering is oldest-first.
        let created: Vec<u64> = messages
            .iter()
            .map(|m| m["created_at"].as_u64().expect("created_at"))
            .collect();
        assert!(
            created.windows(2).all(|w| w[0] <= w[1]),
            "replay must be delivered oldest-first"
        );
    }

    #[tokio::test]
    async fn ws_replay_no_truncation_signal_at_exactly_1000_messages() {
        let (url, state) = start_test_server("tok").await;
        seed_replay_workspace(&state, "ws-replay").await;

        // Exactly 1000 qualifying messages — the sharp edge of the cap: the
        // per-workspace fetch asks for 1001, gets 1000 back, and no
        // ReplayCatchUp may be sent.
        let base_ts = 1_700_000_000u64;
        for i in 0..1000u64 {
            state
                .messages
                .store(&replay_event(
                    &format!("replay-m-{i:04}"),
                    "ws-replay",
                    base_ts + i,
                ))
                .await
                .unwrap();
        }

        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws, "tok").await;
        subscribe_with_last_seen(&mut ws, &["ws-replay"], Some(0)).await;
        let (messages, catchups) = split_replay_frames(fenced_replay_frames(&mut ws).await);

        assert_eq!(
            messages.len(),
            1000,
            "all 1000 qualifying messages must be replayed"
        );
        assert_eq!(
            catchups.len(),
            0,
            "1000 qualifying messages is not truncation: no ReplayCatchUp"
        );
        assert!(
            messages.iter().any(|m| m["id"] == "replay-m-0999"),
            "the newest message must be delivered"
        );
    }

    #[tokio::test]
    async fn ws_replay_cross_workspace_combined_newest_1000() {
        let (url, state) = start_test_server("tok").await;
        seed_replay_workspace(&state, "ws-replay-a").await;
        seed_replay_workspace(&state, "ws-replay-b").await;

        // 600 messages per workspace with globally distinct timestamps:
        // ws-replay-a owns the even millis 0..=1198, ws-replay-b the odd
        // millis 1..=1199. 1200 qualify in total, so the newest 1000 across
        // BOTH workspaces combined is exactly the set with created_at >= 200
        // (500 from each workspace). A per-workspace cap (the old behavior)
        // would deliver 600 from the first-subscribed workspace instead.
        for i in 0..600u64 {
            state
                .messages
                .store(&replay_event(
                    &format!("replay-a-{i:04}"),
                    "ws-replay-a",
                    i * 2,
                ))
                .await
                .unwrap();
            state
                .messages
                .store(&replay_event(
                    &format!("replay-b-{i:04}"),
                    "ws-replay-b",
                    i * 2 + 1,
                ))
                .await
                .unwrap();
        }

        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws, "tok").await;
        subscribe_with_last_seen(&mut ws, &["ws-replay-a", "ws-replay-b"], Some(0)).await;
        let (messages, catchups) = split_replay_frames(fenced_replay_frames(&mut ws).await);

        assert_eq!(
            messages.len(),
            1000,
            "the 1000-message cap is total across subscribed workspaces"
        );
        assert_eq!(
            catchups.len(),
            1,
            "1200 qualified: the truncation signal must be sent"
        );

        let from_a = messages
            .iter()
            .filter(|m| m["workspace_id"] == "ws-replay-a")
            .count();
        let from_b = messages
            .iter()
            .filter(|m| m["workspace_id"] == "ws-replay-b")
            .count();
        assert_eq!(
            (from_a, from_b),
            (500, 500),
            "the combined newest-1000 window draws 500 from each workspace, \
             not a per-workspace cap"
        );

        // The delivered set is the newest 1000 across both workspaces:
        // everything with created_at >= 200, nothing older.
        for m in &messages {
            let ts = m["created_at"].as_u64().expect("created_at");
            assert!(
                ts >= 200,
                "message older than the combined newest-1000 window must not be replayed"
            );
        }
        let delivered_newest = messages
            .iter()
            .map(|m| m["created_at"].as_u64().expect("created_at"))
            .collect::<std::collections::BTreeSet<_>>();
        let expected: std::collections::BTreeSet<u64> = (200..=1199).collect();
        assert_eq!(
            delivered_newest, expected,
            "the window must be exactly the 1000 newest timestamps across both workspaces"
        );

        // Oldest-first across workspaces combined.
        let created: Vec<u64> = messages
            .iter()
            .map(|m| m["created_at"].as_u64().expect("created_at"))
            .collect();
        assert!(
            created.windows(2).all(|w| w[0] <= w[1]),
            "cross-workspace replay must be delivered oldest-first"
        );
    }

    #[tokio::test]
    async fn ws_replay_strict_last_seen_boundary_on_mem() {
        let (url, state) = start_test_server("tok").await;
        seed_replay_workspace(&state, "ws-replay").await;

        // Three events around the boundary: strictly older, exactly at, and
        // 1ms after the client's last_seen.
        for (id, ts) in [
            ("boundary-old", 5_000u64),
            ("boundary-at", 6_000),
            ("boundary-after", 6_001),
        ] {
            state
                .messages
                .store(&replay_event(id, "ws-replay", ts))
                .await
                .unwrap();
        }

        // last_seen == 6_000: the message created exactly at last_seen must
        // NOT be re-delivered (the old >= filter re-delivered it on every
        // reconnect); only the strictly newer one is replayed.
        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws, "tok").await;
        subscribe_with_last_seen(&mut ws, &["ws-replay"], Some(6_000)).await;
        let (messages, catchups) = split_replay_frames(fenced_replay_frames(&mut ws).await);
        assert_eq!(
            messages.len(),
            1,
            "only strictly-newer messages qualify: exactly the 1ms-later one"
        );
        assert_eq!(messages[0]["id"], "boundary-after");
        assert_eq!(catchups.len(), 0);

        // last_seen == 5_999: both the at-boundary and the later message
        // qualify — the boundary itself is delivered only when last_seen is
        // strictly below it.
        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws, "tok").await;
        subscribe_with_last_seen(&mut ws, &["ws-replay"], Some(5_999)).await;
        let (messages, catchups) = split_replay_frames(fenced_replay_frames(&mut ws).await);
        let ids: Vec<&str> = messages
            .iter()
            .map(|m| m["id"].as_str().expect("message id"))
            .collect();
        assert_eq!(ids, vec!["boundary-at", "boundary-after"]);
        assert_eq!(catchups.len(), 0);
    }

    // ── Presence departure rebroadcast (HSI §7 Presence Awareness) ────────
    // Every removal path must notify other workspace subscribers (task-092 F4):
    // graceful disconnect message, socket close, 5-session cap eviction, and
    // the idle sweeper. Test helper: a second subscribed connection asserts it
    // receives the departure UserPresence.

    /// Create a user + API key in the state so a WS connection authenticating
    /// with `raw_key` has user_id Some(...) — required for presence tracking
    /// (shared-token connections have user_id None and are excluded).
    ///
    /// The user carries a tenant binding on purpose: API-key auth fails
    /// closed for users without one (task-099 F1), and production creates
    /// users exclusively through paths that bind the caller's tenant. The
    /// binding matches the seeded workspace's tenant so Subscribe's
    /// tenant-match validation accepts it.
    async fn seed_api_key_user(state: &Arc<AppState>, user_id: &str, raw_key: &str) {
        use gyre_domain::{User, Workspace};
        let ws = Workspace::new(
            gyre_common::Id::new("ws-presence"),
            gyre_common::Id::new("default"),
            "presence-test",
            "presence-test",
            now_secs(),
        );
        state.workspaces.create(&ws).await.unwrap();
        let user = User {
            tenant_id: Some(gyre_common::Id::new("default")),
            ..User::new(
                gyre_common::Id::new(user_id),
                format!("ext-{user_id}"),
                format!("user-{user_id}"),
                1000,
            )
        };
        state.users.create(&user).await.unwrap();
        state
            .api_keys
            .create(
                &crate::auth::hash_api_key(raw_key),
                &user.id,
                "presence-test-key",
            )
            .await
            .unwrap();
    }

    fn user_presence_msg(session_id: &str, workspace_id: &str, view: &str) -> String {
        serde_json::to_string(&WsMessage::UserPresence {
            user_id: gyre_common::Id::new("ignored"), // server uses verified identity
            session_id: session_id.to_string(),
            workspace_id: gyre_common::Id::new(workspace_id),
            view: view.to_string(),
            timestamp: 0,
            editing_entity: None,
        })
        .unwrap()
    }

    /// Subscribe a connection to the workspace, asserting the subscribe ack.
    async fn subscribe_ws(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        workspace_id: &str,
        session_id: Option<&str>,
    ) {
        let sub = WsMessage::Subscribe {
            scopes: vec![gyre_common::SubscribeScope {
                workspace_id: gyre_common::Id::new(workspace_id),
            }],
            last_seen: None,
            session_id: session_id.map(|s| s.to_string()),
        };
        ws.send(tungstenite::Message::Text(
            serde_json::to_string(&sub).unwrap(),
        ))
        .await
        .unwrap();
    }

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Read messages until a UserPresence with view == expected_view for
    /// session_id arrives (skipping unrelated broadcasts). Times out via the
    /// test deadline.
    async fn expect_presence(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        session_id: &str,
        expected_view: &str,
    ) -> WsMessage {
        loop {
            let msg = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
                .await
                .expect("timed out waiting for presence message")
                .unwrap()
                .unwrap();
            if let tungstenite::Message::Text(text) = msg {
                let decoded: WsMessage = serde_json::from_str(&text).unwrap();
                if let WsMessage::UserPresence {
                    session_id: sid,
                    view,
                    ..
                } = &decoded
                {
                    if sid == session_id && view == expected_view {
                        return decoded;
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn ws_graceful_disconnect_rebroadcasts_departure() {
        let (url, state) = start_test_server("tok").await;
        seed_api_key_user(&state, "u-disc", "key-disc").await;

        // Observer: shared-token connection subscribed to the workspace.
        let (mut observer, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut observer, "tok").await;
        subscribe_ws(&mut observer, "ws-presence", None).await;

        // Subject: API-key user connection with presence.
        let (mut subject, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut subject, "key-disc").await;
        subscribe_ws(&mut subject, "ws-presence", Some("sess-disc")).await;
        subject
            .send(tungstenite::Message::Text(user_presence_msg(
                "sess-disc",
                "ws-presence",
                "specs",
            )))
            .await
            .unwrap();

        // Observer sees the subject's live presence rebroadcast.
        let live = expect_presence(&mut observer, "sess-disc", "specs").await;
        let WsMessage::UserPresence { user_id, .. } = &live else {
            unreachable!("expect_presence returns UserPresence")
        };
        assert_eq!(
            user_id.to_string(),
            "u-disc",
            "rebroadcast must carry the server-verified user id"
        );

        // Subject disconnects gracefully (beforeunload leg).
        subject
            .send(tungstenite::Message::Text(user_presence_msg(
                "sess-disc",
                "ws-presence",
                "disconnected",
            )))
            .await
            .unwrap();

        // Observer must be told about the departure.
        let departure = expect_presence(&mut observer, "sess-disc", "disconnected").await;
        let WsMessage::UserPresence { user_id, .. } = &departure else {
            unreachable!()
        };
        assert_eq!(user_id.to_string(), "u-disc");

        // Entry is gone from the presence map.
        assert!(!state
            .presence
            .read()
            .await
            .contains_key(&("u-disc".to_string(), "sess-disc".to_string())));
    }

    #[tokio::test]
    async fn ws_socket_close_rebroadcasts_departure() {
        let (url, state) = start_test_server("tok").await;
        seed_api_key_user(&state, "u-close", "key-close").await;

        let (mut observer, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut observer, "tok").await;
        subscribe_ws(&mut observer, "ws-presence", None).await;

        let (mut subject, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut subject, "key-close").await;
        subscribe_ws(&mut subject, "ws-presence", Some("sess-close")).await;
        subject
            .send(tungstenite::Message::Text(user_presence_msg(
                "sess-close",
                "ws-presence",
                "specs",
            )))
            .await
            .unwrap();
        expect_presence(&mut observer, "sess-close", "specs").await;

        // Abrupt close (tab killed / network drop) — no disconnect message.
        subject.close(None).await.unwrap();

        let departure = expect_presence(&mut observer, "sess-close", "disconnected").await;
        let WsMessage::UserPresence { user_id, .. } = &departure else {
            unreachable!()
        };
        assert_eq!(user_id.to_string(), "u-close");

        assert!(!state
            .presence
            .read()
            .await
            .contains_key(&("u-close".to_string(), "sess-close".to_string())));
    }

    #[tokio::test]
    async fn ws_session_cap_eviction_rebroadcasts_departure() {
        let (url, state) = start_test_server("tok").await;
        seed_api_key_user(&state, "u-cap", "key-cap").await;

        let (mut observer, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut observer, "tok").await;
        subscribe_ws(&mut observer, "ws-presence", None).await;

        // Open 5 sessions for the user (the cap). Each is a separate WS
        // connection with its own session_id.
        //
        // After sending each presence, wait for its broadcast to reach the
        // observer BEFORE opening the next session. Cross-connection
        // broadcast order is not guaranteed (each connection is an independent
        // server task), so an unordered drain loop would discard a
        // later-expected session's presence while waiting for an earlier one.
        // The round-trip also guarantees strictly increasing server_last_seen
        // (the insert completes before the broadcast), so the eviction below
        // deterministically targets the first-opened session — min_by_key over
        // a HashMap breaks ties and reorders by (random) iteration order
        // otherwise.
        let mut subjects = Vec::new();
        for i in 0..5 {
            let (mut s, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
            auth_ws(&mut s, "key-cap").await;
            let sid = format!("sess-cap-{i}");
            subscribe_ws(&mut s, "ws-presence", Some(&sid)).await;
            s.send(tungstenite::Message::Text(user_presence_msg(
                &sid,
                "ws-presence",
                "specs",
            )))
            .await
            .unwrap();
            subjects.push(s);
            expect_presence(&mut observer, &sid, "specs").await;
        }

        // 6th session: oldest (sess-cap-0) is evicted by the 5-session cap.
        let (mut sixth, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut sixth, "key-cap").await;
        subscribe_ws(&mut sixth, "ws-presence", Some("sess-cap-5")).await;
        sixth
            .send(tungstenite::Message::Text(user_presence_msg(
                "sess-cap-5",
                "ws-presence",
                "specs",
            )))
            .await
            .unwrap();

        // Observer must learn about the evicted session's departure…
        let departure = expect_presence(&mut observer, "sess-cap-0", "disconnected").await;
        let WsMessage::UserPresence { user_id, .. } = &departure else {
            unreachable!()
        };
        assert_eq!(user_id.to_string(), "u-cap");
        // …and the 6th session's arrival.
        expect_presence(&mut observer, "sess-cap-5", "specs").await;

        // The evicted entry is gone from the presence map; the other 5 remain.
        let map = state.presence.read().await;
        assert!(!map.contains_key(&("u-cap".to_string(), "sess-cap-0".to_string())));
        assert!(map.contains_key(&("u-cap".to_string(), "sess-cap-5".to_string())));
        assert_eq!(
            map.values()
                .filter(|e| e.workspace_id == "ws-presence")
                .count(),
            5
        );
    }

    #[tokio::test]
    async fn ws_idle_sweeper_rebroadcasts_departure() {
        let (url, state) = start_test_server("tok").await;
        seed_api_key_user(&state, "u-idle", "key-idle").await;

        let (mut observer, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut observer, "tok").await;
        subscribe_ws(&mut observer, "ws-presence", None).await;

        let (mut subject, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut subject, "key-idle").await;
        subscribe_ws(&mut subject, "ws-presence", Some("sess-idle")).await;
        subject
            .send(tungstenite::Message::Text(user_presence_msg(
                "sess-idle",
                "ws-presence",
                "specs",
            )))
            .await
            .unwrap();
        expect_presence(&mut observer, "sess-idle", "specs").await;

        // Backdate the entry past the 60s idle threshold, then run one sweep.
        {
            let mut map = state.presence.write().await;
            let entry = map
                .get_mut(&("u-idle".to_string(), "sess-idle".to_string()))
                .expect("presence entry should exist");
            entry.server_last_seen -= 61_000;
        }
        crate::evict_stale_presence(&state).await;

        let departure = expect_presence(&mut observer, "sess-idle", "disconnected").await;
        let WsMessage::UserPresence { user_id, .. } = &departure else {
            unreachable!()
        };
        assert_eq!(user_id.to_string(), "u-idle");

        assert!(!state
            .presence
            .read()
            .await
            .contains_key(&("u-idle".to_string(), "sess-idle".to_string())));
    }

    #[tokio::test]
    async fn ws_activity_event_emits_to_telemetry() {
        let (url, state) = start_test_server("tok").await;
        // Seed a workspace in the caller's tenant (the shared dev token
        // authenticates as the "default" system tenant). Telemetry emission
        // resolves the tenant scope from the workspace record and skips
        // messages addressed to ids that do not exist (task-099 F1), so the
        // legacy ActivityEvent path scopes to a real tenant workspace —
        // without one the event would be dropped.
        let ws = gyre_domain::Workspace::new(
            gyre_common::Id::new("ws-activity"),
            gyre_common::Id::new("default"),
            "activity-test",
            "activity-test",
            now_secs(),
        );
        state.workspaces.create(&ws).await.unwrap();

        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws, "tok").await;

        // Send ActivityEvent (legacy path, no subscription yet).
        let event = WsMessage::ActivityEvent {
            event_id: "ev1".to_string(),
            agent_id: "agent1".to_string(),
            event_type: gyre_common::AgEventType::RunStarted,
            description: "test description".to_string(),
            timestamp: 1000,
        };
        ws.send(tungstenite::Message::Text(
            serde_json::to_string(&event).unwrap(),
        ))
        .await
        .unwrap();

        // Give async emit a moment to process.
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // The event must land in the telemetry buffer scoped to the tenant's
        // default (earliest-created, here: only) workspace — not dropped, and
        // not addressed to a fabricated `default-{tenant}` id that no lookup
        // can resolve.
        let scoped =
            state
                .telemetry_buffer
                .list_since(&gyre_common::Id::new("ws-activity"), 0, 100);
        assert_eq!(
            scoped.len(),
            1,
            "telemetry event must land scoped to the tenant-default workspace"
        );
        let m = &scoped[0];
        assert_eq!(m.tenant_id.to_string(), "default");
        assert_eq!(m.kind.as_str(), MessageKind::StateChanged.as_str());
        let payload = m.payload.as_ref().expect("telemetry payload");
        assert_eq!(payload["event_id"], "ev1");
        assert_eq!(payload["agent_id"], "agent1");
        assert_eq!(payload["description"], "test description");
    }

    #[tokio::test]
    async fn ws_activity_event_without_tenant_workspace_skips_emission() {
        // Skip-and-log contract (task-099 F1): when the caller's tenant has
        // no workspace to scope to, the legacy ActivityEvent path drops the
        // event instead of addressing it to a fabricated workspace id.
        let (url, state) = start_test_server("tok").await;
        // No workspace seeded — the tenant-default resolution finds none.

        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws, "tok").await;

        let event = WsMessage::ActivityEvent {
            event_id: "ev-none".to_string(),
            agent_id: "agent1".to_string(),
            event_type: gyre_common::AgEventType::RunStarted,
            description: "no scope".to_string(),
            timestamp: 1000,
        };
        ws.send(tungstenite::Message::Text(
            serde_json::to_string(&event).unwrap(),
        ))
        .await
        .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let all = state.telemetry_buffer.list_all_since(0, 100);
        assert!(
            all.is_empty(),
            "no tenant workspace to scope to: the event must be skipped, not attributed to a fabricated one"
        );
    }

    // ── Socket-free departure-notification tests (F4 core logic) ──────────
    // The four socket tests above exercise the wiring (each removal path →
    // broadcast). These two cover the shared primitives directly —
    // broadcast_presence_departure's payload/fan-out and the idle sweeper's
    // full effect (entry removal + PresenceEvicted to the evicted connection
    // + departure to workspace subscribers) — without loopback TCP, so they
    // run in environments where accept() is unavailable.

    #[tokio::test]
    async fn broadcast_presence_departure_reaches_only_workspace_subscribers() {
        let state = Arc::new((*crate::mem::test_state()).clone());
        // Fake connections: conn 1 subscribes to the workspace, conn 2 does not.
        let (tx1, mut rx1) = tokio::sync::mpsc::channel::<String>(8);
        let (tx2, mut rx2) = tokio::sync::mpsc::channel::<String>(8);
        state.ws_connections.write().await.insert(1, tx1);
        state.ws_connections.write().await.insert(2, tx2);
        state
            .ws_connection_workspaces
            .write()
            .await
            .insert(1, vec![gyre_common::Id::new("ws-dep")]);
        state
            .ws_connection_workspaces
            .write()
            .await
            .insert(2, vec![gyre_common::Id::new("ws-other")]);

        broadcast_presence_departure(&state, "u-dep", "sess-dep", "ws-dep").await;

        let payload = rx1
            .recv()
            .await
            .expect("workspace subscriber must receive the departure");
        match serde_json::from_str::<WsMessage>(&payload).unwrap() {
            WsMessage::UserPresence {
                user_id,
                session_id,
                workspace_id,
                view,
                editing_entity,
                ..
            } => {
                assert_eq!(user_id.to_string(), "u-dep");
                assert_eq!(session_id, "sess-dep");
                assert_eq!(workspace_id.to_string(), "ws-dep");
                assert_eq!(view, "disconnected");
                assert!(
                    editing_entity.is_none(),
                    "a departure presence must not carry an editing entity"
                );
            }
            other => panic!("expected UserPresence departure, got {other:?}"),
        }
        assert!(
            rx2.try_recv().is_err(),
            "a subscriber of a different workspace must not receive the departure"
        );
    }

    #[tokio::test]
    async fn evict_stale_presence_removes_stale_and_notifies_evictee_and_subscribers() {
        let state = Arc::new((*crate::mem::test_state()).clone());
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        {
            let mut map = state.presence.write().await;
            map.insert(
                ("u-old".to_string(), "sess-old".to_string()),
                PresenceEntry {
                    workspace_id: "ws-idle".to_string(),
                    view: "specs".to_string(),
                    editing_entity: Some("spec:specs/a.md".to_string()),
                    timestamp: 0,
                    server_last_seen: now_ms - 61_000,
                    connection_id: 10,
                },
            );
            map.insert(
                ("u-new".to_string(), "sess-new".to_string()),
                PresenceEntry {
                    workspace_id: "ws-idle".to_string(),
                    view: "specs".to_string(),
                    editing_entity: None,
                    timestamp: 0,
                    server_last_seen: now_ms,
                    connection_id: 11,
                },
            );
        }
        // Conn 10 = the stale session's own connection (targeted PresenceEvicted;
        // not workspace-subscribed, so it must NOT also see the broadcast).
        // Conn 20 = observer subscribed to ws-idle.
        let (tx_old, mut rx_old) = tokio::sync::mpsc::channel::<String>(8);
        let (tx_obs, mut rx_obs) = tokio::sync::mpsc::channel::<String>(8);
        state.ws_connections.write().await.insert(10, tx_old);
        state.ws_connections.write().await.insert(20, tx_obs);
        state
            .ws_connection_workspaces
            .write()
            .await
            .insert(20, vec![gyre_common::Id::new("ws-idle")]);

        crate::evict_stale_presence(&state).await;

        {
            let map = state.presence.read().await;
            assert!(
                !map.contains_key(&("u-old".to_string(), "sess-old".to_string())),
                "stale entry must be removed"
            );
            assert!(
                map.contains_key(&("u-new".to_string(), "sess-new".to_string())),
                "fresh entry must survive the sweep"
            );
        }

        let targeted = rx_old
            .recv()
            .await
            .expect("evicted connection must receive PresenceEvicted");
        match serde_json::from_str::<WsMessage>(&targeted).unwrap() {
            WsMessage::PresenceEvicted { session_id } => assert_eq!(session_id, "sess-old"),
            other => panic!("expected PresenceEvicted, got {other:?}"),
        }

        let observed = tokio::time::timeout(std::time::Duration::from_secs(5), rx_obs.recv())
            .await
            .expect("workspace subscriber must receive the departure")
            .expect("channel open");
        match serde_json::from_str::<WsMessage>(&observed).unwrap() {
            WsMessage::UserPresence {
                user_id,
                session_id,
                view,
                ..
            } => {
                assert_eq!(user_id.to_string(), "u-old");
                assert_eq!(session_id, "sess-old");
                assert_eq!(view, "disconnected");
            }
            other => panic!("expected UserPresence departure, got {other:?}"),
        }
    }

    // ── `acknowledged` exclusion from WebSocket pushes (message-bus.md
    // §Message Envelope: the field is excluded from send (POST) responses
    // AND WebSocket pushes via conditional serialization — it IS included
    // in GET inbox responses for crash recovery) ─────────────────────

    /// Live bus-delivery arm: a Message pushed on the bus to a subscribed
    /// agent session must NOT carry the `acknowledged` key in the JSON.
    #[tokio::test]
    async fn ws_live_delivery_strips_acknowledged() {
        let (url, state) = start_test_server("tok").await;
        seed_agent_ws_identity(&state, "agent-live-ack", "live-ack-token").await;

        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws, "live-ack-token").await;

        state
            .emit_event(
                gyre_common::Id::new("ws-live-agents"),
                gyre_common::message::Destination::Agent(gyre_common::Id::new("agent-live-ack")),
                gyre_common::message::MessageKind::TaskAssignment,
                Some(serde_json::json!({"task_id": "TASK-ACK-1"})),
            )
            .await;

        let msg = ws.next().await.unwrap().unwrap();
        let tungstenite::Message::Text(text) = msg else {
            panic!("expected text message");
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["kind"], "task_assignment");
        assert_eq!(v["payload"]["task_id"], "TASK-ACK-1");
        assert!(
            v.get("acknowledged").is_none(),
            "live WS delivery must exclude `acknowledged`, got: {v}"
        );
    }

    /// Subscribe replay arm (review round-3 correction): connect, Subscribe
    /// with last_seen=0 against a workspace with a persisted Event-tier
    /// message, and assert the replayed JSON also lacks the `acknowledged`
    /// key — BOTH WebSocket Message serialization sites strip it, not just
    /// the live arm.
    #[tokio::test]
    async fn ws_subscribe_replay_strips_acknowledged() {
        let (url, state) = start_test_server("tok").await;

        // A persisted Event-tier message in a workspace the (Admin) global
        // token is authorized to subscribe to.
        let ws_id = gyre_common::Id::new("ws-replay-ack");
        let workspace = gyre_domain::Workspace::new(
            ws_id.clone(),
            gyre_common::Id::new("default"),
            "replay-ack",
            "replay-ack",
            now_secs(),
        );
        state.workspaces.create(&workspace).await.unwrap();
        let msg = gyre_common::message::Message {
            id: gyre_common::Id::new("msg-replay-ack"),
            tenant_id: gyre_common::Id::new("default"),
            from: gyre_common::message::MessageOrigin::Server,
            workspace_id: Some(ws_id.clone()),
            to: gyre_common::message::Destination::Workspace(ws_id.clone()),
            kind: gyre_common::message::MessageKind::TaskCreated,
            payload: Some(serde_json::json!({"task_id": "TASK-REPLAY-1"})),
            created_at: 1_000,
            signature: None,
            key_id: None,
            acknowledged: false,
        };
        state.messages.store(&msg).await.unwrap();

        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws, "tok").await;

        // Subscribe with last_seen=0 to force a full replay.
        let sub = WsMessage::Subscribe {
            scopes: vec![gyre_common::SubscribeScope {
                workspace_id: gyre_common::Id::new("ws-replay-ack"),
            }],
            last_seen: Some(0),
            session_id: None,
        };
        ws.send(tungstenite::Message::Text(
            serde_json::to_string(&sub).unwrap(),
        ))
        .await
        .unwrap();

        // Read frames until the replayed message arrives (skipping any
        // unrelated frames).
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            assert!(!remaining.is_zero(), "timed out waiting for replay");
            let frame = tokio::time::timeout(remaining, ws.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let tungstenite::Message::Text(text) = frame else {
                continue;
            };
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            if v["id"] == "msg-replay-ack" {
                assert_eq!(v["kind"], "task_created");
                assert!(
                    v.get("acknowledged").is_none(),
                    "replayed WS message must exclude `acknowledged`, got: {v}"
                );
                break;
            }
        }
    }
}
