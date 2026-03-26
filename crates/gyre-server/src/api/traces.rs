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
use gyre_common::Id;
use serde::Serialize;
use std::sync::Arc;
use tracing::instrument;

use crate::AppState;

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

    let spans = trace
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

    Ok(Json(GateTraceResponse {
        id: trace.id.as_str().to_string(),
        mr_id: trace.mr_id.as_str().to_string(),
        gate_run_id: trace.gate_run_id.as_str().to_string(),
        commit_sha: trace.commit_sha,
        captured_at: trace.captured_at,
        spans,
        root_spans,
    }))
}

/// GET /api/v1/trace-spans/:span_id/payload
///
/// Returns the full input/output payload for a specific span (base64-encoded).
/// Per-handler auth: resolves span → gate_trace → MR → workspace for authorization.
/// Listed in hierarchy-enforcement.md §4 ABAC-exempt list.
/// 404 if the span has no stored payload.
///
/// Note: `span_id` in the URL is the compound "trace_id-span_id" format used
/// when storing spans. The `gate_run_id` query parameter is required to uniquely
/// identify the trace (span_ids are only unique within a trace).
#[instrument(skip(state), fields(span_id = %span_id))]
pub async fn get_span_payload(
    Path(span_id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<SpanPayloadQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<(StatusCode, Json<SpanPayloadResponse>), ApiError> {
    let gate_run_id = Id::new(
        params
            .gate_run_id
            .as_deref()
            .ok_or_else(|| ApiError::BadRequest("gate_run_id query param required".to_string()))?,
    );

    // Per-handler auth: verify the caller has access to the MR this span belongs to.
    // The TraceRepository is already tenant-scoped (via SqliteStorage.with_tenant()),
    // so a cross-tenant lookup returns None naturally.

    let payload = state
        .traces
        .get_span_payload(&gate_run_id, &span_id)
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

#[derive(Debug, serde::Deserialize)]
pub struct SpanPayloadQuery {
    pub gate_run_id: Option<String>,
}
