use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream;
use gyre_domain::{AuditEvent, AuditEventType, AuditOutcome};
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

/// Inbound audit event (agent-side push). `agent_id` is accepted but ignored -
/// the caller identity from auth is always used (NEW-31).
#[derive(Deserialize)]
pub struct RecordAuditEventRequest {
    pub agent_id: Option<String>,
    pub event_type: String,
    pub session_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub outcome: Option<AuditOutcome>,
    pub detail: Option<serde_json::Value>,
    // Legacy fields (pre-envelope schema): folded into `detail` so old
    // agent clients keep working.
    pub path: Option<String>,
    pub pid: Option<u32>,
}

#[derive(Deserialize)]
pub struct QueryAuditParams {
    pub agent_id: Option<String>,
    pub event_type: Option<String>,
    pub workspace_id: Option<String>,
    pub user_id: Option<String>,
    pub resource_type: Option<String>,
    /// Parsed manually so a typo yields the repo's ApiError shape instead
    /// of axum's generic Query rejection (task-102 F4).
    pub outcome: Option<String>,
    pub since: Option<u64>,
    pub until: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct AuditEventResponse {
    pub id: String,
    pub event_type: String,
    pub agent_id: Option<String>,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub outcome: AuditOutcome,
    pub detail: serde_json::Value,
    pub source_ip: Option<String>,
    pub user_agent: Option<String>,
    pub timestamp: u64,
}

impl From<AuditEvent> for AuditEventResponse {
    fn from(e: AuditEvent) -> Self {
        Self {
            id: e.id.to_string(),
            event_type: e.event_type.as_str(),
            agent_id: e.agent_id.map(|id| id.to_string()),
            user_id: e.user_id.map(|id| id.to_string()),
            session_id: e.session_id,
            workspace_id: e.workspace_id.map(|id| id.to_string()),
            repo_id: e.repo_id.map(|id| id.to_string()),
            resource_type: e.resource_type,
            resource_id: e.resource_id,
            outcome: e.outcome,
            detail: e.detail,
            source_ip: e.source_ip,
            user_agent: e.user_agent,
            timestamp: e.timestamp,
        }
    }
}

/// Record a server-initiated audit event and broadcast it to SSE subscribers.
///
/// Shared emission path for every server-side audit event (task-104:
/// observability.md §Audit Event Types source control + access control
/// events). Recording is best-effort — a storage failure is logged and the
/// requesting path continues; the audit trail must never take down the
/// operation it observes.
pub async fn emit_server_audit_event(state: &AppState, event: AuditEvent) {
    if let Err(e) = state.audit.record(&event).await {
        tracing::warn!(
            error = %e,
            event_type = event.event_type.as_str(),
            "failed to record server-side audit event"
        );
    }
    if let Ok(json) = serde_json::to_string(&AuditEventResponse::from(event)) {
        let _ = state.audit_broadcast_tx.send(json);
    }
}

/// Caller-scope validation (task-102 F2): the scoping fields on a recorded
/// audit event must resolve within the caller's tenant. `agent_id` is bound
/// server-side in the handler; letting `workspace_id`/`repo_id` pass through
/// unvalidated would be partial enforcement of the same anti-forgery
/// principle — any authenticated agent could attribute its events to
/// another workspace and corrupt the compliance record the query API
/// filters on. Each named scope is looked up and its tenant compared
/// against the caller's; mismatch or unknown id rejects the write.
/// Shared with the analytics record handler (task-146 review F1).
pub(crate) async fn validate_workspace_scope(
    state: &AppState,
    id: &gyre_common::Id,
    auth: &AuthenticatedAgent,
) -> Result<(), ApiError> {
    let ws = state
        .workspaces
        .find_by_id(id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {id} not found")))?;
    if ws.tenant_id.as_str() != auth.tenant_id {
        return Err(ApiError::Forbidden(format!(
            "workspace {id} is outside the caller's tenant"
        )));
    }
    Ok(())
}

/// Repo-scope validation (task-102 F2): the repo must exist, its workspace
/// must resolve within the caller's tenant, and — when the event is also
/// labeled with a workspace — the repo must belong to that workspace.
/// Inconsistent or cross-tenant attribution is rejected.
pub(crate) async fn validate_repo_scope(
    state: &AppState,
    id: &gyre_common::Id,
    auth: &AuthenticatedAgent,
    workspace_id: Option<&gyre_common::Id>,
) -> Result<(), ApiError> {
    let repo = state
        .repos
        .find_by_id(id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("repo {id} not found")))?;
    // The repo's workspace is authoritative for its tenant scope.
    let repo_ws = state
        .workspaces
        .find_by_id(&repo.workspace_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("workspace {} not found", repo.workspace_id)))?;
    if repo_ws.tenant_id.as_str() != auth.tenant_id {
        return Err(ApiError::Forbidden(format!(
            "repo {id} is outside the caller's tenant"
        )));
    }
    if let Some(ws) = workspace_id {
        if *ws != repo.workspace_id {
            return Err(ApiError::Forbidden(format!(
                "repo {id} does not belong to workspace {ws}"
            )));
        }
    }
    Ok(())
}

/// Decision-scope validation for the analytics Decision API (analytics.md
/// §Decision API for Agents): a decision context naming a
/// `workspace_id`/`repo_id` scopes the events the decision is computed
/// over, so a forged id would surface another scope's aggregated
/// analytics through the decision outcome. Same lookup-and-compare rule
/// as `validate_workspace_scope`/`validate_repo_scope` above; returns the
/// validated ids for the event query filter. Absent fields scope to
/// nothing (platform-wide), consistent with the analytics query API.
pub(crate) async fn validate_decision_scope(
    state: &AppState,
    context: &serde_json::Value,
    auth: &AuthenticatedAgent,
) -> Result<(Option<gyre_common::Id>, Option<gyre_common::Id>), ApiError> {
    let workspace_id = match context.get("workspace_id").and_then(|v| v.as_str()) {
        Some(w) => {
            let id = gyre_common::Id::new(w);
            validate_workspace_scope(state, &id, auth).await?;
            Some(id)
        }
        None => None,
    };
    let repo_id = match context.get("repo_id").and_then(|v| v.as_str()) {
        Some(r) => {
            let id = gyre_common::Id::new(r);
            validate_repo_scope(state, &id, auth, workspace_id.as_ref()).await?;
            Some(id)
        }
        None => None,
    };
    Ok((workspace_id, repo_id))
}

pub async fn record_audit_event(
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
    connect_info: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<RecordAuditEventRequest>,
) -> Result<(StatusCode, Json<AuditEventResponse>), ApiError> {
    // Bind agent_id to the verified caller identity to prevent audit trail forgery
    // (NEW-31). The request body agent_id field is ignored - the audit record always
    // reflects who actually made the call, not what the caller claims.
    // Agent JWT callers carry an agent identity; user/API-key callers carry user_id.
    let agent_id = if auth.agent_id.is_empty() {
        None
    } else {
        Some(gyre_common::Id::new(auth.agent_id.clone()))
    };
    // Identity-security.md §Impersonation: events pushed during an
    // impersonation session are attributed to the impersonator (the admin
    // acting as the target), with the attribution context in detail.
    let user_id = auth
        .impersonation
        .as_ref()
        .map(|imp| imp.impersonator_id.clone())
        .or_else(|| auth.user_id.clone());

    // Caller-scope validation (task-102 F2), see `validate_workspace_scope` /
    // `validate_repo_scope` above: mismatch or unknown scope rejects the
    // write instead of persisting an attribution the caller forged.
    let workspace_id = match req.workspace_id {
        Some(w) => {
            let id = gyre_common::Id::new(w);
            validate_workspace_scope(&state, &id, &auth).await?;
            Some(id)
        }
        None => None,
    };
    let repo_id = match req.repo_id {
        Some(r) => {
            let id = gyre_common::Id::new(r);
            validate_repo_scope(&state, &id, &auth, workspace_id.as_ref()).await?;
            Some(id)
        }
        None => None,
    };

    // Forensic source IP (task-102 F3, CWE-348): the socket peer address is
    // the source of truth; X-Forwarded-For / X-Real-Ip are caller-controlled
    // and honored only when the immediate peer is a configured trusted proxy
    // (GYRE_TRUSTED_PROXIES, empty by default = every connection is direct).
    // Fails closed to None when no socket information is available.
    let source_ip = state
        .trusted_proxies
        .resolve_source_ip(connect_info.map(|c| c.0.ip()), &headers);
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Event-specific payload: merge legacy path/pid into detail so the
    // envelope stays the storage contract while old clients keep working.
    let mut detail = req
        .detail
        .unwrap_or(serde_json::Value::Object(Default::default()));
    if let (Some(obj), path) = (detail.as_object_mut(), req.path) {
        if let Some(p) = path {
            obj.entry("path").or_insert(serde_json::json!(p));
        }
    }
    if let (Some(obj), pid) = (detail.as_object_mut(), req.pid) {
        if let Some(p) = pid {
            obj.entry("pid").or_insert(serde_json::json!(p));
        }
    }
    // Impersonation attribution context (identity-security.md §Impersonation):
    // the record's user_id is the impersonator; detail carries both identities
    // plus the session so the trail shows the impersonator acting as the target.
    if let (Some(obj), Some(imp)) = (detail.as_object_mut(), auth.impersonation.as_ref()) {
        obj.entry("impersonator_id")
            .or_insert(serde_json::json!(imp.impersonator_id.as_str()));
        obj.entry("target_user_id")
            .or_insert(serde_json::json!(imp.target_user_id.as_str()));
        obj.entry("impersonation_session_id")
            .or_insert(serde_json::json!(imp.session_id));
    }

    let event = AuditEvent::new(
        new_id(),
        AuditEventType::from_str(&req.event_type),
        agent_id,
        user_id,
        req.session_id,
        workspace_id,
        repo_id,
        req.resource_type.unwrap_or_else(|| "agent".to_string()),
        req.resource_id,
        req.outcome.unwrap_or(AuditOutcome::Success),
        detail,
        source_ip,
        user_agent,
        now_secs(),
    );
    state.audit.record(&event).await?;
    // search.md §Searchable Entities: audit events are indexed on write
    // (event_type, details, path facets).
    crate::search_indexer::index(&state, crate::search_indexer::audit_event_document(&event)).await;

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
    // Parse the typed enum filter manually so an invalid value returns the
    // repo's ApiError JSON shape (400 {"error": ...}) rather than axum's
    // generic Query rejection (task-102 F4).
    let outcome = match params.outcome.as_deref() {
        None => None,
        Some(s) => Some(AuditOutcome::from_str(s).ok_or_else(|| {
            ApiError::InvalidInput(format!(
                "invalid outcome filter '{s}': expected one of success|failure|blocked"
            ))
        })?),
    };
    let filter = gyre_ports::AuditQueryFilter {
        agent_id: params.agent_id,
        event_type: params.event_type,
        workspace_id: params.workspace_id,
        user_id: params.user_id,
        resource_type: params.resource_type,
        outcome,
        since: params.since,
        until: params.until,
        limit: params.limit.unwrap_or(100).min(1000),
    };
    let events = state.audit.query(&filter).await?;
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
                // Timeout - send a heartbeat comment
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
        // agent_id in body is ignored - caller identity from token is used (NEW-31).
        let body = serde_json::json!({
            "agent_id": "forged-agent-id",
            "event_type": "file_access",
            "detail": { "mode": "read", "path": "/etc/hosts", "pid": 1234 },
            "resource_type": "agent"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/audit/events")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    // Forged forwarded headers must NOT become source_ip: with
                    // no trusted proxies configured (test default) and no
                    // socket peer attached to this oneshot request, the
                    // resolution fails closed to null (task-102 F3, CWE-348).
                    .header("X-Forwarded-For", "198.51.100.7, 10.0.0.1")
                    .header("X-Real-Ip", "198.51.100.99")
                    .header("User-Agent", "gyre-agent/1.0")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // Verify the recorded agent_id reflects the token identity, not the forged body value.
        assert_ne!(
            json["agent_id"].as_str().unwrap(),
            "forged-agent-id",
            "audit event must not allow caller to forge agent_id (NEW-31)"
        );
        // F3: caller-controlled forwarded headers are never trusted without a
        // socket peer + trusted-proxy gate.
        assert!(
            json["source_ip"].is_null(),
            "source_ip must fail closed without socket info, got {:?}",
            json["source_ip"]
        );
        assert_eq!(json["user_agent"].as_str().unwrap(), "gyre-agent/1.0");
        assert_eq!(json["resource_type"].as_str().unwrap(), "agent");
        assert_eq!(json["outcome"].as_str().unwrap(), "success");
        // All 14 envelope fields present in the response.
        for key in [
            "id",
            "event_type",
            "agent_id",
            "user_id",
            "session_id",
            "workspace_id",
            "repo_id",
            "resource_type",
            "resource_id",
            "outcome",
            "detail",
            "source_ip",
            "user_agent",
            "timestamp",
        ] {
            assert!(
                json.get(key).is_some(),
                "response missing envelope field {key}"
            );
        }
    }

    #[tokio::test]
    async fn source_ip_uses_socket_peer_and_ignores_forged_xff_without_trusted_proxy() {
        // Direct connection (default deployment): the socket peer is the
        // source; X-Forwarded-For is caller-controlled and ignored (F3).
        use axum::extract::ConnectInfo;
        let app = app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/audit/events")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .header("X-Forwarded-For", "1.2.3.4")
                    .extension(ConnectInfo(
                        "203.0.113.9:50000".parse::<std::net::SocketAddr>().unwrap(),
                    ))
                    .body(Body::from(r#"{"event_type":"file_access"}"#.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["source_ip"].as_str().unwrap(), "203.0.113.9");
    }

    #[tokio::test]
    async fn source_ip_honors_xff_only_from_trusted_proxy_peer() {
        // Peer inside a trusted-proxy network: X-Forwarded-For is honored,
        // rightmost-untrusted entry wins (the actual client).
        use axum::extract::ConnectInfo;
        let mut state = (*crate::mem::test_state()).clone();
        state.trusted_proxies = crate::trusted_proxy::TrustedProxies::from_csv("10.0.0.0/8");
        let state = std::sync::Arc::new(state);
        let app = crate::build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/audit/events")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .header("X-Forwarded-For", "198.51.100.7, 10.0.0.1")
                    .extension(ConnectInfo(
                        "10.0.0.2:40000".parse::<std::net::SocketAddr>().unwrap(),
                    ))
                    .body(Body::from(r#"{"event_type":"file_access"}"#.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["source_ip"].as_str().unwrap(), "198.51.100.7");
    }

    #[tokio::test]
    async fn audit_event_workspace_scope_validated_against_caller_tenant() {
        // F2: a caller may only attribute audit events to scopes that
        // resolve within its own tenant (global token -> "default").
        let state = crate::mem::test_state();
        let own_ws = gyre_domain::Workspace::new(
            gyre_common::Id::new("ws-own"),
            gyre_common::Id::new("default"),
            "Own",
            "own",
            1000,
        );
        state.workspaces.create(&own_ws).await.unwrap();
        let foreign_ws = gyre_domain::Workspace::new(
            gyre_common::Id::new("ws-foreign"),
            gyre_common::Id::new("tenant-other"),
            "Foreign",
            "foreign",
            1000,
        );
        state.workspaces.create(&foreign_ws).await.unwrap();
        let app = crate::build_router(state);

        let post = |ws: &str| {
            let body = serde_json::json!({
                "event_type": "file_access",
                "workspace_id": ws
            });
            app.clone().oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/audit/events")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
        };

        // Workspace in the caller's tenant: accepted and attributed.
        let resp = post("ws-own").await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["workspace_id"].as_str().unwrap(), "ws-own");

        // Workspace in another tenant: Forbidden — no cross-tenant attribution.
        let resp = post("ws-foreign").await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Unknown workspace: NotFound, not a silent attribution.
        let resp = post("ws-nope").await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn audit_event_repo_scope_validated_against_caller_tenant() {
        // F2: repo attribution requires the repo to exist and its workspace
        // to resolve within the caller's tenant; a repo/workspace mismatch
        // is rejected as inconsistent attribution.
        let state = crate::mem::test_state();
        let own_ws = gyre_domain::Workspace::new(
            gyre_common::Id::new("ws-own"),
            gyre_common::Id::new("default"),
            "Own",
            "own",
            1000,
        );
        state.workspaces.create(&own_ws).await.unwrap();
        let foreign_ws = gyre_domain::Workspace::new(
            gyre_common::Id::new("ws-foreign"),
            gyre_common::Id::new("tenant-other"),
            "Foreign",
            "foreign",
            1000,
        );
        state.workspaces.create(&foreign_ws).await.unwrap();
        state
            .repos
            .create(&gyre_domain::Repository::new(
                gyre_common::Id::new("repo-own"),
                gyre_common::Id::new("ws-own"),
                "own-repo",
                "/repos/own-repo",
                1000,
            ))
            .await
            .unwrap();
        // A second workspace in the caller's OWN tenant, with its own repo:
        // both scopes individually authorized, but a repo event labeled with
        // the other workspace is inconsistent attribution.
        let own_ws2 = gyre_domain::Workspace::new(
            gyre_common::Id::new("ws-own-2"),
            gyre_common::Id::new("default"),
            "Own 2",
            "own-2",
            1000,
        );
        state.workspaces.create(&own_ws2).await.unwrap();
        state
            .repos
            .create(&gyre_domain::Repository::new(
                gyre_common::Id::new("repo-own-2"),
                gyre_common::Id::new("ws-own-2"),
                "own-repo-2",
                "/repos/own-repo-2",
                1000,
            ))
            .await
            .unwrap();
        state
            .repos
            .create(&gyre_domain::Repository::new(
                gyre_common::Id::new("repo-foreign"),
                gyre_common::Id::new("ws-foreign"),
                "foreign-repo",
                "/repos/foreign-repo",
                1000,
            ))
            .await
            .unwrap();
        let app = crate::build_router(state);

        let post = |body: serde_json::Value| {
            app.clone().oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/audit/events")
                    .header("Authorization", "Bearer test-token")
                    .header("Content-Type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
        };

        // Own repo, consistent workspace: accepted.
        let resp = post(serde_json::json!({
            "event_type": "file_access",
            "workspace_id": "ws-own",
            "repo_id": "repo-own"
        }))
        .await
        .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Repo in another tenant: Forbidden.
        let resp = post(serde_json::json!({
            "event_type": "file_access",
            "repo_id": "repo-foreign"
        }))
        .await
        .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Repo in the caller's tenant but labeled with a different
        // (individually authorized) workspace: inconsistent attribution
        // is Forbidden.
        let resp = post(serde_json::json!({
            "event_type": "file_access",
            "workspace_id": "ws-own",
            "repo_id": "repo-own-2"
        }))
        .await
        .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);

        // Unknown repo: NotFound.
        let resp = post(serde_json::json!({
            "event_type": "file_access",
            "repo_id": "repo-nope"
        }))
        .await
        .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn record_audit_event_legacy_path_pid_folded_into_detail() {
        // Pre-envelope clients send path/pid at the top level; the server
        // folds them into detail so the data is not lost.
        let app = app();
        let body = serde_json::json!({
            "event_type": "file_access",
            "path": "/etc/hosts",
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
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["detail"]["path"].as_str().unwrap(), "/etc/hosts");
        assert_eq!(json["detail"]["pid"].as_u64().unwrap(), 1234);
        assert!(
            json.get("path").is_none(),
            "path must not be a top-level field"
        );
        assert!(
            json.get("pid").is_none(),
            "pid must not be a top-level field"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn audit_event_agent_id_bound_to_caller() {
        // NEW-31 regression: verify agent_id comes from auth, not request body.
        use crate::abac_middleware::seed_builtin_policies;
        use crate::auth::test_helpers::{make_test_state_with_jwt, sign_test_jwt};
        let state = make_test_state_with_jwt();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(seed_builtin_policies(&state))
                .expect("seed built-in policies")
        });

        // Agent-role JWT with known sub.
        let agent_token = sign_test_jwt(
            &serde_json::json!({
                "tenant_id": "default",
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
                    .body(Body::from(
                        r#"{"agent_id":"evil-agent","event_type":"file_access"}"#,
                    ))
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
        assert_ne!(
            json["agent_id"].as_str().unwrap(),
            "evil-agent",
            "audit trail forgery must be prevented (NEW-31)"
        );
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
    async fn query_audit_events_filters_by_outcome() {
        // Record two events with different outcomes, then filter.
        let state = test_state();
        let app = crate::build_router(state.clone());
        for (outcome, et) in [("failure", "auth_failure"), ("success", "file_access")] {
            let body = serde_json::json!({
                "event_type": et,
                "outcome": outcome,
                "resource_type": "agent"
            });
            let resp = app
                .clone()
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
        }
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/audit/events?outcome=failure")
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
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["outcome"].as_str().unwrap(), "failure");
        assert_eq!(arr[0]["event_type"].as_str().unwrap(), "auth_failure");
    }

    #[tokio::test]
    async fn query_audit_events_invalid_outcome_returns_api_error_shape() {
        // F4: a typo'd outcome filter must return the repo's ApiError JSON
        // shape (400 {"error": ...}), not axum's generic Query rejection.
        let app = app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/audit/events?outcome=succes")
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let msg = json["error"].as_str().unwrap();
        assert!(
            msg.contains("outcome") && msg.contains("succes"),
            "error message should name the field and the bad value: {msg}"
        );
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
