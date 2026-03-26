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
                let unique_span_id = if let Some(tid) = &span.trace_id {
                    format!("{}-{}", tid, span.span_id)
                } else {
                    span.span_id.clone()
                };

                guard.push(TraceSpan {
                    span_id: unique_span_id,
                    parent_span_id: span
                        .parent_span_id
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
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
pub async fn resolve_graph_linkage(state: &Arc<AppState>, mut trace: GateTrace) -> GateTrace {
    for span in &mut trace.spans {
        if span.graph_node_id.is_some() {
            continue;
        }

        let node_id = match &span.kind {
            SpanKind::Server => {
                // Match HTTP server spans to Endpoint nodes by http.route.
                let route = span
                    .attributes
                    .get("http.route")
                    .or_else(|| span.attributes.get("http.target"))
                    .cloned();
                if let Some(route) = route {
                    find_endpoint_node(state, &route).await
                } else {
                    None
                }
            }
            SpanKind::Internal => {
                // Match internal spans to Function nodes by code.function.
                let qualified = span
                    .attributes
                    .get("code.function")
                    .or_else(|| span.attributes.get("code.namespace"))
                    .cloned();
                if let Some(q) = qualified {
                    find_function_node(state, &q).await
                } else {
                    None
                }
            }
            SpanKind::Database => {
                // Match DB spans to adapter nodes by db.system + service_name.
                let db_system = span.attributes.get("db.system").cloned();
                if let Some(sys) = db_system {
                    find_adapter_node(state, &sys, &span.service_name).await
                } else {
                    None
                }
            }
            _ => None,
        };

        span.graph_node_id = node_id;
    }

    trace
}

async fn find_endpoint_node(state: &Arc<AppState>, route: &str) -> Option<Id> {
    let query = gyre_ports::search::SearchQuery {
        query: route.to_string(),
        entity_type: Some("Endpoint".to_string()),
        workspace_id: None,
        limit: 1,
    };
    match state.search.search(query).await {
        Ok(results) => results.into_iter().next().map(|r| Id::new(r.entity_id)),
        Err(_) => None,
    }
}

async fn find_function_node(state: &Arc<AppState>, qualified_name: &str) -> Option<Id> {
    let query = gyre_ports::search::SearchQuery {
        query: qualified_name.to_string(),
        entity_type: Some("Function".to_string()),
        workspace_id: None,
        limit: 1,
    };
    match state.search.search(query).await {
        Ok(results) => results.into_iter().next().map(|r| Id::new(r.entity_id)),
        Err(_) => None,
    }
}

async fn find_adapter_node(
    state: &Arc<AppState>,
    db_system: &str,
    service_name: &str,
) -> Option<Id> {
    let query = gyre_ports::search::SearchQuery {
        query: format!("{db_system} {service_name}"),
        entity_type: Some("Module".to_string()),
        workspace_id: None,
        limit: 1,
    };
    match state.search.search(query).await {
        Ok(results) => results.into_iter().next().map(|r| Id::new(r.entity_id)),
        Err(_) => None,
    }
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
            .unwrap_or(false);
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
