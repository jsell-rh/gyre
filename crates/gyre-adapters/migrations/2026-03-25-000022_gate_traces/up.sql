-- Gate-time OTel trace capture (HSI §3a).
-- Traces are stored per-MR, capped at the most recent gate run.
-- Full payloads are zstd-compressed blobs stored in trace_spans.payload_blob.

CREATE TABLE IF NOT EXISTS gate_traces (
    id          TEXT    NOT NULL PRIMARY KEY,
    mr_id       TEXT    NOT NULL,
    gate_run_id TEXT    NOT NULL,
    commit_sha  TEXT    NOT NULL,
    captured_at INTEGER NOT NULL,
    tenant_id   TEXT    NOT NULL,
    -- Permanent flag: set by promote_to_attestation (MR merge path).
    -- Permanent traces are NOT deleted by delete_by_mr.
    permanent   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS trace_spans (
    span_id        TEXT NOT NULL,
    gate_trace_id  TEXT NOT NULL REFERENCES gate_traces(id) ON DELETE CASCADE,
    parent_span_id TEXT,
    operation_name TEXT NOT NULL,
    service_name   TEXT NOT NULL,
    kind           TEXT NOT NULL,
    start_time     INTEGER NOT NULL,
    duration_us    INTEGER NOT NULL,
    attributes     TEXT NOT NULL DEFAULT '{}',  -- JSON object
    input_summary  TEXT,                        -- truncated to 4KB
    output_summary TEXT,                        -- truncated to 4KB
    payload_blob   BYTEA,                       -- zstd-compressed, NULL if no payload
    status         TEXT NOT NULL DEFAULT 'unset',
    graph_node_id  TEXT,
    PRIMARY KEY (span_id, gate_trace_id)
);

CREATE INDEX IF NOT EXISTS idx_gate_traces_mr     ON gate_traces (mr_id);
CREATE INDEX IF NOT EXISTS idx_gate_traces_tenant ON gate_traces (tenant_id);
CREATE INDEX IF NOT EXISTS idx_trace_spans_trace  ON trace_spans (gate_trace_id);
