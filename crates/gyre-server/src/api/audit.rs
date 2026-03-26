use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream;
use gyre_domain::{AuditEvent, AuditEventType};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use crate::{
    auth::AuthenticatedAgent,
    siem::{SiemTarget, TargetType},
    AppState,
};

use super::error::ApiError;
use super::{new_id, now_secs};

// ─── Audit Events ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RecordAuditEventRequest {
    pub agent_id: String,
    pub event_type: String,
    pub path: Option<String>,
    pub details: Option<serde_json::Value>,
    pub pid: Option<u32>,
}

#[derive(Deserialize)]
pub struct QueryAuditParams {
    pub agent_id: Option<String>,
    pub event_type: Option<String>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct AuditEventResponse {
    pub id: String,
    pub agent_id: String,
    pub event_type: String,
    pub path: Option<String>,
    pub details: serde_json::Value,
    pub pid: Option<u32>,
    pub timestamp: u64,
}

impl From<AuditEvent> for AuditEventResponse {
    fn from(e: AuditEvent) -> Self {
        Self {
            id: e.id.to_string(),
            agent_id: e.agent_id.to_string(),
            event_type: e.event_type.as_str(),
            path: e.path,
            details: e.details,
            pid: e.pid,
            timestamp: e.timestamp,
        }
    }
}

pub async fn record_audit_event(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    Json(req): Json<RecordAuditEventRequest>,
) -> Result<(StatusCode, Json<AuditEventResponse>), ApiError> {
    // Bind agent_id to the verified caller identity to prevent audit trail forgery
    // (NEW-31). The request body agent_id field is ignored — the audit record always
    // reflects who actually made the call, not what the caller claims.
    let agent_id = auth.agent_id.to_string();
    let event = AuditEvent::new(
        new_id(),
        gyre_common::Id::new(agent_id),
        AuditEventType::from_str(&req.event_type),
        req.path,
        req.details
            .unwrap_or(serde_json::Value::Object(Default::default())),
        req.pid,
        now_secs(),
    );
    state.audit.record(&event).await?;

    // Broadcast to SSE stream subscribers via broadcast channel
    let _ = state
        .audit_broadcast_tx
        .send(serde_json::to_string(&AuditEventResponse::from(event.clone())).unwrap_or_default());

    Ok((StatusCode::CREATED, Json(AuditEventResponse::from(event))))
}

pub async fn query_audit_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<QueryAuditParams>,
) -> Result<Json<Vec<AuditEventResponse>>, ApiError> {
    let limit = params.limit.unwrap_or(100).min(1000);
    let events = state
        .audit
        .query(
            params.agent_id.as_deref(),
            params.event_type.as_deref(),
            params.since,
            params.until,
            limit,
        )
        .await?;
    Ok(Json(
        events.into_iter().map(AuditEventResponse::from).collect(),
    ))
}

pub async fn audit_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let total = state.audit.count().await?;
    let by_type = state.audit.stats_by_type().await?;
    Ok(Json(serde_json::json!({
        "total": total,
        "by_type": by_type.into_iter().map(|(t, c)| serde_json::json!({ "event_type": t, "count": c })).collect::<Vec<_>>(),
    })))
}

/// SSE stream of live audit events. Clients connect and receive events as they are recorded.
pub async fn audit_stream(
    State(state): State<Arc<AppState>>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = state.audit_broadcast_tx.subscribe();
    let s = stream::unfold(rx, |mut rx| async move {
        // Poll with a heartbeat timeout so the connection stays alive
        let result = tokio::time::timeout(Duration::from_secs(30), rx.recv()).await;
        match result {
            Ok(Ok(msg)) => {
                let event = Event::default().data(msg);
                Some((Ok(event), rx))
            }
            Ok(Err(_)) => None, // channel closed
            Err(_) => {
                // Timeout — send a heartbeat comment
                let event = Event::default().comment("heartbeat");
                Some((Ok(event), rx))
            }
        }
    });
    Sse::new(s).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

// ─── SIEM Targets ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateSiemTargetRequest {
    pub name: String,
    pub target_type: String,
    pub config: serde_json::Value,
    pub enabled: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateSiemTargetRequest {
    pub name: Option<String>,
    pub config: Option<serde_json::Value>,
    pub enabled: Option<bool>,
}

#[derive(Serialize)]
pub struct SiemTargetResponse {
    pub id: String,
    pub name: String,
    pub target_type: String,
    pub config: serde_json::Value,
    pub enabled: bool,
}

impl From<SiemTarget> for SiemTargetResponse {
    fn from(t: SiemTarget) -> Self {
        Self {
            id: t.id,
            name: t.name,
            target_type: t.target_type.to_string(),
            config: t.config,
            enabled: t.enabled,
        }
    }
}

pub async fn create_siem_target(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSiemTargetRequest>,
) -> Result<(StatusCode, Json<SiemTargetResponse>), ApiError> {
    let target_type = TargetType::from_str(&req.target_type).ok_or_else(|| {
        ApiError::InvalidInput(format!("unknown target_type: {}", req.target_type))
    })?;
    let target = SiemTarget {
        id: uuid::Uuid::new_v4().to_string(),
        name: req.name,
        target_type,
        config: req.config,
        enabled: req.enabled.unwrap_or(true),
    };
    state.siem_store.add(target.clone()).await;
    Ok((StatusCode::CREATED, Json(SiemTargetResponse::from(target))))
}

pub async fn list_siem_targets(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SiemTargetResponse>>, ApiError> {
    let targets = state.siem_store.list().await;
    Ok(Json(
        targets.into_iter().map(SiemTargetResponse::from).collect(),
    ))
}

pub async fn update_siem_target(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<UpdateSiemTargetRequest>,
) -> Result<Json<SiemTargetResponse>, ApiError> {
    let mut target = state
        .siem_store
        .get(&id)
        .await
        .ok_or_else(|| ApiError::NotFound(format!("SIEM target {} not found", id)))?;
    if let Some(name) = req.name {
        target.name = name;
    }
    if let Some(config) = req.config {
        target.config = config;
    }
    if let Some(enabled) = req.enabled {
        target.enabled = enabled;
    }
    state.siem_store.update(target.clone()).await;
    Ok(Json(SiemTargetResponse::from(target)))
}

pub async fn delete_siem_target(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<StatusCode, ApiError> {
    if state.siem_store.remove(&id).await {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound(format!("SIEM target {} not found", id)))
    }
}

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::build_router(test_state())
    }

    #[tokio::test]
    async fn record_audit_event_returns_201() {
        let app = app();
        // agent_id in body is ignored — caller identity from token is used (NEW-31).
        let body = serde_json::json!({
            "agent_id": "forged-agent-id",
            "event_type": "file_access",
            "path": "/etc/hosts",
            "details": { "mode": "read" },
            "pid": 1234
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/audit/events")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        // Verify the recorded agent_id reflects the token identity, not the forged body value.
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_ne!(json["agent_id"].as_str().unwrap(), "forged-agent-id",
            "audit event must not allow caller to forge agent_id (NEW-31)");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn audit_event_agent_id_bound_to_caller() {
        // NEW-31 regression: verify agent_id comes from auth, not request body.
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(seed_builtin_policies(&state))
        });

        // Agent-role JWT with known sub.
        let agent_token = sign_test_jwt(
            &serde_json::json!({
                "sub": "known-agent-sub",
                "preferred_username": "known-agent",
                "realm_access": { "roles": ["agent"] }
            }),
            3600,
        );

        let resp = crate::api::api_router()
            .with_state(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/audit/events")
                    .header("authorization", format!("Bearer {agent_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"agent_id":"evil-agent","event_type":"file_access"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // Must NOT be the forged value.
        assert_ne!(json["agent_id"].as_str().unwrap(), "evil-agent",
            "audit trail forgery must be prevented (NEW-31)");
    }

    #[tokio::test]
    async fn query_audit_events_empty() {
        let app = app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/audit/events")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json.is_array());
    }

    #[tokio::test]
    async fn audit_stats_returns_ok() {
        let app = app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/audit/stats")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json["total"].is_number());
        assert!(json["by_type"].is_array());
    }

    #[tokio::test]
    async fn siem_crud() {
        let app = app();

        // Create
        let body = serde_json::json!({
            "name": "test-webhook",
            "target_type": "webhook",
            "config": { "url": "http://example.com/siem" },
            "enabled": true
        });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/siem")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let id = created["id"].as_str().unwrap().to_string();

        // List
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/admin/siem")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let list: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1);

        // Update
        let update_body = serde_json::json!({ "enabled": false });
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/admin/siem/{}", id))
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&update_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Delete
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/admin/siem/{}", id))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}
