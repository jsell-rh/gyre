use futures_util::{SinkExt, StreamExt};
use gyre_common::WsMessage;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, connect_async, tungstenite::Message};

/// Spawn a minimal mock WebSocket server that handles Auth + Ping.
/// Returns the WebSocket URL to connect to.
async fn spawn_mock_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}");

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();

        while let Some(Ok(msg)) = ws.next().await {
            if let Message::Text(text) = msg {
                let parsed: WsMessage = serde_json::from_str(&text).unwrap();
                match parsed {
                    WsMessage::Auth { .. } => {
                        let resp = WsMessage::AuthResult {
                            success: true,
                            message: "OK".to_string(),
                        };
                        let json = serde_json::to_string(&resp).unwrap();
                        ws.send(Message::Text(json)).await.unwrap();
                    }
                    WsMessage::Ping { timestamp } => {
                        let resp = WsMessage::Pong { timestamp };
                        let json = serde_json::to_string(&resp).unwrap();
                        ws.send(Message::Text(json)).await.unwrap();
                    }
                    _ => {}
                }
            }
        }
    });

    url
}

#[tokio::test]
async fn test_auth_and_ping_roundtrip() {
    let url = spawn_mock_server().await;

    let (mut ws, _) = connect_async(&url).await.expect("connect failed");

    // Send Auth
    let auth = WsMessage::Auth {
        token: "test-token".to_string(),
    };
    ws.send(Message::Text(serde_json::to_string(&auth).unwrap()))
        .await
        .unwrap();

    // Expect AuthResult
    let frame = ws.next().await.unwrap().unwrap();
    let text = if let Message::Text(t) = frame {
        t
    } else {
        panic!("expected text frame")
    };
    let msg: WsMessage = serde_json::from_str(&text).unwrap();
    assert!(matches!(msg, WsMessage::AuthResult { success: true, .. }));

    // Send Ping
    let ts = 12345u64;
    let ping = WsMessage::Ping { timestamp: ts };
    ws.send(Message::Text(serde_json::to_string(&ping).unwrap()))
        .await
        .unwrap();

    // Expect Pong
    let frame = ws.next().await.unwrap().unwrap();
    let text = if let Message::Text(t) = frame {
        t
    } else {
        panic!("expected text frame")
    };
    let msg: WsMessage = serde_json::from_str(&text).unwrap();
    assert!(matches!(msg, WsMessage::Pong { timestamp: 12345 }));
}
