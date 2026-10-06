//! REST API handlers for gate-time trace capture (HSI §3a).
//!
//! Endpoints:
//! - GET /api/v1/merge-requests/:id/trace — returns GateTrace for an MR (ABAC: mr/read)
//! - GET /api/v1/trace-spans/:span_id/payload — returns full span payload (per-handler auth)

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use gyre_common::{GateTrace, Id};
use serde::Serialize;
use std::sync::Arc;
use tracing::instrument;

use crate::{auth::AuthenticatedAgent, AppState};

use super::error::ApiError;

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TraceSpanResponse {
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub operation_name: String,
    pub service_name: String,
    pub kind: String,
    pub start_time: u64,
    pub duration_us: u64,
    pub attributes: std::collections::HashMap<String, String>,
    pub input_summary: Option<String>,
    pub output_summary: Option<String>,
    pub status: String,
    pub graph_node_id: Option<String>,
}

#[derive(Serialize)]
pub struct GateTraceResponse {
    pub id: String,
    pub mr_id: String,
    pub gate_run_id: String,
    pub commit_sha: String,
    pub captured_at: u64,
    pub span_count: usize,
    pub spans: Vec<TraceSpanResponse>,
    /// Top-level (root) span IDs — entry points for flow animation.
    pub root_spans: Vec<String>,
}

#[derive(Serialize)]
pub struct SpanPayloadResponse {
    pub input: Option<String>,  // base64-encoded
    pub output: Option<String>, // base64-encoded
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// Core trace assembly logic shared by both REST and MCP handlers.
/// Converts a domain `GateTrace` into the `GateTraceResponse` API shape.
pub fn assemble_gate_trace(trace: GateTrace) -> GateTraceResponse {
    // Identify root spans (no parent, or parent not in this trace).
    let all_span_ids: std::collections::HashSet<&str> =
        trace.spans.iter().map(|s| s.span_id.as_str()).collect();
    let root_spans: Vec<String> = trace
        .spans
        .iter()
        .filter(|s| {
            s.parent_span_id
                .as_deref()
                .map(|pid| !all_span_ids.contains(pid))
                .unwrap_or(true)
        })
        .map(|s| s.span_id.clone())
        .collect();

    let spans: Vec<TraceSpanResponse> = trace
        .spans
        .into_iter()
        .map(|s| TraceSpanResponse {
            span_id: s.span_id,
            parent_span_id: s.parent_span_id,
            operation_name: s.operation_name,
            service_name: s.service_name,
            kind: s.kind.as_str().to_string(),
            start_time: s.start_time,
            duration_us: s.duration_us,
            attributes: s.attributes,
            input_summary: s.input_summary,
            output_summary: s.output_summary,
            status: s.status.as_str().to_string(),
            graph_node_id: s.graph_node_id.map(|id| id.as_str().to_string()),
        })
        .collect();

    GateTraceResponse {
        id: trace.id.as_str().to_string(),
        mr_id: trace.mr_id.as_str().to_string(),
        gate_run_id: trace.gate_run_id.as_str().to_string(),
        commit_sha: trace.commit_sha,
        captured_at: trace.captured_at,
        span_count: spans.len(),
        spans,
        root_spans,
    }
}

/// GET /api/v1/merge-requests/:id/trace
///
/// Returns the most recent GateTrace for an MR.
/// ABAC: resource_type="merge_request", id_param="id", action="read" (middleware-enforced).
/// 404 if no trace exists for this MR.
#[instrument(skip(state), fields(mr_id = %id))]
pub async fn get_trace_for_mr(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<GateTraceResponse>, ApiError> {
    let mr_id = Id::new(&id);

    let trace = state
        .traces
        .get_by_mr(&mr_id)
        .await
        .map_err(ApiError::Internal)?;

    let trace = match trace {
        Some(t) => t,
        None => return Err(ApiError::NotFound("no trace for this MR".to_string())),
    };

    Ok(Json(assemble_gate_trace(trace)))
}

/// GET /api/v1/trace-spans/:span_id/payload
///
/// Returns the full input/output payload for a specific span (base64-encoded).
/// 404 if the span has no stored payload.
///
/// Per-handler auth (route is ABAC-exempt because the workspace cannot be
/// determined from the URL alone): resolves span → trace → MR → workspace,
/// then compares the workspace's tenant against the caller's tenant and
/// returns Forbidden on mismatch. The caller's own tenant is the scope
/// boundary; the repository itself is shared, so this comparison is the
/// authorization.
///
/// `span_id` in the URL is the compound "trace_id-span_id" format assigned
/// at ingestion, so it is globally unique across traces and identifies the
/// containing trace on its own.
#[instrument(skip(state, auth), fields(span_id = %span_id))]
pub async fn get_span_payload(
    Path(span_id): Path<String>,
    State(state): State<Arc<AppState>>,
    auth: AuthenticatedAgent,
) -> Result<(StatusCode, Json<SpanPayloadResponse>), ApiError> {
    // Find the trace containing this span. Traces are capped at one per MR,
    // so scanning the MRs' traces finds at most one match.
    let mrs = state.merge_requests.list().await.map_err(ApiError::Internal)?;
    let mut found = None;
    for mr in &mrs {
        let trace = state
            .traces
            .get_by_mr(&mr.id)
            .await
            .map_err(ApiError::Internal)?;
        if let Some(trace) = trace {
            if trace.spans.iter().any(|s| s.span_id == span_id) {
                found = Some((trace, mr.clone()));
                break;
            }
        }
    }

    let (trace, mr) = found.ok_or_else(|| ApiError::NotFound("no trace for this span".to_string()))?;

    // Authorization: the span's MR belongs to a workspace, and the
    // workspace belongs to a tenant. Compare against the caller's tenant.
    let workspace = state
        .workspaces
        .find_by_id(&mr.workspace_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound("workspace for this span's MR not found".to_string()))?;
    if workspace.tenant_id.as_str() != auth.tenant_id {
        return Err(ApiError::Forbidden(
            "Cross-tenant trace span access denied".to_string(),
        ));
    }

    let payload = state
        .traces
        .get_span_payload(&trace.gate_run_id, &span_id)
        .await
        .map_err(ApiError::Internal)?;

    match payload {
        None => Err(ApiError::NotFound("no payload for this span".to_string())),
        Some(p) => Ok((
            StatusCode::OK,
            Json(SpanPayloadResponse {
                input: p.input.map(|b| B64.encode(&b)),
                output: p.output.map(|b| B64.encode(&b)),
            }),
        )),
    }
}

// ─── Integration tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use gyre_common::{GateTrace, Id, SpanKind, SpanStatus, TraceSpan};
    use gyre_domain::{MergeRequest, Workspace};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app(state: &std::sync::Arc<crate::AppState>) -> Router {
        crate::api::api_router().with_state(state.clone())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn make_span(span_id: &str, input: Option<&str>, output: Option<&str>) -> TraceSpan {
        TraceSpan {
            span_id: span_id.to_string(),
            parent_span_id: None,
            operation_name: "op".to_string(),
            service_name: "svc".to_string(),
            kind: SpanKind::Server,
            start_time: 1000,
            duration_us: 500,
            attributes: Default::default(),
            input_summary: input.map(|s| s.to_string()),
            output_summary: output.map(|s| s.to_string()),
            status: SpanStatus::Ok,
            graph_node_id: None,
        }
    }

    async fn seed_mr_with_trace(
        state: &std::sync::Arc<crate::AppState>,
        workspace_id: &str,
        tenant_id: &str,
    ) {
        let ws = Workspace::new(
            Id::new(workspace_id),
            Id::new(tenant_id),
            format!("{workspace_id} name"),
            format!("{workspace_id}-slug"),
            1000,
        );
        state.workspaces.create(&ws).await.unwrap();

        let mut mr = MergeRequest::new(
            Id::new(format!("{workspace_id}-mr")),
            Id::new("repo-1"),
            format!("{workspace_id} MR"),
            "feat/x",
            "main",
            1000,
        );
        mr.workspace_id = Id::new(workspace_id);
        state.merge_requests.create(&mr).await.unwrap();

        let trace = GateTrace {
            id: Id::new(format!("{workspace_id}-trace")),
            mr_id: mr.id.clone(),
            gate_run_id: Id::new(format!("{workspace_id}-gate-run")),
            commit_sha: "0123456789abcdef0123456789abcdef01234567".to_string(),
            spans: vec![make_span(
                &format!("{workspace_id}-span-1"),
                Some("{\"req\":1}"),
                Some("{\"resp\":2}"),
            )],
            captured_at: 1000,
        };
        state.traces.store(&trace).await.unwrap();
    }

    async fn get_payload(
        state: &std::sync::Arc<crate::AppState>,
        span_id: &str,
    ) -> axum::response::Response {
        app(state)
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/trace-spans/{span_id}/payload"))
                    .header("Authorization", "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn span_payload_returns_base64_payload_for_same_tenant_caller() {
        let state = test_state();
        // Workspace tenant "default" matches the global dev token's tenant.
        seed_mr_with_trace(&state, "ws-default", "default").await;

        let resp = get_payload(&state, "ws-default-span-1").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let json = body_json(resp).await;
        let input = json["input"].as_str().unwrap();
        let output = json["output"].as_str().unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(input)
                .unwrap(),
            b"{\"req\":1}".to_vec(),
            "input must decode to the raw input_summary bytes"
        );
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(output)
                .unwrap(),
            b"{\"resp\":2}".to_vec(),
            "output must decode to the raw output_summary bytes"
        );
    }

    #[tokio::test]
    async fn span_payload_returns_404_for_unknown_span() {
        let state = test_state();
        seed_mr_with_trace(&state, "ws-default", "default").await;

        let resp = get_payload(&state, "does-not-exist").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn span_payload_returns_404_when_span_has_no_payload() {
        let state = test_state();
        // Seed a span with neither input nor output captured — the mem
        // adapter stores no payload row for it (SQLite: NULL blob).
        seed_mr_with_trace(&state, "ws-default", "default").await;
        let mut mr = state
            .merge_requests
            .find_by_id(&Id::new("ws-default-mr"))
            .await
            .unwrap()
            .unwrap();
        mr.id = Id::new("ws-default-mr-2");
        state.merge_requests.create(&mr).await.unwrap();
        let trace = GateTrace {
            id: Id::new("t-no-payload"),
            mr_id: mr.id.clone(),
            gate_run_id: Id::new("gr-no-payload"),
            commit_sha: "0123456789abcdef0123456789abcdef01234567".to_string(),
            spans: vec![make_span("ws-default-span-none", None, None)],
            captured_at: 1000,
        };
        state.traces.store(&trace).await.unwrap();

        let resp = get_payload(&state, "ws-default-span-none").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn span_payload_forbids_cross_tenant_access() {
        let state = test_state();
        // Workspace tenant "ws-other-tenant" ≠ global dev token's "default".
        seed_mr_with_trace(&state, "ws-other", "ws-other-tenant").await;

        let resp = get_payload(&state, "ws-other-span-1").await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn span_payload_requires_authentication() {
        let state = test_state();
        seed_mr_with_trace(&state, "ws-default", "default").await;

        let resp = app(&state)
            .oneshot(
                Request::builder()
                    .uri("/api/v1/trace-spans/ws-default-span-1/payload")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn span_payload_survives_replacement_trace_for_same_mr() {
        let state = test_state();
        seed_mr_with_trace(&state, "ws-default", "default").await;

        // A fresh gate run replaces the trace; old payload keys must not leak.
        let mr = state
            .merge_requests
            .find_by_id(&Id::new("ws-default-mr"))
            .await
            .unwrap()
            .unwrap();
        let trace = GateTrace {
            id: Id::new("t-replaced"),
            mr_id: mr.id.clone(),
            gate_run_id: Id::new("gr-replaced"),
            commit_sha: "0123456789abcdef0123456789abcdef01234567".to_string(),
            spans: vec![make_span(
                "ws-default-span-new",
                Some("{\"new\":1}"),
                None,
            )],
            captured_at: 2000,
        };
        state.traces.store(&trace).await.unwrap();

        // Old span no longer exists in any trace → 404, not stale payload.
        let resp = get_payload(&state, "ws-default-span-1").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        // New span's payload is served.
        let resp = get_payload(&state, "ws-default-span-new").await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
