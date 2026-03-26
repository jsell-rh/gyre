//! SQLite adapter for gate-time trace capture (HSI §3a).
//!
//! Traces are stored per-MR, capped at the most recent gate run.
//! store() replaces any existing (non-permanent) trace for the same MR.
//! Full span payloads are zstd-compressed and stored as BLOBs.

use anyhow::{Context, Result};
use async_trait::async_trait;
use diesel::prelude::*;
use gyre_common::{GateTrace, Id, SpanKind, SpanStatus, TraceSpan};
use gyre_ports::trace::{SpanPayload, TraceRepository};
use std::collections::HashMap;
use std::sync::Arc;

use super::SqliteStorage;
use crate::schema::{gate_traces, trace_spans};

const MAX_PAYLOAD_BYTES: usize = 1024 * 1024; // 1MB
const MAX_SUMMARY_BYTES: usize = 4096; // 4KB

// ── Diesel row types ─────────────────────────────────────────────────────────

#[derive(Queryable, Selectable)]
#[diesel(table_name = gate_traces)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct GateTraceRow {
    id: String,
    mr_id: String,
    gate_run_id: String,
    commit_sha: String,
    captured_at: i64,
    #[allow(dead_code)]
    tenant_id: String,
    #[allow(dead_code)]
    permanent: i32,
}

#[derive(Insertable)]
#[diesel(table_name = gate_traces)]
struct InsertGateTraceRow<'a> {
    id: &'a str,
    mr_id: &'a str,
    gate_run_id: &'a str,
    commit_sha: &'a str,
    captured_at: i64,
    tenant_id: &'a str,
    permanent: i32,
}

#[derive(Queryable, Selectable)]
#[diesel(table_name = trace_spans)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
struct TraceSpanRow {
    span_id: String,
    #[allow(dead_code)]
    gate_trace_id: String,
    parent_span_id: Option<String>,
    operation_name: String,
    service_name: String,
    kind: String,
    start_time: i64,
    duration_us: i64,
    attributes: String,
    input_summary: Option<String>,
    output_summary: Option<String>,
    #[allow(dead_code)]
    payload_blob: Option<Vec<u8>>,
    status: String,
    graph_node_id: Option<String>,
}

#[derive(Insertable)]
#[diesel(table_name = trace_spans)]
struct InsertTraceSpanRow<'a> {
    span_id: &'a str,
    gate_trace_id: &'a str,
    parent_span_id: Option<&'a str>,
    operation_name: &'a str,
    service_name: &'a str,
    kind: &'a str,
    start_time: i64,
    duration_us: i64,
    attributes: String,
    input_summary: Option<&'a str>,
    output_summary: Option<&'a str>,
    payload_blob: Option<Vec<u8>>,
    status: &'a str,
    graph_node_id: Option<&'a str>,
}

// ── Conversion helpers ───────────────────────────────────────────────────────

fn row_to_span(row: TraceSpanRow) -> TraceSpan {
    let attributes: HashMap<String, String> =
        serde_json::from_str(&row.attributes).unwrap_or_default();
    TraceSpan {
        span_id: row.span_id,
        parent_span_id: row.parent_span_id,
        operation_name: row.operation_name,
        service_name: row.service_name,
        kind: SpanKind::parse(&row.kind),
        start_time: row.start_time as u64,
        duration_us: row.duration_us as u64,
        attributes,
        input_summary: row.input_summary,
        output_summary: row.output_summary,
        status: SpanStatus::parse(&row.status),
        graph_node_id: row.graph_node_id.map(Id::new),
    }
}

fn compress_payload(data: &[u8]) -> Result<Vec<u8>> {
    let compressed = zstd::bulk::compress(data, 3)?;
    Ok(compressed)
}

fn decompress_payload(data: &[u8]) -> Result<Vec<u8>> {
    let decompressed = zstd::bulk::decompress(data, MAX_PAYLOAD_BYTES * 2)?;
    Ok(decompressed)
}

fn truncate_summary(s: &str) -> &str {
    if s.len() <= MAX_SUMMARY_BYTES {
        s
    } else {
        // Truncate at char boundary
        let mut end = MAX_SUMMARY_BYTES;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

// ── TraceRepository implementation ──────────────────────────────────────────

#[async_trait]
impl TraceRepository for SqliteStorage {
    async fn store(&self, trace: &GateTrace) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let tenant_id = self.tenant_id.clone();
        let trace = trace.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;

            conn.transaction::<_, anyhow::Error, _>(|conn| {
                // Delete any existing non-permanent trace for this MR.
                diesel::delete(
                    gate_traces::table
                        .filter(gate_traces::mr_id.eq(trace.mr_id.as_str()))
                        .filter(gate_traces::tenant_id.eq(tenant_id.as_str()))
                        .filter(gate_traces::permanent.eq(0)),
                )
                .execute(conn)
                .context("delete old gate trace")?;

                // Insert the new trace header.
                let trace_row = InsertGateTraceRow {
                    id: trace.id.as_str(),
                    mr_id: trace.mr_id.as_str(),
                    gate_run_id: trace.gate_run_id.as_str(),
                    commit_sha: &trace.commit_sha,
                    captured_at: trace.captured_at as i64,
                    tenant_id: &tenant_id,
                    permanent: 0,
                };
                diesel::insert_into(gate_traces::table)
                    .values(&trace_row)
                    .execute(conn)
                    .context("insert gate trace")?;

                // Insert all spans.
                for span in &trace.spans {
                    let attrs_json = serde_json::to_string(&span.attributes)
                        .unwrap_or_else(|_| "{}".to_string());

                    // Build payload blob from input+output if both present.
                    let payload_blob = build_payload_blob(
                        span.input_summary.as_deref(),
                        span.output_summary.as_deref(),
                    );

                    let input_trunc = span.input_summary.as_deref().map(truncate_summary);
                    let output_trunc = span.output_summary.as_deref().map(truncate_summary);

                    let span_row = InsertTraceSpanRow {
                        span_id: &span.span_id,
                        gate_trace_id: trace.id.as_str(),
                        parent_span_id: span.parent_span_id.as_deref(),
                        operation_name: &span.operation_name,
                        service_name: &span.service_name,
                        kind: span.kind.as_str(),
                        start_time: span.start_time as i64,
                        duration_us: span.duration_us as i64,
                        attributes: attrs_json,
                        input_summary: input_trunc,
                        output_summary: output_trunc,
                        payload_blob,
                        status: span.status.as_str(),
                        graph_node_id: span.graph_node_id.as_ref().map(|id| id.as_str()),
                    };
                    diesel::insert_into(trace_spans::table)
                        .values(&span_row)
                        .execute(conn)
                        .context("insert trace span")?;
                }

                Ok(())
            })
        })
        .await?
    }

    async fn get_by_mr(&self, mr_id: &Id) -> Result<Option<GateTrace>> {
        let pool = Arc::clone(&self.pool);
        let mr_id = mr_id.as_str().to_string();
        let tenant_id = self.tenant_id.clone();

        tokio::task::spawn_blocking(move || -> Result<Option<GateTrace>> {
            let mut conn = pool.get().context("get db connection")?;

            // Get the most recent trace for this MR (highest captured_at).
            let trace_row = gate_traces::table
                .filter(gate_traces::mr_id.eq(mr_id.as_str()))
                .filter(gate_traces::tenant_id.eq(tenant_id.as_str()))
                .order(gate_traces::captured_at.desc())
                .first::<GateTraceRow>(&mut *conn)
                .optional()
                .context("query gate trace by mr")?;

            let trace_row = match trace_row {
                Some(r) => r,
                None => return Ok(None),
            };

            // Load all spans for this trace.
            let span_rows = trace_spans::table
                .filter(trace_spans::gate_trace_id.eq(trace_row.id.as_str()))
                .load::<TraceSpanRow>(&mut *conn)
                .context("load trace spans")?;

            let spans = span_rows.into_iter().map(row_to_span).collect();

            Ok(Some(GateTrace {
                id: Id::new(&trace_row.id),
                mr_id: Id::new(&trace_row.mr_id),
                gate_run_id: Id::new(&trace_row.gate_run_id),
                commit_sha: trace_row.commit_sha,
                spans,
                captured_at: trace_row.captured_at as u64,
            }))
        })
        .await?
    }

    async fn get_span_payload(
        &self,
        gate_run_id: &Id,
        span_id: &str,
    ) -> Result<Option<SpanPayload>> {
        let pool = Arc::clone(&self.pool);
        let gate_run_id = gate_run_id.as_str().to_string();
        let span_id = span_id.to_string();
        let tenant_id = self.tenant_id.clone();

        tokio::task::spawn_blocking(move || -> Result<Option<SpanPayload>> {
            let mut conn = pool.get().context("get db connection")?;

            // Find the trace by gate_run_id (with tenant check).
            let trace_id: Option<String> = gate_traces::table
                .filter(gate_traces::gate_run_id.eq(gate_run_id.as_str()))
                .filter(gate_traces::tenant_id.eq(tenant_id.as_str()))
                .select(gate_traces::id)
                .first::<String>(&mut *conn)
                .optional()
                .context("find gate trace by gate_run_id")?;

            let trace_id = match trace_id {
                Some(id) => id,
                None => return Ok(None),
            };

            // Load the payload_blob from the span.
            let blob: Option<Vec<u8>> = trace_spans::table
                .filter(trace_spans::span_id.eq(span_id.as_str()))
                .filter(trace_spans::gate_trace_id.eq(trace_id.as_str()))
                .select(trace_spans::payload_blob)
                .first::<Option<Vec<u8>>>(&mut *conn)
                .optional()
                .context("load span payload blob")?
                .flatten();

            let blob = match blob {
                Some(b) if !b.is_empty() => b,
                _ => return Ok(None),
            };

            // Decompress and split into input/output.
            let decompressed = decompress_payload(&blob).context("decompress span payload")?;
            let payload = decode_payload_blob(&decompressed)?;
            Ok(Some(payload))
        })
        .await?
    }

    async fn promote_to_attestation(&self, mr_id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let mr_id = mr_id.as_str().to_string();
        let tenant_id = self.tenant_id.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            diesel::update(
                gate_traces::table
                    .filter(gate_traces::mr_id.eq(mr_id.as_str()))
                    .filter(gate_traces::tenant_id.eq(tenant_id.as_str())),
            )
            .set(gate_traces::permanent.eq(1))
            .execute(&mut *conn)
            .context("promote trace to attestation")?;
            Ok(())
        })
        .await?
    }

    async fn delete_by_mr(&self, mr_id: &Id) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let mr_id = mr_id.as_str().to_string();
        let tenant_id = self.tenant_id.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut conn = pool.get().context("get db connection")?;
            // Only delete non-permanent traces (permanent ones stay for attestation).
            diesel::delete(
                gate_traces::table
                    .filter(gate_traces::mr_id.eq(mr_id.as_str()))
                    .filter(gate_traces::tenant_id.eq(tenant_id.as_str()))
                    .filter(gate_traces::permanent.eq(0)),
            )
            .execute(&mut *conn)
            .context("delete gate traces for mr")?;
            Ok(())
        })
        .await?
    }
}

// ── Payload blob encoding ────────────────────────────────────────────────────
// Simple format: [4 bytes input_len][input bytes][output bytes]
// If no payload, returns None. Compressed with zstd before storing.

fn build_payload_blob(input: Option<&str>, output: Option<&str>) -> Option<Vec<u8>> {
    if input.is_none() && output.is_none() {
        return None;
    }
    let input_bytes = input.unwrap_or("").as_bytes();
    let output_bytes = output.unwrap_or("").as_bytes();

    // Cap total at MAX_PAYLOAD_BYTES before compression.
    let total = input_bytes.len() + output_bytes.len() + 4;
    if total > MAX_PAYLOAD_BYTES {
        // Truncate output to fit.
        let available = MAX_PAYLOAD_BYTES.saturating_sub(input_bytes.len() + 4);
        let output_bytes = &output_bytes[..available.min(output_bytes.len())];
        let mut buf = Vec::with_capacity(4 + input_bytes.len() + output_bytes.len());
        buf.extend_from_slice(&(input_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(input_bytes);
        buf.extend_from_slice(output_bytes);
        return compress_payload(&buf).ok();
    }

    let mut buf = Vec::with_capacity(4 + input_bytes.len() + output_bytes.len());
    buf.extend_from_slice(&(input_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(input_bytes);
    buf.extend_from_slice(output_bytes);
    compress_payload(&buf).ok()
}

fn decode_payload_blob(data: &[u8]) -> Result<SpanPayload> {
    if data.len() < 4 {
        return Ok(SpanPayload {
            input: None,
            output: None,
        });
    }
    let input_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if 4 + input_len > data.len() {
        return Ok(SpanPayload {
            input: Some(data[4..].to_vec()),
            output: None,
        });
    }
    let input = &data[4..4 + input_len];
    let output = &data[4 + input_len..];

    Ok(SpanPayload {
        input: if input.is_empty() {
            None
        } else {
            Some(input.to_vec())
        },
        output: if output.is_empty() {
            None
        } else {
            Some(output.to_vec())
        },
    })
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn setup() -> (NamedTempFile, SqliteStorage) {
        let tmp = NamedTempFile::new().unwrap();
        let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
        (tmp, s)
    }

    fn make_trace(mr_id: &str, gate_run_id: &str) -> GateTrace {
        GateTrace {
            id: Id::new(uuid::Uuid::new_v4().to_string()),
            mr_id: Id::new(mr_id),
            gate_run_id: Id::new(gate_run_id),
            commit_sha: "abc123".to_string(),
            spans: vec![TraceSpan {
                span_id: "span-1".to_string(),
                parent_span_id: None,
                operation_name: "GET /api/health".to_string(),
                service_name: "gyre-server".to_string(),
                kind: SpanKind::Server,
                start_time: 1_000_000,
                duration_us: 2_000,
                attributes: HashMap::new(),
                input_summary: Some("{}".to_string()),
                output_summary: Some(r#"{"ok":true}"#.to_string()),
                status: SpanStatus::Ok,
                graph_node_id: None,
            }],
            captured_at: 1_700_000_000,
        }
    }

    #[tokio::test]
    async fn store_and_get_by_mr() {
        let (_tmp, s) = setup();
        let trace = make_trace("mr-1", "gate-run-1");
        TraceRepository::store(&s, &trace).await.unwrap();

        let got = TraceRepository::get_by_mr(&s, &Id::new("mr-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.mr_id.as_str(), "mr-1");
        assert_eq!(got.commit_sha, "abc123");
        assert_eq!(got.spans.len(), 1);
        assert_eq!(got.spans[0].operation_name, "GET /api/health");
        assert_eq!(got.spans[0].status, SpanStatus::Ok);
    }

    #[tokio::test]
    async fn store_replaces_existing_trace() {
        let (_tmp, s) = setup();
        let trace1 = make_trace("mr-1", "gate-run-1");
        let mut trace2 = make_trace("mr-1", "gate-run-2");
        trace2.commit_sha = "def456".to_string();

        TraceRepository::store(&s, &trace1).await.unwrap();
        TraceRepository::store(&s, &trace2).await.unwrap();

        let got = TraceRepository::get_by_mr(&s, &Id::new("mr-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.commit_sha, "def456");
    }

    #[tokio::test]
    async fn get_by_mr_missing_returns_none() {
        let (_tmp, s) = setup();
        let got = TraceRepository::get_by_mr(&s, &Id::new("no-such-mr"))
            .await
            .unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn delete_by_mr_removes_trace() {
        let (_tmp, s) = setup();
        let trace = make_trace("mr-1", "gate-run-1");
        TraceRepository::store(&s, &trace).await.unwrap();

        TraceRepository::delete_by_mr(&s, &Id::new("mr-1"))
            .await
            .unwrap();

        let got = TraceRepository::get_by_mr(&s, &Id::new("mr-1"))
            .await
            .unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn promote_to_attestation_preserves_trace() {
        let (_tmp, s) = setup();
        let trace = make_trace("mr-1", "gate-run-1");
        TraceRepository::store(&s, &trace).await.unwrap();

        TraceRepository::promote_to_attestation(&s, &Id::new("mr-1"))
            .await
            .unwrap();

        // delete_by_mr should not remove permanent traces.
        TraceRepository::delete_by_mr(&s, &Id::new("mr-1"))
            .await
            .unwrap();

        let got = TraceRepository::get_by_mr(&s, &Id::new("mr-1"))
            .await
            .unwrap();
        assert!(got.is_some(), "promoted trace should survive delete_by_mr");
    }

    #[tokio::test]
    async fn get_span_payload_roundtrip() {
        let (_tmp, s) = setup();
        let trace = make_trace("mr-1", "gate-run-1");
        TraceRepository::store(&s, &trace).await.unwrap();

        let payload = TraceRepository::get_span_payload(&s, &Id::new("gate-run-1"), "span-1")
            .await
            .unwrap();

        let payload = payload.expect("should have payload");
        let input = payload.input.map(|b| String::from_utf8(b).unwrap());
        let output = payload.output.map(|b| String::from_utf8(b).unwrap());
        assert_eq!(input.as_deref(), Some("{}"));
        assert_eq!(output.as_deref(), Some(r#"{"ok":true}"#));
    }

    #[tokio::test]
    async fn span_with_no_payload_returns_none() {
        let (_tmp, s) = setup();
        let mut trace = make_trace("mr-1", "gate-run-1");
        trace.spans[0].input_summary = None;
        trace.spans[0].output_summary = None;

        TraceRepository::store(&s, &trace).await.unwrap();

        let payload = TraceRepository::get_span_payload(&s, &Id::new("gate-run-1"), "span-1")
            .await
            .unwrap();
        assert!(payload.is_none());
    }
}
