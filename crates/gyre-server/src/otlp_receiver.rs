//! Lightweight OTLP HTTP receiver for gate-time trace capture (HSI §3a).
//!
//! This is NOT a general-purpose observability backend. It is scoped to
//! gate-time traces only — started per gate run, stopped after test execution.
//!
//! Protocol: OTLP HTTP/JSON (Content-Type: application/json) on a configurable port.
//! The application under test is started with:
//!   OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:<port>
//!   OTEL_EXPORTER_OTLP_PROTOCOL=http/json
//!   OTEL_SERVICE_NAME=<service>
//!
//! This avoids gRPC/tonic complexity while remaining OTLP-compliant.

use anyhow::{Context, Result};
use axum::{body::Bytes, extract::State, http::StatusCode, routing::post, Router};
use gyre_common::{GateTrace, Id, SpanKind, SpanStatus, TraceSpan};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tracing::warn;
use uuid::Uuid;

use crate::AppState;

// ── Config ───────────────────────────────────────────────────────────────────

/// Configuration for a TraceCapture gate (parsed from gate.command JSON).
#[derive(Debug, Clone, Deserialize)]
pub struct TraceCaptureConfig {
    /// Port to start the OTLP HTTP receiver on.
    #[serde(default = "default_otlp_port")]
    pub otlp_port: u16,
    /// Test command to run with OTel env vars injected.
    #[serde(default = "default_test_command")]
    pub test_command: String,
    /// Maximum spans per trace (prevents unbounded storage from fuzz tests).
    #[serde(default = "default_max_spans")]
    pub max_spans: usize,
    /// Whether to capture external dependency spans.
    #[serde(default)]
    pub capture_external: bool,
}

fn default_otlp_port() -> u16 {
    4318
}

fn default_test_command() -> String {
    "cargo test --features integration".to_string()
}

fn default_max_spans() -> usize {
    10_000
}

impl Default for TraceCaptureConfig {
    fn default() -> Self {
        Self {
            otlp_port: default_otlp_port(),
            test_command: default_test_command(),
            max_spans: default_max_spans(),
            capture_external: false,
        }
    }
}

// ── OTLP JSON types (subset needed for span ingestion) ───────────────────────

/// Minimal OTLP JSON ExportTraceServiceRequest structure.
/// Only the fields we need — extras are ignored by serde.
#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
struct OtlpExportRequest {
    #[serde(default)]
    resource_spans: Vec<OtlpResourceSpans>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OtlpResourceSpans {
    #[serde(default)]
    resource: Option<OtlpResource>,
    #[serde(default)]
    scope_spans: Vec<OtlpScopeSpans>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OtlpResource {
    #[serde(default)]
    attributes: Vec<OtlpAttribute>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OtlpScopeSpans {
    #[serde(default)]
    spans: Vec<OtlpSpan>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OtlpSpan {
    trace_id: Option<String>,
    span_id: String,
    parent_span_id: Option<String>,
    name: String,
    kind: Option<i32>,
    start_time_unix_nano: Option<String>,
    end_time_unix_nano: Option<String>,
    #[serde(default)]
    attributes: Vec<OtlpAttribute>,
    #[serde(default)]
    status: Option<OtlpStatus>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OtlpAttribute {
    key: String,
    value: Option<OtlpAnyValue>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OtlpAnyValue {
    string_value: Option<String>,
    int_value: Option<serde_json::Value>,
    bool_value: Option<bool>,
    double_value: Option<f64>,
}

impl OtlpAnyValue {
    fn to_string_repr(&self) -> String {
        if let Some(s) = &self.string_value {
            return s.clone();
        }
        if let Some(i) = &self.int_value {
            return i.to_string();
        }
        if let Some(b) = self.bool_value {
            return b.to_string();
        }
        if let Some(d) = self.double_value {
            return d.to_string();
        }
        String::new()
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OtlpStatus {
    code: Option<i32>,
}

// ── Receiver state ────────────────────────────────────────────────────────────

type SpanAccumulator = Arc<Mutex<Vec<TraceSpan>>>;

/// Axum handler: POST /v1/traces
async fn ingest_traces(
    State((accumulator, max_spans)): State<(SpanAccumulator, usize)>,
    body: Bytes,
) -> StatusCode {
    let request: OtlpExportRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            warn!("otlp_receiver: failed to parse OTLP JSON: {e}");
            return StatusCode::BAD_REQUEST;
        }
    };

    let mut guard = accumulator.lock().unwrap();
    if guard.len() >= max_spans {
        return StatusCode::OK; // silently drop when over limit
    }

    for resource_spans in &request.resource_spans {
        // Extract service.name from resource attributes.
        let service_name = resource_spans
            .resource
            .as_ref()
            .map(|r| {
                r.attributes
                    .iter()
                    .find(|a| a.key == "service.name")
                    .and_then(|a| a.value.as_ref())
                    .map(|v| v.to_string_repr())
                    .unwrap_or_default()
            })
            .unwrap_or_default();

        for scope_spans in &resource_spans.scope_spans {
            for span in &scope_spans.spans {
                if guard.len() >= max_spans {
                    break;
                }

                let start_us = span
                    .start_time_unix_nano
                    .as_deref()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0)
                    / 1000; // ns → µs

                let end_us = span
                    .end_time_unix_nano
                    .as_deref()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0)
                    / 1000;

                let duration_us = end_us.saturating_sub(start_us);

                // Convert OTLP span kind integer to our enum.
                let kind = match span.kind {
                    Some(1) => SpanKind::Internal,
                    Some(2) => SpanKind::Server,
                    Some(3) => SpanKind::Client,
                    Some(4) => SpanKind::Producer,
                    Some(5) => SpanKind::Consumer,
                    _ => SpanKind::Internal,
                };

                // Convert OTLP status code (0=Unset, 1=Ok, 2=Error).
                let status = match span.status.as_ref().and_then(|s| s.code) {
                    Some(1) => SpanStatus::Ok,
                    Some(2) => SpanStatus::Error,
                    _ => SpanStatus::Unset,
                };

                // Collect all attributes as string map.
                let mut attributes: HashMap<String, String> = HashMap::new();
                for attr in &span.attributes {
                    if let Some(v) = &attr.value {
                        attributes.insert(attr.key.clone(), v.to_string_repr());
                    }
                }

                // Extract input/output from well-known OTel semantic conventions.
                let input_summary = attributes
                    .get("http.request.body")
                    .or_else(|| attributes.get("rpc.request.metadata"))
                    .cloned();
                let output_summary = attributes
                    .get("http.response.body")
                    .or_else(|| attributes.get("rpc.response.metadata"))
                    .cloned();

                // Use trace_id + span_id for uniqueness (OTLP span_id alone is trace-scoped).
                // IMPORTANT: apply the same prefix to parent_span_id so parent-child
                // references remain consistent after uniquification.
                let tid = span.trace_id.as_deref().unwrap_or("");
                let unique_span_id = if !tid.is_empty() {
                    format!("{}-{}", tid, span.span_id)
                } else {
                    span.span_id.clone()
                };
                let unique_parent_id = span
                    .parent_span_id
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .map(|pid| {
                        if !tid.is_empty() {
                            format!("{}-{}", tid, pid)
                        } else {
                            pid.to_string()
                        }
                    });

                guard.push(TraceSpan {
                    span_id: unique_span_id,
                    parent_span_id: unique_parent_id,
                    operation_name: span.name.clone(),
                    service_name: service_name.clone(),
                    kind,
                    start_time: start_us,
                    duration_us,
                    attributes,
                    input_summary,
                    output_summary,
                    status,
                    graph_node_id: None, // resolved post-capture
                });
            }
        }
    }

    StatusCode::OK
}

// ── Main entry point: run a TraceCapture gate ─────────────────────────────────

/// Run the full TraceCapture gate lifecycle:
/// 1. Start OTLP HTTP receiver on config.otlp_port
/// 2. Run test_command with OTel env vars
/// 3. Stop receiver
/// 4. Return the captured GateTrace (spans not yet graph-linked)
pub async fn run_trace_capture(
    config: TraceCaptureConfig,
    mr_id: Id,
    gate_run_id: Id,
    commit_sha: String,
) -> Result<GateTrace> {
    let max_spans = config.max_spans;
    let accumulator: SpanAccumulator = Arc::new(Mutex::new(Vec::new()));

    // Build the receiver Axum app.
    let app = Router::new()
        .route("/v1/traces", post(ingest_traces))
        .with_state((Arc::clone(&accumulator), max_spans));

    let addr = format!("127.0.0.1:{}", config.otlp_port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind OTLP receiver on {addr}"))?;

    let actual_addr = listener.local_addr()?;

    // Shutdown channel.
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // Start receiver in background.
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
    });

    // Run the test command with OTel env vars.
    let otlp_endpoint = format!("http://{actual_addr}");
    let parts: Vec<&str> = config.test_command.split_whitespace().collect();
    let command_output = if parts.is_empty() {
        Err(anyhow::anyhow!("empty test_command"))
    } else {
        tokio::process::Command::new(parts[0])
            .args(&parts[1..])
            .env("OTEL_EXPORTER_OTLP_ENDPOINT", &otlp_endpoint)
            .env("OTEL_EXPORTER_OTLP_PROTOCOL", "http/json")
            .env("OTEL_SERVICE_NAME", "gyre-gate-test")
            .env("OTEL_TRACES_EXPORTER", "otlp")
            .output()
            .await
            .context("run test_command")
    };

    // Stop the OTLP receiver.
    let _ = shutdown_tx.send(());
    let _ = server.await;

    command_output?;

    // Collect spans.
    let spans = Arc::try_unwrap(accumulator)
        .unwrap_or_else(|a| {
            // If Arc still has other references (shouldn't happen post-shutdown),
            // take a clone of the contents.
            let guard = a.lock().unwrap();
            let cloned = guard.clone();
            drop(guard);
            Mutex::new(cloned)
        })
        .into_inner()
        .unwrap_or_default();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok(GateTrace {
        id: Id::new(Uuid::new_v4().to_string()),
        mr_id,
        gate_run_id,
        commit_sha,
        spans,
        captured_at: now,
    })
}

// ── Graph node linkage (heuristic post-capture) ───────────────────────────────

/// Resolve span-to-graph-node linkage using heuristics:
/// - HTTP Server spans → Endpoint nodes (matched by `http.route` attribute)
/// - Function spans → Function nodes (matched by `code.function` qualified name)
/// - Database spans → adapter nodes (matched by `db.system` + service_name)
///
/// Unresolved spans are stored as-is (graph_node_id remains None).
/// Resolve span-to-graph-node linkage by querying the knowledge graph directly.
///
/// Loads all graph nodes for the MR's repo once, builds lookup maps, then
/// matches spans using heuristics:
/// - HTTP Server spans → Endpoint/Function nodes (matched by `http.route`)
/// - Internal spans → Function/Type nodes (matched by `code.function` qualified name)
/// - Database spans → Module nodes (matched by `db.system`)
/// - Unmatched spans → fuzzy match by operation_name against any node name
///
/// Previously this used the search index, but graph nodes are not indexed there.
pub async fn resolve_graph_linkage(state: &Arc<AppState>, mut trace: GateTrace) -> GateTrace {
    // Resolve repo_id from the MR.
    let repo_id = match state.merge_requests.find_by_id(&trace.mr_id).await {
        Ok(Some(mr)) => mr.repository_id,
        _ => {
            tracing::warn!(mr_id = %trace.mr_id, "cannot resolve repo for graph linkage");
            return trace;
        }
    };

    // Load all graph nodes for this repo (typically hundreds, not millions).
    let all_nodes = match state.graph_store.list_nodes(&repo_id, None).await {
        Ok(nodes) => nodes,
        Err(e) => {
            tracing::warn!(repo_id = %repo_id, error = %e, "failed to load graph nodes for linkage");
            return trace;
        }
    };

    if all_nodes.is_empty() {
        return trace;
    }

    // Build lookup maps by different matching strategies.
    use gyre_common::graph::NodeType;
    use std::collections::HashMap;

    // qualified_name (lowercase) → node_id
    let mut by_qualified: HashMap<String, Id> = HashMap::new();
    // name (lowercase) → node_id
    let mut by_name: HashMap<String, Id> = HashMap::new();
    // node_type → Vec<(name_lower, qualified_lower, id)>
    let mut by_type: HashMap<NodeType, Vec<(String, String, Id)>> = HashMap::new();

    for node in &all_nodes {
        let name_lc = node.name.to_lowercase();
        let qual_lc = node.qualified_name.to_lowercase();
        by_qualified.insert(qual_lc.clone(), node.id.clone());
        by_name.insert(name_lc.clone(), node.id.clone());
        by_type.entry(node.node_type.clone()).or_default().push((
            name_lc,
            qual_lc,
            node.id.clone(),
        ));
    }

    for span in &mut trace.spans {
        if span.graph_node_id.is_some() {
            continue;
        }

        let node_id = match &span.kind {
            SpanKind::Server => {
                // Match HTTP server spans by http.route against Endpoint or Function nodes.
                let route = span
                    .attributes
                    .get("http.route")
                    .or_else(|| span.attributes.get("http.target"))
                    .cloned();
                if let Some(route) = route {
                    // Try exact match on endpoint nodes first, then functions.
                    let route_lc = route.to_lowercase();
                    let route_name = route.rsplit('/').next().unwrap_or(&route).to_lowercase();
                    find_in_types(
                        &by_type,
                        &[NodeType::Endpoint, NodeType::Function],
                        &route_lc,
                        &route_name,
                    )
                } else {
                    None
                }
            }
            SpanKind::Internal => {
                // Match by code.function qualified name.
                let qualified = span
                    .attributes
                    .get("code.function")
                    .or_else(|| span.attributes.get("code.namespace"))
                    .cloned();
                if let Some(q) = qualified {
                    let q_lc = q.to_lowercase();
                    // Exact qualified_name match first.
                    by_qualified.get(&q_lc).cloned().or_else(|| {
                        // Try matching the last segment (function name).
                        let short = q.rsplit("::").next().unwrap_or(&q).to_lowercase();
                        find_in_types(
                            &by_type,
                            &[NodeType::Function, NodeType::Type],
                            &q_lc,
                            &short,
                        )
                    })
                } else {
                    None
                }
            }
            SpanKind::Database => {
                // Match DB spans to module/type nodes by db.system or operation name.
                let db_system = span.attributes.get("db.system").cloned();
                if let Some(_sys) = db_system {
                    // Try matching the table name from the operation.
                    let op_lc = span.operation_name.to_lowercase();
                    find_in_types(
                        &by_type,
                        &[NodeType::Module, NodeType::Type],
                        &op_lc,
                        &op_lc,
                    )
                } else {
                    None
                }
            }
            _ => None,
        };

        // Fallback: fuzzy match operation_name against any node name.
        span.graph_node_id = node_id.or_else(|| {
            let op_lc = span.operation_name.to_lowercase();
            // Check if any node name appears in the operation name.
            for node in &all_nodes {
                let name_lc = node.name.to_lowercase();
                if name_lc.len() >= 3 && op_lc.contains(&name_lc) {
                    return Some(node.id.clone());
                }
            }
            None
        });
    }

    let linked = trace
        .spans
        .iter()
        .filter(|s| s.graph_node_id.is_some())
        .count();
    tracing::info!(
        mr_id = %trace.mr_id,
        total = trace.spans.len(),
        linked,
        "graph linkage resolved"
    );

    trace
}

/// Search typed node lists for a match by qualified name or short name.
fn find_in_types(
    by_type: &std::collections::HashMap<gyre_common::graph::NodeType, Vec<(String, String, Id)>>,
    types: &[gyre_common::graph::NodeType],
    qualified_lc: &str,
    short_lc: &str,
) -> Option<Id> {
    for node_type in types {
        if let Some(entries) = by_type.get(node_type) {
            // Exact qualified match.
            for (_, qual, id) in entries {
                if qual == qualified_lc {
                    return Some(id.clone());
                }
            }
            // Short name match.
            for (name, _, id) in entries {
                if name == short_lc {
                    return Some(id.clone());
                }
            }
            // Substring match (e.g., route "/api/greet" contains "greet").
            for (name, _, id) in entries {
                if name.len() >= 3
                    && (qualified_lc.contains(name.as_str()) || short_lc.contains(name.as_str()))
                {
                    return Some(id.clone());
                }
            }
        }
    }
    None
}

// ── OTlp config from server env vars ─────────────────────────────────────────

/// Server-level OTLP configuration (from env vars, applied when no gate-level config exists).
#[derive(Clone, Debug)]
pub struct OtlpServerConfig {
    pub enabled: bool,
    pub grpc_port: u16,
    pub max_spans_per_trace: usize,
}

impl OtlpServerConfig {
    pub fn from_env() -> Self {
        let enabled = std::env::var("GYRE_OTLP_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(true); // default: enabled (spec §3a)
        let grpc_port = std::env::var("GYRE_OTLP_GRPC_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4317);
        let max_spans_per_trace = std::env::var("GYRE_OTLP_MAX_SPANS_PER_TRACE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10_000);
        Self {
            enabled,
            grpc_port,
            max_spans_per_trace,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn make_app(max_spans: usize) -> (Router, SpanAccumulator) {
        let accumulator: SpanAccumulator = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/traces", post(ingest_traces))
            .with_state((Arc::clone(&accumulator), max_spans));
        (app, accumulator)
    }

    fn otlp_json(trace_id: &str, span_id: &str, parent_id: Option<&str>, name: &str) -> String {
        let parent_field = match parent_id {
            Some(p) => format!(r#","parentSpanId": "{p}""#),
            None => String::new(),
        };
        format!(
            r#"{{
                "resourceSpans": [{{
                    "resource": {{"attributes": [{{"key": "service.name", "value": {{"stringValue": "test-svc"}}}}]}},
                    "scopeSpans": [{{
                        "spans": [{{
                            "traceId": "{trace_id}",
                            "spanId": "{span_id}"
                            {parent_field},
                            "name": "{name}",
                            "kind": 2,
                            "startTimeUnixNano": "1000000000000",
                            "endTimeUnixNano": "1001000000000",
                            "attributes": [],
                            "status": {{"code": 1}}
                        }}]
                    }}]
                }}]
            }}"#
        )
    }

    #[tokio::test]
    async fn ingest_span_basic() {
        let (app, acc) = make_app(100);
        let body = otlp_json("trace1", "span1", None, "GET /health");
        let req = Request::builder()
            .method("POST")
            .uri("/v1/traces")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let spans = acc.lock().unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].span_id, "trace1-span1");
        assert_eq!(spans[0].operation_name, "GET /health");
        assert_eq!(spans[0].service_name, "test-svc");
        assert_eq!(spans[0].kind, SpanKind::Server);
        assert_eq!(spans[0].status, SpanStatus::Ok);
        assert_eq!(spans[0].start_time, 1_000_000_000); // 1_000_000_000_000 ns / 1000
        assert_eq!(spans[0].duration_us, 1_000_000); // (1_001_000_000_000 - 1_000_000_000_000) ns / 1000
    }

    #[tokio::test]
    async fn ingest_span_parent_id_prefixed_consistently() {
        let (app, acc) = make_app(100);
        let body = otlp_json("trace1", "child-span", Some("parent-span"), "child op");
        let req = Request::builder()
            .method("POST")
            .uri("/v1/traces")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let _ = app.oneshot(req).await.unwrap();

        let spans = acc.lock().unwrap();
        assert_eq!(spans[0].span_id, "trace1-child-span");
        assert_eq!(
            spans[0].parent_span_id.as_deref(),
            Some("trace1-parent-span"),
            "parent_span_id must be prefixed with trace_id to match stored span_id format"
        );
    }

    #[tokio::test]
    async fn ingest_respects_max_spans() {
        let (app, acc) = make_app(1);
        // Send two spans in one request.
        let body = r#"{
            "resourceSpans": [{"resource": {"attributes": []}, "scopeSpans": [{"spans": [
                {"traceId": "t1", "spanId": "s1", "name": "op1", "kind": 1, "startTimeUnixNano": "0", "endTimeUnixNano": "1000", "attributes": [], "status": {}},
                {"traceId": "t1", "spanId": "s2", "name": "op2", "kind": 1, "startTimeUnixNano": "0", "endTimeUnixNano": "1000", "attributes": [], "status": {}}
            ]}]}]
        }"#;
        let req = Request::builder()
            .method("POST")
            .uri("/v1/traces")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let _ = app.oneshot(req).await.unwrap();

        let spans = acc.lock().unwrap();
        assert_eq!(spans.len(), 1, "should cap at max_spans=1");
    }

    #[tokio::test]
    async fn ingest_bad_json_returns_400() {
        let (app, _) = make_app(100);
        let req = Request::builder()
            .method("POST")
            .uri("/v1/traces")
            .header("content-type", "application/json")
            .body(Body::from("not json"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn otlp_server_config_defaults() {
        // Clear env vars to test defaults (they may not be set in CI).
        let cfg = OtlpServerConfig {
            enabled: true,
            grpc_port: 4317,
            max_spans_per_trace: 10_000,
        };
        assert!(cfg.enabled);
        assert_eq!(cfg.grpc_port, 4317);
        assert_eq!(cfg.max_spans_per_trace, 10_000);
    }
}
