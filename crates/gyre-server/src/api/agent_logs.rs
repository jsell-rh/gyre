use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

use crate::AppState;

use super::error::ApiError;
use super::now_secs;

#[derive(Serialize)]
pub struct LogLine {
    pub timestamp: u64,
    pub message: String,
}

#[derive(Deserialize)]
pub struct AppendLogRequest {
    pub message: String,
}

#[derive(Deserialize)]
pub struct LogsQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

pub async fn append_log(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<AppendLogRequest>,
) -> Result<StatusCode, ApiError> {
    let ts = now_secs();
    let raw = format!("[{}] {}", ts, req.message);
    {
        let mut logs = state.agent_logs.lock().await;
        let buf = logs.entry(id.clone()).or_default();
        buf.push(raw.clone());
        if buf.len() > 10_000 {
            buf.drain(0..buf.len() - 10_000);
        }
    }
    // Broadcast to live SSE subscribers (create channel if needed)
    let mut txs = state.agent_log_tx.lock().await;
    let tx = txs.entry(id).or_insert_with(|| {
        let (tx, _) = broadcast::channel(256);
        tx
    });
    let _ = tx.send(raw);
    Ok(StatusCode::CREATED)
}

pub async fn get_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<LogsQuery>,
) -> Result<Json<Vec<String>>, ApiError> {
    let limit = params.limit.unwrap_or(100).min(1000);
    let offset = params.offset.unwrap_or(0);
    let logs = state.agent_logs.lock().await;
    let lines: Vec<String> = logs
        .get(&id)
        .map(|buf| buf.iter().skip(offset).take(limit).cloned().collect())
        .unwrap_or_default();
    Ok(Json(lines))
}

pub async fn stream_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = {
        let mut txs = state.agent_log_tx.lock().await;
        let tx = txs.entry(id).or_insert_with(|| {
            let (tx, _) = broadcast::channel(256);
            tx
        });
        tx.subscribe()
    };
    let s = stream::unfold(rx, |mut rx| async move {
        let result = tokio::time::timeout(Duration::from_secs(30), rx.recv()).await;
        match result {
            Ok(Ok(msg)) => Some((Ok(Event::default().data(msg)), rx)),
            Ok(Err(_)) => None,
            Err(_) => Some((Ok(Event::default().comment("heartbeat")), rx)),
        }
    });
    Sse::new(s).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn append_and_get_logs() {
        let app = app();
        let body = serde_json::json!({ "message": "hello world" });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/agents/agent-1/logs")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp2 = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/agent-1/logs?limit=10")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
        let json = body_json(resp2).await;
        let lines = json.as_array().unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].as_str().unwrap().contains("hello world"));
    }

    #[tokio::test]
    async fn get_logs_empty() {
        let app = app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/agents/no-such-agent/logs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        assert_eq!(json.as_array().unwrap().len(), 0);
    }
}
