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
    /// Wire/storage representation. PascalCase per the HSI §3a JSON example
    /// (`"kind": "Server"`); `parse` accepts the legacy lowercase rows too.
    pub fn as_str(&self) -> &'static str {
        match self {
            SpanKind::Server => "Server",
            SpanKind::Client => "Client",
            SpanKind::Internal => "Internal",
            SpanKind::Database => "Database",
            SpanKind::Producer => "Producer",
            SpanKind::Consumer => "Consumer",
        }
    }

    pub fn parse(s: &str) -> Self {
        if s.eq_ignore_ascii_case("server") {
            SpanKind::Server
        } else if s.eq_ignore_ascii_case("client") {
            SpanKind::Client
        } else if s.eq_ignore_ascii_case("database") {
            SpanKind::Database
        } else if s.eq_ignore_ascii_case("producer") {
            SpanKind::Producer
        } else if s.eq_ignore_ascii_case("consumer") {
            SpanKind::Consumer
        } else {
            SpanKind::Internal
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
    /// Wire/storage representation. PascalCase per the HSI §3a JSON example
    /// (`"status": "Ok"`); `parse` accepts the legacy lowercase rows too.
    pub fn as_str(&self) -> &'static str {
        match self {
            SpanStatus::Ok => "Ok",
            SpanStatus::Error => "Error",
            SpanStatus::Unset => "Unset",
        }
    }

    pub fn parse(s: &str) -> Self {
        if s.eq_ignore_ascii_case("ok") {
            SpanStatus::Ok
        } else if s.eq_ignore_ascii_case("error") {
            SpanStatus::Error
        } else {
            SpanStatus::Unset
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_kind_as_str_matches_spec_casing() {
        // HSI §3a JSON example: "kind": "Server".
        assert_eq!(SpanKind::Server.as_str(), "Server");
        assert_eq!(SpanKind::Client.as_str(), "Client");
        assert_eq!(SpanKind::Internal.as_str(), "Internal");
        assert_eq!(SpanKind::Database.as_str(), "Database");
        assert_eq!(SpanKind::Producer.as_str(), "Producer");
        assert_eq!(SpanKind::Consumer.as_str(), "Consumer");
    }

    #[test]
    fn span_status_as_str_matches_spec_casing() {
        // HSI §3a JSON example: "status": "Ok".
        assert_eq!(SpanStatus::Ok.as_str(), "Ok");
        assert_eq!(SpanStatus::Error.as_str(), "Error");
        assert_eq!(SpanStatus::Unset.as_str(), "Unset");
    }

    #[test]
    fn span_kind_status_round_trip() {
        for kind in [
            SpanKind::Server,
            SpanKind::Client,
            SpanKind::Internal,
            SpanKind::Database,
            SpanKind::Producer,
            SpanKind::Consumer,
        ] {
            assert_eq!(SpanKind::parse(kind.as_str()), kind);
        }
        for status in [SpanStatus::Ok, SpanStatus::Error, SpanStatus::Unset] {
            assert_eq!(SpanStatus::parse(status.as_str()), status);
        }
    }

    #[test]
    fn span_kind_status_parse_accepts_legacy_lowercase_rows() {
        // Rows written before the casing fix store lowercase values.
        assert_eq!(SpanKind::parse("server"), SpanKind::Server);
        assert_eq!(SpanKind::parse("client"), SpanKind::Client);
        assert_eq!(SpanKind::parse("internal"), SpanKind::Internal);
        assert_eq!(SpanKind::parse("database"), SpanKind::Database);
        assert_eq!(SpanKind::parse("producer"), SpanKind::Producer);
        assert_eq!(SpanKind::parse("consumer"), SpanKind::Consumer);
        assert_eq!(SpanStatus::parse("ok"), SpanStatus::Ok);
        assert_eq!(SpanStatus::parse("error"), SpanStatus::Error);
        assert_eq!(SpanStatus::parse("unset"), SpanStatus::Unset);
        // Unknown values keep the historical defaults.
        assert_eq!(SpanKind::parse("nonsense"), SpanKind::Internal);
        assert_eq!(SpanStatus::parse("nonsense"), SpanStatus::Unset);
    }
}
