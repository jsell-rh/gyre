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

                                    // Replay Event-tier messages since last_seen, oldest-first.
                                    let mut replayed = 0usize;
                                    let replay_limit = 1000usize;
                                    let mut truncated = false;

                                    for ws_id in &subscribed_workspaces {
                                        let since_ms = last_seen.unwrap_or(0);
                                        if let Ok(mut messages) = state.messages.list_by_workspace(
                                            ws_id,
                                            None,
                                            Some(since_ms),
                                            None,
                                            None,
                                            Some(replay_limit + 1),
                                        ).await {
                                            // list_by_workspace returns newest-first; reverse to oldest-first.
                                            messages.reverse();
                                            for m in messages {
                                                if replayed >= replay_limit {
                                                    truncated = true;
                                                    break;
                                                }
                                                let payload = serde_json::to_string(&m).unwrap();
                                                if sender.send(Message::Text(payload)).await.is_err() {
                                                    return;
                                                }
                                                replayed += 1;
                                            }
                                        }
                                        if truncated {
                                            break;
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
                                    // Scope telemetry to the caller's first subscribed workspace
                                    // (or a per-tenant default workspace).
                                    let ws_id = subscribed_workspaces
                                        .first()
                                        .cloned()
                                        .unwrap_or_else(|| {
                                            gyre_common::Id::new(format!("default-{}", caller.tenant_id))
                                        });
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
                                    );
                                }
                                WsMessage::ActivityQuery { since, limit } => {
                                    // Query TelemetryBuffer scoped to subscribed workspaces.
                                    let since_ms = since.unwrap_or(0);
                                    let lim = limit.unwrap_or(100);
                                    let mut events: Vec<gyre_common::ActivityEventData> = vec![];

                                    let scoped_workspaces: Vec<_> = if subscribed_workspaces.is_empty() {
                                        // No subscription yet: fall back to tenant-default workspace.
                                        vec![gyre_common::Id::new(format!("default-{}", caller.tenant_id))]
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
                            Destination::Agent(_) => false, // Agent-directed: not delivered via WS
                        };
                        if deliver {
                            let payload = serde_json::to_string(&msg).unwrap();
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
        .ws_connection_workspaces
        .write()
        .await
        .remove(&connection_id);

    info!("WebSocket connection closed");
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
        let ws_id = gyre_common::Id::new("mutation-no-such-workspace");
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

    // ── Presence departure rebroadcast (HSI §7 Presence Awareness) ────────
    // Every removal path must notify other workspace subscribers (task-092 F4):
    // graceful disconnect message, socket close, 5-session cap eviction, and
    // the idle sweeper. Test helper: a second subscribed connection asserts it
    // receives the departure UserPresence.

    /// Create a user + API key in the state so a WS connection authenticating
    /// with `raw_key` has user_id Some(...) — required for presence tracking
    /// (shared-token connections have user_id None and are excluded).
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
        let user = User::new(
            gyre_common::Id::new(user_id),
            &format!("ext-{user_id}"),
            &format!("user-{user_id}"),
            1000,
        );
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
    async fn subscribe_ws(ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >, workspace_id: &str, session_id: Option<&str>) {
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
            let msg = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                ws.next(),
            )
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
                "sess-disc", "ws-presence", "specs",
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
                "sess-disc", "ws-presence", "disconnected",
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
        assert!(
            !state
                .presence
                .read()
                .await
                .contains_key(&("u-disc".to_string(), "sess-disc".to_string()))
        );
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
                "sess-close", "ws-presence", "specs",
            )))
            .await
            .unwrap();
        expect_presence(&mut observer, "sess-close", "specs").await;

        // Abrupt close (tab killed / network drop) — no disconnect message.
        subject.close(None).await.unwrap();

        let departure = expect_presence(&mut observer, "sess-close", "disconnected").await;
        let WsMessage::UserPresence { user_id, .. } = &departure else { unreachable!() };
        assert_eq!(user_id.to_string(), "u-close");

        assert!(
            !state
                .presence
                .read()
                .await
                .contains_key(&("u-close".to_string(), "sess-close".to_string()))
        );
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
        let mut subjects = Vec::new();
        for i in 0..5 {
            let (mut s, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
            auth_ws(&mut s, "key-cap").await;
            let sid = format!("sess-cap-{i}");
            subscribe_ws(&mut s, "ws-presence", Some(&sid)).await;
            s.send(tungstenite::Message::Text(user_presence_msg(
                &sid, "ws-presence", "specs",
            )))
            .await
            .unwrap();
            // Ensure strictly increasing server_last_seen: min_by_key over a
            // HashMap breaks ties by (random) iteration order, which would make
            // the "oldest is evicted" assertion flaky on same-ms inserts.
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            subjects.push(s);
        }
        // Drain the observer until all 5 live presences arrived.
        for i in 0..5 {
            expect_presence(&mut observer, &format!("sess-cap-{i}"), "specs").await;
        }

        // 6th session: oldest (sess-cap-0) is evicted by the 5-session cap.
        let (mut sixth, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut sixth, "key-cap").await;
        subscribe_ws(&mut sixth, "ws-presence", Some("sess-cap-5")).await;
        sixth
            .send(tungstenite::Message::Text(user_presence_msg(
                "sess-cap-5", "ws-presence", "specs",
            )))
            .await
            .unwrap();

        // Observer must learn about the evicted session's departure…
        let departure = expect_presence(&mut observer, "sess-cap-0", "disconnected").await;
        let WsMessage::UserPresence { user_id, .. } = &departure else { unreachable!() };
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
                "sess-idle", "ws-presence", "specs",
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
        let WsMessage::UserPresence { user_id, .. } = &departure else { unreachable!() };
        assert_eq!(user_id.to_string(), "u-idle");

        assert!(
            !state
                .presence
                .read()
                .await
                .contains_key(&("u-idle".to_string(), "sess-idle".to_string()))
        );
    }

    #[tokio::test]
    async fn ws_activity_event_emits_to_telemetry() {
        let (url, state) = start_test_server("tok").await;
        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws, "tok").await;

        // Send ActivityEvent (legacy path).
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

        // Verify it landed in the telemetry buffer.
        let all = state.telemetry_buffer.list_all_since(0, 100);
        assert!(
            !all.is_empty(),
            "telemetry buffer should have at least one entry"
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
}
