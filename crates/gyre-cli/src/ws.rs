use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use gyre_common::WsMessage;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_tungstenite::{connect_async, tungstenite::Message};

pub struct WsClient {
    url: String,
    token: String,
}

impl WsClient {
    pub fn new(url: String, token: String) -> Self {
        Self { url, token }
    }

    pub async fn connect_and_auth(
        &self,
    ) -> Result<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    > {
        let (mut ws, _) = connect_async(&self.url)
            .await
            .with_context(|| format!("Failed to connect to {}", self.url))?;

        let auth = WsMessage::Auth {
            token: self.token.clone(),
        };
        let msg = serde_json::to_string(&auth)?;
        ws.send(Message::Text(msg)).await?;

        // Wait for AuthResult
        while let Some(frame) = ws.next().await {
            let frame = frame?;
            if let Message::Text(text) = frame {
                let msg: WsMessage = serde_json::from_str(&text)?;
                match msg {
                    WsMessage::AuthResult { success, message } => {
                        if success {
                            println!("Auth: OK - {message}");
                        } else {
                            anyhow::bail!("Auth failed: {message}");
                        }
                        break;
                    }
                    _ => continue,
                }
            }
        }

        Ok(ws)
    }

    pub async fn ping(
        &self,
        ws: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> Result<u64> {
        let sent_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let ping_msg = WsMessage::Ping { timestamp: sent_at };
        let text = serde_json::to_string(&ping_msg)?;
        ws.send(Message::Text(text)).await?;

        while let Some(frame) = ws.next().await {
            let frame = frame?;
            if let Message::Text(text) = frame {
                let msg: WsMessage = serde_json::from_str(&text)?;
                if let WsMessage::Pong { timestamp } = msg {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64;
                    let rtt = now.saturating_sub(timestamp);
                    return Ok(rtt);
                }
            }
        }

        anyhow::bail!("Connection closed before pong received")
    }
}
