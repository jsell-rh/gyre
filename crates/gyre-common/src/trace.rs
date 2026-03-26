//! Gate-time trace capture types (HSI §3a).
//!
//! These types represent OpenTelemetry spans captured during integration test
//! gate execution and mapped to the knowledge graph.

use crate::Id;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The kind of span (operation type).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// Inbound HTTP request (server-side).
    Server,
    /// Outbound HTTP request (client-side).
    Client,
    /// Internal function call.
    Internal,
    /// Database query or operation.
    Database,
    /// Message producer (e.g., queue publish).
    Producer,
    /// Message consumer (e.g., queue subscribe).
    Consumer,
}

impl SpanKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpanKind::Server => "server",
            SpanKind::Client => "client",
            SpanKind::Internal => "internal",
            SpanKind::Database => "database",
            SpanKind::Producer => "producer",
            SpanKind::Consumer => "consumer",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "server" => SpanKind::Server,
            "client" => SpanKind::Client,
            "database" => SpanKind::Database,
            "producer" => SpanKind::Producer,
            "consumer" => SpanKind::Consumer,
            _ => SpanKind::Internal,
        }
    }
}

/// Status of a span (did the operation succeed?).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanStatus {
    /// Operation completed successfully.
    Ok,
    /// Operation encountered an error.
    Error,
    /// Status not explicitly set.
    Unset,
}

impl SpanStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpanStatus::Ok => "ok",
            SpanStatus::Error => "error",
            SpanStatus::Unset => "unset",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "ok" => SpanStatus::Ok,
            "error" => SpanStatus::Error,
            _ => SpanStatus::Unset,
        }
    }
}

/// A single OpenTelemetry span captured during gate execution.
///
/// `input_summary` and `output_summary` are truncated to 4KB.
/// Full payloads are stored separately (see `SpanPayload` in gyre-ports).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSpan {
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub operation_name: String,
    pub service_name: String,
    pub kind: SpanKind,
    /// Epoch microseconds.
    pub start_time: u64,
    /// Duration in microseconds.
    pub duration_us: u64,
    pub attributes: HashMap<String, String>,
    /// Truncated to 4KB (request body / call input).
    pub input_summary: Option<String>,
    /// Truncated to 4KB (response body / call output).
    pub output_summary: Option<String>,
    pub status: SpanStatus,
    /// Resolved post-capture via heuristic graph-node linkage. None if unresolved.
    pub graph_node_id: Option<Id>,
}

/// A complete gate-time trace: all OTel spans captured during a single gate run.
///
/// Stored per-MR, capped at the most recent gate run. Old traces are evicted
/// when the MR merges (the merged trace is preserved on the MergeAttestation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateTrace {
    /// Stable identifier for this trace record (DB primary key).
    pub id: Id,
    pub mr_id: Id,
    pub gate_run_id: Id,
    pub commit_sha: String,
    pub spans: Vec<TraceSpan>,
    /// Epoch seconds.
    pub captured_at: u64,
}
