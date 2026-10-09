//! Lightweight OTLP gRPC receiver for gate-time trace capture (HSI §3a).
//!
//! This is NOT a general-purpose observability backend. It is scoped to
//! gate-time traces only — started per gate run, stopped after test execution.
//!
//! Protocol: OTLP/gRPC (per the OpenTelemetry Protocol specification). The
//! receiver implements the `TraceService` gRPC service and accepts
//! `ExportTraceServiceRequest` messages. The application under test is started
//! with:
//!   OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:<port>
//!   OTEL_EXPORTER_OTLP_PROTOCOL=grpc
//!   OTEL_SERVICE_NAME=<service>

use anyhow::{Context, Result};
use gyre_common::{GateTrace, Id, SpanKind, SpanStatus, TraceSpan};
use opentelemetry_proto::tonic::collector::trace::v1::{
    trace_service_server::{TraceService, TraceServiceServer},
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use opentelemetry_proto::tonic::common::v1::{any_value::Value as OtlpValue, AnyValue, KeyValue};
use parking_lot::Mutex;
use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use uuid::Uuid;

use crate::AppState;

// ── Config ───────────────────────────────────────────────────────────────────

/// Configuration for a TraceCapture gate (parsed from gate.command JSON).
#[derive(Debug, Clone, Deserialize)]
pub struct TraceCaptureConfig {
    /// Port to start the OTLP gRPC receiver on. When omitted, the server-level
    /// `GYRE_OTLP_GRPC_PORT` (default 4317) is used.
    #[serde(default)]
    pub otlp_port: Option<u16>,
    /// Test command to run with OTel env vars injected.
    #[serde(default = "default_test_command")]
    pub test_command: String,
    /// Maximum spans per trace (prevents unbounded storage from fuzz tests).
    #[serde(default = "default_max_spans")]
    pub max_spans: usize,
    #[serde(default)]
    pub capture_external: bool,
    /// Extra env vars injected into the test command (OTel or otherwise).
    /// Values support `{{repo_name}}` templating, resolved by the gate
    /// executor before the capture starts. Merged over the receiver's
    /// built-in OTel defaults, so an explicit user value (e.g. a custom
    /// `OTEL_SERVICE_NAME`) wins.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Default gRPC OTLP port (per OTLP spec).
pub const DEFAULT_OTLP_GRPC_PORT: u16 = 4317;

fn default_test_command() -> String {
    "cargo test --features integration".to_string()
}

fn default_max_spans() -> usize {
    10_000
}

impl Default for TraceCaptureConfig {
    fn default() -> Self {
        Self {
            otlp_port: None,
            test_command: default_test_command(),
            max_spans: default_max_spans(),
            capture_external: false,
            env: HashMap::new(),
        }
    }
}

// ── gRPC receiver service ─────────────────────────────────────────────────────

type SpanAccumulator = Arc<Mutex<Vec<TraceSpan>>>;

/// gRPC `TraceService` implementation that buffers received spans in memory.
struct TraceCollector {
    accumulator: SpanAccumulator,
    max_spans: usize,
}

#[tonic::async_trait]
impl TraceService for TraceCollector {
    async fn export(
        &self,
        request: tonic::Request<ExportTraceServiceRequest>,
    ) -> std::result::Result<tonic::Response<ExportTraceServiceResponse>, tonic::Status> {
        let req = request.into_inner();
        {
            let mut guard = self.accumulator.lock();
            ingest_request(&req, &mut guard, self.max_spans);
        }
        Ok(tonic::Response::new(ExportTraceServiceResponse {
            partial_success: None,
        }))
    }
}

/// Extract a string attribute value by key from an OTLP `KeyValue` list.
fn attr_string(attrs: &[KeyValue], key: &str) -> Option<String> {
    attrs
        .iter()
        .find(|kv| kv.key == key)
        .and_then(|kv| kv.value.as_ref())
        .map(anyvalue_to_string)
}

/// Render an OTLP `AnyValue` as a display string.
fn anyvalue_to_string(v: &AnyValue) -> String {
    match &v.value {
        Some(OtlpValue::StringValue(s)) => s.clone(),
        Some(OtlpValue::IntValue(i)) => i.to_string(),
        Some(OtlpValue::BoolValue(b)) => b.to_string(),
        Some(OtlpValue::DoubleValue(d)) => d.to_string(),
        Some(OtlpValue::BytesValue(b)) => hex::encode(b),
        // Array/kvlist values are not used by our span heuristics; skip.
        _ => String::new(),
    }
}

/// Convert all spans in an `ExportTraceServiceRequest` into `TraceSpan`s,
/// appending to `guard` while respecting `max_spans`.
fn ingest_request(req: &ExportTraceServiceRequest, guard: &mut Vec<TraceSpan>, max_spans: usize) {
    for resource_spans in &req.resource_spans {
        let service_name = resource_spans
            .resource
            .as_ref()
            .and_then(|r| attr_string(&r.attributes, "service.name"))
            .unwrap_or_default();

        for scope_spans in &resource_spans.scope_spans {
            for span in &scope_spans.spans {
                if guard.len() >= max_spans {
                    return;
                }

                let start_us = span.start_time_unix_nano / 1000; // ns → µs
                let end_us = span.end_time_unix_nano / 1000;
                let duration_us = end_us.saturating_sub(start_us);

                // OTLP SpanKind: 1=Internal, 2=Server, 3=Client, 4=Producer, 5=Consumer.
                let kind = match span.kind {
                    2 => SpanKind::Server,
                    3 => SpanKind::Client,
                    4 => SpanKind::Producer,
                    5 => SpanKind::Consumer,
                    _ => SpanKind::Internal,
                };

                // OTLP StatusCode: 0=Unset, 1=Ok, 2=Error.
                let status = match span.status.as_ref().map(|s| s.code) {
                    Some(1) => SpanStatus::Ok,
                    Some(2) => SpanStatus::Error,
                    _ => SpanStatus::Unset,
                };

                // Collect attributes as a string map.
                let mut attributes: HashMap<String, String> = HashMap::new();
                for attr in &span.attributes {
                    if let Some(v) = &attr.value {
                        attributes.insert(attr.key.clone(), anyvalue_to_string(v));
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

                // trace_id/span_id are raw bytes in OTLP/gRPC. Hex-encode and
                // combine so span_ids are globally unique across traces.
                let trace_hex = hex::encode(&span.trace_id);
                let span_hex = hex::encode(&span.span_id);
                let unique_span_id = if trace_hex.is_empty() {
                    span_hex
                } else {
                    format!("{trace_hex}-{span_hex}")
                };
                // Apply the same prefix to parent_span_id so parent-child
                // references remain consistent after uniquification.
                let unique_parent_id = if span.parent_span_id.is_empty() {
                    None
                } else {
                    let parent_hex = hex::encode(&span.parent_span_id);
                    Some(if trace_hex.is_empty() {
                        parent_hex
                    } else {
                        format!("{trace_hex}-{parent_hex}")
                    })
                };

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
}

// ── Receiver lifecycle ────────────────────────────────────────────────────────

/// Bind and start the OTLP gRPC receiver, returning the bound address, a
/// shutdown sender, and the server task handle. The socket is bound before
/// this returns, so the receiver is guaranteed ready before the caller runs
/// the test command (HSI §3a lifecycle step 1 precedes step 2).
async fn spawn_receiver(
    port: u16,
    accumulator: SpanAccumulator,
    max_spans: usize,
) -> Result<( // tuple-carrier:ok -- pre-existing 3-field receiver lifecycle handle (addr, shutdown tx, task join); unchanged by task-198, line-shifted into scan range
    SocketAddr,
    oneshot::Sender<()>,
    JoinHandle<std::result::Result<(), tonic::transport::Error>>,
)> {
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind OTLP gRPC receiver on {addr}"))?;
    let actual_addr = listener.local_addr()?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let collector = TraceCollector {
        accumulator,
        max_spans,
    };
    let incoming = TcpListenerStream::new(listener);

    let handle = tokio::spawn(async move {
        Server::builder()
            .add_service(TraceServiceServer::new(collector))
            .serve_with_incoming_shutdown(incoming, async {
                let _ = shutdown_rx.await;
            })
            .await
    });

    Ok((actual_addr, shutdown_tx, handle))
}

/// Run the full TraceCapture gate lifecycle:
/// 1. Start OTLP gRPC receiver on `port`
/// 2. Run `test_command` with OTel env vars
/// 3. Stop receiver
/// 4. Return the captured GateTrace (spans not yet graph-linked)
pub async fn run_trace_capture(
    config: TraceCaptureConfig,
    port: u16,
    mr_id: Id,
    gate_run_id: Id,
    commit_sha: String,
) -> Result<GateTrace> {
    let accumulator: SpanAccumulator = Arc::new(Mutex::new(Vec::new()));
    let (actual_addr, shutdown_tx, server) =
        spawn_receiver(port, Arc::clone(&accumulator), config.max_spans).await?;

    // Run the test command with OTel env vars pointing at our gRPC receiver.
    let otlp_endpoint = format!("http://{actual_addr}");
    let parts: Vec<&str> = config.test_command.split_whitespace().collect();
    let command_output = if parts.is_empty() {
        Err(anyhow::anyhow!("empty test_command"))
    } else {
        let mut cmd = tokio::process::Command::new(parts[0]);
        cmd.args(&parts[1..])
            .env("OTEL_EXPORTER_OTLP_ENDPOINT", &otlp_endpoint)
            .env("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc")
            .env("OTEL_SERVICE_NAME", "gyre-gate-test")
            .env("OTEL_TRACES_EXPORTER", "otlp")
            .env(
                "GYRE_CAPTURE_EXTERNAL",
                if config.capture_external {
                    "true"
                } else {
                    "false"
                },
            );
        // User-supplied env (gate config `env` map) wins over the defaults
        // above — an explicit OTEL_SERVICE_NAME or endpoint override takes
        // precedence (HSI §3a gate config).
        cmd.envs(&config.env);
        cmd.output().await.context("run test_command")
    };

    // Stop the OTLP receiver.
    let _ = shutdown_tx.send(());
    let _ = server.await;

    command_output?;

    // Collect spans.
    let spans = Arc::try_unwrap(accumulator)
        .unwrap_or_else(|a| {
            // If the Arc still has other references (shouldn't happen after
            // the server task has been awaited), clone the contents.
            let cloned = a.lock().clone();
            Mutex::new(cloned)
        })
        .into_inner();

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

/// Resolve span-to-graph-node linkage by querying the knowledge graph directly.
///
/// Loads all graph nodes for the MR's repo once, builds lookup maps, then
/// matches spans using heuristics:
/// - HTTP Server spans → Endpoint/Function nodes (matched by `http.route`)
/// - Internal spans → Function/Type nodes (matched by `code.function` qualified name)
/// - Database spans → Module nodes (matched by `db.system`)
/// - Unmatched spans → fuzzy match by operation_name against any node name
///
/// Unresolved spans are stored as-is (graph_node_id remains None).
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
                if db_system.is_some() {
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

// ── OTLP config from server env vars ─────────────────────────────────────────

/// Server-level OTLP configuration (from env vars, HSI §3a).
#[derive(Clone, Debug)]
pub struct OtlpServerConfig {
    /// Whether the OTLP receiver is enabled at all (GYRE_OTLP_ENABLED).
    pub enabled: bool,
    /// Default gRPC port used when a gate does not specify `otlp_port`.
    pub grpc_port: u16,
    /// Safety cap on spans per trace (GYRE_OTLP_MAX_SPANS_PER_TRACE).
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
            .unwrap_or(DEFAULT_OTLP_GRPC_PORT);
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
    use opentelemetry_proto::tonic::collector::trace::v1::trace_service_client::TraceServiceClient;
    use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue};
    use opentelemetry_proto::tonic::resource::v1::Resource;
    use opentelemetry_proto::tonic::trace::v1::{
        ResourceSpans, ScopeSpans, Span as OtlpSpan, Status as OtlpProtoStatus,
    };

    fn string_attr(key: &str, value: &str) -> KeyValue {
        KeyValue {
            key: key.to_string(),
            value: Some(AnyValue {
                value: Some(OtlpValue::StringValue(value.to_string())),
            }),
        }
    }

    fn make_request(service: &str, spans: Vec<OtlpSpan>) -> ExportTraceServiceRequest {
        ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource: Some(Resource {
                    attributes: vec![string_attr("service.name", service)],
                    ..Default::default()
                }),
                scope_spans: vec![ScopeSpans {
                    spans,
                    ..Default::default()
                }],
                ..Default::default()
            }],
        }
    }

    fn make_span(
        trace_id: &[u8],
        span_id: &[u8],
        parent: Option<&[u8]>,
        name: &str,
        kind: i32,
        start_ns: u64,
        end_ns: u64,
        status_code: i32,
    ) -> OtlpSpan {
        OtlpSpan {
            trace_id: trace_id.to_vec(),
            span_id: span_id.to_vec(),
            parent_span_id: parent.map(|p| p.to_vec()).unwrap_or_default(),
            name: name.to_string(),
            kind,
            start_time_unix_nano: start_ns,
            end_time_unix_nano: end_ns,
            status: Some(OtlpProtoStatus {
                code: status_code,
                message: String::new(),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn ingest_converts_span_fields() {
        let req = make_request(
            "test-svc",
            vec![make_span(
                &[0x01, 0x02],
                &[0xaa, 0xbb],
                None,
                "GET /health",
                2, // Server
                1_000_000_000_000,
                1_001_000_000_000,
                1, // Ok
            )],
        );
        let mut spans = Vec::new();
        ingest_request(&req, &mut spans, 100);

        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].span_id, "0102-aabb");
        assert_eq!(spans[0].operation_name, "GET /health");
        assert_eq!(spans[0].service_name, "test-svc");
        assert_eq!(spans[0].kind, SpanKind::Server);
        assert_eq!(spans[0].status, SpanStatus::Ok);
        assert_eq!(spans[0].start_time, 1_000_000_000); // ns / 1000
        assert_eq!(spans[0].duration_us, 1_000_000);
    }

    #[test]
    fn ingest_prefixes_parent_span_id_consistently() {
        let req = make_request(
            "svc",
            vec![make_span(
                &[0x01],
                &[0xcc],
                Some(&[0xdd]),
                "child op",
                1,
                0,
                1000,
                0,
            )],
        );
        let mut spans = Vec::new();
        ingest_request(&req, &mut spans, 100);

        assert_eq!(spans[0].span_id, "01-cc");
        assert_eq!(
            spans[0].parent_span_id.as_deref(),
            Some("01-dd"),
            "parent_span_id must be prefixed with trace_id to match stored span_id format"
        );
    }

    #[test]
    fn ingest_respects_max_spans() {
        let req = make_request(
            "svc",
            vec![
                make_span(&[0x01], &[0x01], None, "op1", 1, 0, 1000, 0),
                make_span(&[0x01], &[0x02], None, "op2", 1, 0, 1000, 0),
            ],
        );
        let mut spans = Vec::new();
        ingest_request(&req, &mut spans, 1);
        assert_eq!(spans.len(), 1, "should cap at max_spans=1");
    }

    #[test]
    fn ingest_maps_kind_and_status() {
        let req = make_request(
            "svc",
            vec![
                make_span(&[0x01], &[0x01], None, "internal", 1, 0, 1, 0),
                make_span(&[0x01], &[0x02], None, "client", 3, 0, 1, 2),
            ],
        );
        let mut spans = Vec::new();
        ingest_request(&req, &mut spans, 100);
        assert_eq!(spans[0].kind, SpanKind::Internal);
        assert_eq!(spans[0].status, SpanStatus::Unset);
        assert_eq!(spans[1].kind, SpanKind::Client);
        assert_eq!(spans[1].status, SpanStatus::Error);
    }

    /// End-to-end gRPC round-trip: start the receiver, send an
    /// ExportTraceServiceRequest via a real tonic client, verify accumulation.
    #[tokio::test]
    async fn grpc_receiver_accepts_export() {
        let accumulator: SpanAccumulator = Arc::new(Mutex::new(Vec::new()));
        let (addr, shutdown_tx, server) = spawn_receiver(0, Arc::clone(&accumulator), 100)
            .await
            .unwrap();

        let mut client = TraceServiceClient::connect(format!("http://{addr}"))
            .await
            .expect("connect to gRPC receiver");
        let req = make_request(
            "payment-api",
            vec![make_span(
                &[0x0a],
                &[0x0b],
                None,
                "POST /payments/retry",
                2,
                0,
                300_000_000,
                1,
            )],
        );
        client.export(req).await.expect("export spans");

        let _ = shutdown_tx.send(());
        let _ = server.await;

        let spans = accumulator.lock();
        assert_eq!(spans.len(), 1, "receiver should have accumulated one span");
        assert_eq!(spans[0].operation_name, "POST /payments/retry");
        assert_eq!(spans[0].service_name, "payment-api");
        assert_eq!(spans[0].kind, SpanKind::Server);
    }

    #[test]
    fn otlp_server_config_defaults() {
        let cfg = OtlpServerConfig {
            enabled: true,
            grpc_port: DEFAULT_OTLP_GRPC_PORT,
            max_spans_per_trace: 10_000,
        };
        assert!(cfg.enabled);
        assert_eq!(cfg.grpc_port, 4317);
        assert_eq!(cfg.max_spans_per_trace, 10_000);
    }
    /// HSI §3a acceptance: the spec's literal gate-config YAML block must be
    /// parseable into `TraceCaptureConfig` (camelCase gate fields, snake_case
    /// config fields, env map with `{{repo_name}}` templating left raw here —
    /// the gate executor resolves it).
    #[test]
    fn trace_capture_config_parses_spec_yaml_block() {
        let yaml = r#"
otlp_port: 4317
test_command: "cargo test --features integration"
max_spans: 10000
capture_external: false
env:
  OTEL_EXPORTER_OTLP_ENDPOINT: "http://localhost:4317"
  OTEL_SERVICE_NAME: "{{repo_name}}"
"#;
        let cfg: TraceCaptureConfig = serde_yaml::from_str(yaml).expect("parse spec YAML block");
        assert_eq!(cfg.otlp_port, Some(4317));
        assert_eq!(cfg.test_command, "cargo test --features integration");
        assert_eq!(cfg.max_spans, 10_000);
        assert!(!cfg.capture_external);
        assert_eq!(
            cfg.env.get("OTEL_SERVICE_NAME").map(String::as_str),
            Some("{{repo_name}}")
        );
        assert_eq!(
            cfg.env
                .get("OTEL_EXPORTER_OTLP_ENDPOINT")
                .map(String::as_str),
            Some("http://localhost:4317")
        );
    }

    /// Defaults: no `env` key in config means an empty map (serde default).
    #[test]
    fn trace_capture_config_env_defaults_empty() {
        let cfg: TraceCaptureConfig =
            serde_yaml::from_str("test_command: true").expect("parse minimal config");
        assert!(cfg.env.is_empty());
    }

    /// HSI §3a: the test command must receive env vars from the gate's `env`
    /// map, overriding the built-in OTel defaults. Observable via a command
    /// that writes `$OTEL_SERVICE_NAME` to a temp file.
    #[tokio::test]
    async fn run_trace_capture_injects_env_map_into_test_command() {
        // test_command is exec'd without a shell (split on whitespace), so
        // use a script file to observe the child process env.
        let dir = tempfile::tempdir().unwrap();
        let out_path = dir.path().join("service_name.txt");
        let script_path = dir.path().join("dump_env.sh");
        std::fs::write(
            &script_path,
            format!(
                "#!/bin/sh\nprintf '%s' \"$OTEL_SERVICE_NAME\" > {}\n",
                out_path.display()
            ),
        )
        .unwrap();

        let mut env = HashMap::new();
        env.insert("OTEL_SERVICE_NAME".to_string(), "my-repo-tests".to_string());
        let config = TraceCaptureConfig {
            test_command: format!("sh {}", script_path.display()),
            env,
            otlp_port: Some(0),
            ..Default::default()
        };

        let trace = run_trace_capture(config, 0, Id::new("mr-1"), Id::new("gr-1"), "sha".into())
            .await
            .expect("capture should succeed");
        assert!(trace.spans.is_empty());

        let observed =
            std::fs::read_to_string(&out_path).expect("test command should have written env value");
        assert_eq!(
            observed, "my-repo-tests",
            "user env must override the default OTEL_SERVICE_NAME"
        );
    }
}
