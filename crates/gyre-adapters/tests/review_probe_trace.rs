
use gyre_adapters::sqlite::SqliteStorage;
use gyre_common::{GateTrace, Id, SpanKind, SpanStatus, TraceSpan};
use gyre_ports::TraceRepository;
use tempfile::NamedTempFile;

#[tokio::test]
async fn review_probe_sqlite_truncated_summary_but_full_payload() {
    let tmp = NamedTempFile::new().unwrap();
    let s = SqliteStorage::new(tmp.path().to_str().unwrap()).unwrap();
    let big = "x".repeat(5000);
    let trace = GateTrace {
        id: Id::new("t-probe"),
        mr_id: Id::new("mr-probe"),
        gate_run_id: Id::new("gr-probe"),
        commit_sha: "0".repeat(40),
        spans: vec![TraceSpan {
            span_id: "span-probe".to_string(),
            parent_span_id: None,
            operation_name: "op".to_string(),
            service_name: "svc".to_string(),
            kind: SpanKind::Server,
            start_time: 1,
            duration_us: 2,
            attributes: Default::default(),
            input_summary: Some(big.clone()),
            output_summary: Some(big.clone()),
            status: SpanStatus::Ok,
            graph_node_id: None,
        }],
        captured_at: 1,
    };
    TraceRepository::store(&s, &trace).await.unwrap();
    let got = TraceRepository::get_by_mr(&s, &Id::new("mr-probe")).await.unwrap().unwrap();
    // Summary column is 4KB-truncated
    assert_eq!(got.spans[0].input_summary.as_deref().map(str::len), Some(4096));
    // Payload blob keeps the full 5000 bytes
    let p = TraceRepository::get_span_payload(&s, &Id::new("gr-probe"), "span-probe").await.unwrap().unwrap();
    assert_eq!(p.input.as_deref().map(<[u8]>::len), Some(5000));
    assert_eq!(p.output.as_deref().map(<[u8]>::len), Some(5000));
}
