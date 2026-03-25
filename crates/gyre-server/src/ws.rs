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
use std::sync::Arc;
use tracing::{info, instrument, warn};

use crate::AppState;

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
    let authed = match receiver.next().await {
        Some(Ok(Message::Text(text))) => authenticate(&text, &state.auth_token, &mut sender).await,
        Some(Ok(Message::Close(_))) | None => {
            info!("connection closed before auth");
            return;
        }
        other => {
            warn!(?other, "unexpected first message, closing");
            return;
        }
    };

    if !authed {
        return;
    }

    // Subscribed workspace IDs for this connection. Empty = no workspace filtering yet.
    let mut subscribed_workspaces: Vec<gyre_common::Id> = vec![];

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
                                WsMessage::Subscribe { scopes, last_seen } => {
                                    // Extract workspace IDs from SubscribeScope structs.
                                    subscribed_workspaces.clear();
                                    for scope in &scopes {
                                        subscribed_workspaces.push(scope.workspace_id.clone());
                                    }

                                    // Replay Event-tier messages since last_seen.
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
                                WsMessage::ActivityEvent {
                                    event_id,
                                    agent_id,
                                    event_type,
                                    description,
                                    timestamp,
                                } => {
                                    // Map legacy ActivityEvent to a Telemetry-tier message.
                                    // Use a "default" workspace for legacy events without workspace context.
                                    let ws_id = gyre_common::Id::new("default");
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
                                    // Query from TelemetryBuffer across all workspaces.
                                    let since_ms = since.unwrap_or(0);
                                    let lim = limit.unwrap_or(100);
                                    let msgs = state.telemetry_buffer.list_all_since(since_ms, lim);
                                    // Convert to legacy ActivityEventData format.
                                    let mut events = vec![];
                                    for m in msgs {
                                        if let Some(payload) = &m.payload {
                                            if let Ok(ev) = serde_json::from_value::<gyre_common::ActivityEventData>(payload.clone()) {
                                                events.push(ev);
                                            }
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
                        // Filter by destination and subscription.
                        let deliver = match &msg.to {
                            Destination::Broadcast => true,
                            Destination::Workspace(ws_id) => {
                                subscribed_workspaces.is_empty()
                                    || subscribed_workspaces.contains(ws_id)
                            }
                            Destination::Agent(_) => false,
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
        }
    }

    info!("WebSocket connection closed");
}

/// Send AuthResult and return true if token is valid.
#[instrument(skip(token_json, sender, auth_token))]
async fn authenticate(
    token_json: &str,
    auth_token: &str,
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
) -> bool {
    match serde_json::from_str::<WsMessage>(token_json) {
        Ok(WsMessage::Auth { token }) => {
            let success = token == auth_token;
            let result = if success {
                WsMessage::AuthResult {
                    success: true,
                    message: "authenticated".to_string(),
                }
            } else {
                warn!("authentication failed: invalid token");
                WsMessage::AuthResult {
                    success: false,
                    message: "invalid token".to_string(),
                }
            };
            let payload = serde_json::to_string(&result).unwrap();
            let _ = sender.send(Message::Text(payload)).await;
            success
        }
        _ => {
            warn!("expected Auth message, got something else");
            false
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
        assert!(!all.is_empty(), "telemetry buffer should have at least one entry");
    }
}
