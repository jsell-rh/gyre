use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use gyre_common::WsMessage;
use std::sync::Arc;
use tracing::{info, instrument, warn};

use crate::AppState;

/// GET /ws - WebSocket upgrade endpoint.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state.auth_token.clone()))
}

#[instrument(skip(socket, auth_token))]
async fn handle_socket(socket: WebSocket, auth_token: String) {
    info!("WebSocket connection opened");
    let (mut sender, mut receiver) = socket.split();

    // Expect first message to be Auth.
    let authed = match receiver.next().await {
        Some(Ok(Message::Text(text))) => authenticate(&text, &auth_token, &mut sender).await,
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

    // Main message loop: respond to Ping with Pong.
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                    match ws_msg {
                        WsMessage::Ping { timestamp } => {
                            let pong = WsMessage::Pong { timestamp };
                            let payload = serde_json::to_string(&pong).unwrap();
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
            Ok(Message::Close(_)) => break,
            Err(e) => {
                warn!(%e, "WebSocket error");
                break;
            }
            _ => {}
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

    async fn start_test_server(auth_token: &str) -> String {
        let state = Arc::new(AppState {
            auth_token: auth_token.to_string(),
        });
        let app = build_router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("ws://127.0.0.1:{}/ws", addr.port())
    }

    #[tokio::test]
    async fn ws_valid_auth_succeeds() {
        let url = start_test_server("test-token").await;
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
        let url = start_test_server("real-token").await;
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
        let url = start_test_server("tok").await;
        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();

        // Auth first
        let auth = WsMessage::Auth {
            token: "tok".to_string(),
        };
        ws.send(tungstenite::Message::Text(
            serde_json::to_string(&auth).unwrap(),
        ))
        .await
        .unwrap();
        ws.next().await.unwrap().unwrap(); // consume AuthResult

        // Send Ping
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
}
