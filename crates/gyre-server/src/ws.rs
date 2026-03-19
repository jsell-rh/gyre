use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use gyre_common::{ActivityEventData, WsMessage};
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

    let mut broadcast_rx = state.broadcast_tx.subscribe();

    // Main message loop: handle activity messages and broadcast events.
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
                                WsMessage::ActivityEvent {
                                    event_id,
                                    agent_id,
                                    event_type,
                                    description,
                                    timestamp,
                                } => {
                                    let event = ActivityEventData {
                                        event_id,
                                        agent_id,
                                        event_type,
                                        description,
                                        timestamp,
                                    };
                                    state.activity_store.record(event.clone());
                                    // Broadcast; ignore send errors (no subscribers is fine).
                                    let _ = state.broadcast_tx.send(event);
                                }
                                WsMessage::ActivityQuery { since, limit } => {
                                    let events = state.activity_store.query(since, limit);
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
            broadcast = broadcast_rx.recv() => {
                match broadcast {
                    Ok(event) => {
                        let msg = WsMessage::ActivityEvent {
                            event_id: event.event_id,
                            agent_id: event.agent_id,
                            event_type: event.event_type,
                            description: event.description,
                            timestamp: event.timestamp,
                        };
                        let payload = serde_json::to_string(&msg).unwrap();
                        if sender.send(Message::Text(payload)).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(n, "broadcast receiver lagged");
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
    async fn ws_activity_event_recorded_and_queryable() {
        let (url, state) = start_test_server("tok").await;
        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws, "tok").await;

        // Send ActivityEvent
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

        // Consume the broadcast echo of the ActivityEvent sent back to this connection
        let broadcast_echo = ws.next().await.unwrap().unwrap();
        if let tungstenite::Message::Text(text) = broadcast_echo {
            let result: WsMessage = serde_json::from_str(&text).unwrap();
            assert!(matches!(result, WsMessage::ActivityEvent { .. }));
        } else {
            panic!("expected ActivityEvent broadcast");
        }

        // Verify via store
        let events = state.activity_store.query(None, None);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, "ev1");

        // Query via WS
        let query = WsMessage::ActivityQuery {
            since: None,
            limit: Some(10),
        };
        ws.send(tungstenite::Message::Text(
            serde_json::to_string(&query).unwrap(),
        ))
        .await
        .unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        if let tungstenite::Message::Text(text) = msg {
            let result: WsMessage = serde_json::from_str(&text).unwrap();
            if let WsMessage::ActivityResponse { events } = result {
                assert_eq!(events.len(), 1);
                assert_eq!(events[0].event_id, "ev1");
            } else {
                panic!("expected ActivityResponse, got: {:?}", result);
            }
        } else {
            panic!("expected text message");
        }
    }

    #[tokio::test]
    async fn ws_activity_event_broadcast_to_other_client() {
        let (url, _state) = start_test_server("tok").await;

        // Client A: sender
        let (mut ws_a, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws_a, "tok").await;

        // Client B: receiver
        let (mut ws_b, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        auth_ws(&mut ws_b, "tok").await;

        // A sends an ActivityEvent
        let event = WsMessage::ActivityEvent {
            event_id: "broadcast-ev".to_string(),
            agent_id: "agentA".to_string(),
            event_type: gyre_common::AgEventType::TextMessageContent,
            description: "broadcast test".to_string(),
            timestamp: 2000,
        };
        ws_a.send(tungstenite::Message::Text(
            serde_json::to_string(&event).unwrap(),
        ))
        .await
        .unwrap();

        // B should receive the broadcast (both A and B get it; A sent it)
        let msg = ws_b.next().await.unwrap().unwrap();
        if let tungstenite::Message::Text(text) = msg {
            let result: WsMessage = serde_json::from_str(&text).unwrap();
            if let WsMessage::ActivityEvent { event_id, .. } = result {
                assert_eq!(event_id, "broadcast-ev");
            } else {
                panic!("expected ActivityEvent, got: {:?}", result);
            }
        } else {
            panic!("expected text message");
        }
    }
}
