//! GET /ws/agents/:id/tty — WebSocket terminal viewer for agent log output.
//!
//! Auth: first message must be `{"type":"Auth","token":"<bearer>"}`.
//!
//! After auth the server:
//!   1. Sends all buffered log lines for the agent.
//!   2. Streams new log lines in real time as they arrive via POST /agents/:id/logs.
//!
//! If the agent has a registered OS process, a header line notes it. The process
//! output is captured via the log buffer (agents write their output through
//! POST /api/v1/agents/:id/logs), so this TTY view works for any agent regardless
//! of how the process was launched.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use gyre_common::WsMessage;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::AppState;

/// GET /ws/agents/:id/tty — upgrade to a WebSocket TTY session for an agent.
pub async fn tty_handler(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_tty_socket(socket, id, state))
}

async fn handle_tty_socket(socket: WebSocket, agent_id: String, state: Arc<AppState>) {
    info!(agent_id = %agent_id, "TTY WebSocket opened");
    let (mut sender, mut receiver) = socket.split();

    // Expect first message to be Auth (same protocol as /ws).
    let authed = match receiver.next().await {
        Some(Ok(Message::Text(text))) => match serde_json::from_str::<WsMessage>(&text) {
            Ok(WsMessage::Auth { token }) => {
                let ok = validate_token(&token, &state).await;
                let result = if ok {
                    WsMessage::AuthResult {
                        success: true,
                        message: "authenticated".to_string(),
                    }
                } else {
                    warn!(%agent_id, "TTY auth failed: invalid token");
                    WsMessage::AuthResult {
                        success: false,
                        message: "invalid token".to_string(),
                    }
                };
                let payload = serde_json::to_string(&result).unwrap();
                let _ = sender.send(Message::Text(payload)).await;
                ok
            }
            _ => {
                warn!(%agent_id, "TTY: expected Auth message");
                false
            }
        },
        Some(Ok(Message::Close(_))) | None => return,
        other => {
            warn!(?other, %agent_id, "TTY: unexpected first message");
            false
        }
    };

    if !authed {
        return;
    }

    // Check whether the agent has a registered running process.
    let has_process = {
        let registry = state.process_registry.lock().await;
        registry.contains_key(&agent_id)
    };

    let banner = if has_process {
        format!(
            "[tty] agent {} — running process detected, streaming log output\r\n",
            agent_id
        )
    } else {
        format!("[tty] agent {} — log stream\r\n", agent_id)
    };
    if sender.send(Message::Text(banner)).await.is_err() {
        return;
    }

    // Replay the existing log buffer.
    let buffered = {
        let logs = state.agent_logs.lock().await;
        logs.get(&agent_id).cloned().unwrap_or_default()
    };
    for line in &buffered {
        let out = format!("{}\r\n", line);
        if sender.send(Message::Text(out)).await.is_err() {
            return;
        }
    }

    // Subscribe to the live log broadcast channel for this agent.
    let log_rx = {
        let mut txs = state.agent_log_tx.lock().await;
        let tx = txs.entry(agent_id.clone()).or_insert_with(|| {
            let (tx, _) = broadcast::channel(256);
            tx
        });
        tx.subscribe()
    };
    let mut log_rx = log_rx;

    // Stream: forward new log lines, exit on close frame.
    loop {
        tokio::select! {
            client_msg = receiver.next() => {
                match client_msg {
                    // Close frame or disconnected.
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => { warn!(%e, %agent_id, "TTY WS error"); break; }
                    // Ignore any text input from the client for now.
                    _ => {}
                }
            }
            log_msg = log_rx.recv() => {
                match log_msg {
                    Ok(line) => {
                        let out = format!("{}\r\n", line);
                        if sender.send(Message::Text(out)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(n, %agent_id, "TTY log receiver lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    info!(agent_id = %agent_id, "TTY WebSocket closed");
}

/// Returns true if the token is the global auth token or a registered agent token.
async fn validate_token(token: &str, state: &Arc<AppState>) -> bool {
    if token == state.auth_token {
        return true;
    }
    // API-key or JWT auth is not attempted here — WS first-message auth only
    // supports the two token forms used by agents and the dev global token.
    let agent_tokens = state.agent_tokens.lock().await;
    agent_tokens.values().any(|t| t.as_str() == token)
}

#[cfg(test)]
mod tests {
    use crate::build_router;
    use futures_util::{SinkExt, StreamExt};
    use gyre_common::WsMessage;
    use std::sync::Arc;
    use tokio_tungstenite::tungstenite;

    async fn start_test_server() -> (String, Arc<crate::AppState>) {
        let mut state = (*crate::mem::test_state()).clone();
        state.auth_token = "tok".to_string();
        let state = Arc::new(state);
        let app = build_router(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let url = format!("ws://127.0.0.1:{}/ws/agents/agent-1/tty", addr.port());
        (url, state)
    }

    async fn auth_tty(
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        token: &str,
    ) -> bool {
        let auth = WsMessage::Auth {
            token: token.to_string(),
        };
        ws.send(tungstenite::Message::Text(
            serde_json::to_string(&auth).unwrap(),
        ))
        .await
        .unwrap();
        let msg = ws.next().await.unwrap().unwrap();
        if let tungstenite::Message::Text(text) = msg {
            let result: WsMessage = serde_json::from_str(&text).unwrap();
            matches!(result, WsMessage::AuthResult { success: true, .. })
        } else {
            false
        }
    }

    #[tokio::test]
    async fn tty_auth_valid() {
        let (url, _state) = start_test_server().await;
        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        assert!(auth_tty(&mut ws, "tok").await);
    }

    #[tokio::test]
    async fn tty_auth_invalid_rejected() {
        let (url, _state) = start_test_server().await;
        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        assert!(!auth_tty(&mut ws, "wrong").await);
    }

    #[tokio::test]
    async fn tty_streams_buffered_logs_then_live() {
        let (url, state) = start_test_server().await;

        // Pre-populate the log buffer for agent-1.
        {
            let mut logs = state.agent_logs.lock().await;
            logs.entry("agent-1".to_string())
                .or_default()
                .push("[1000] hello from buffer".to_string());
        }

        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        assert!(auth_tty(&mut ws, "tok").await);

        // First message is the banner line.
        let banner = ws.next().await.unwrap().unwrap();
        if let tungstenite::Message::Text(t) = banner {
            assert!(t.contains("agent-1"));
        } else {
            panic!("expected banner");
        }

        // Second message is the buffered log line.
        let buffered = ws.next().await.unwrap().unwrap();
        if let tungstenite::Message::Text(t) = buffered {
            assert!(t.contains("hello from buffer"), "got: {t}");
        } else {
            panic!("expected buffered log line");
        }

        // Now broadcast a live log line and check it arrives.
        {
            let mut txs = state.agent_log_tx.lock().await;
            let tx = txs.entry("agent-1".to_string()).or_insert_with(|| {
                let (tx, _) = tokio::sync::broadcast::channel(256);
                tx
            });
            let _ = tx.send("[1001] live line".to_string());
        }

        let live = ws.next().await.unwrap().unwrap();
        if let tungstenite::Message::Text(t) = live {
            assert!(t.contains("live line"), "got: {t}");
        } else {
            panic!("expected live log line");
        }
    }
}
