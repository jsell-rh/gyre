---
title: "HSI Test-Time Trace Capture Gate"
spec_ref: "human-system-interface.md §3 Test-Time Trace Capture"
depends_on: []
progress: not-started
coverage_sections:
  - "human-system-interface.md §3 Test-Time Trace Capture"
commits: []
---

## Spec Excerpt

**Gate-time OTel instrumentation:** A new gate type (`TraceCapture`) instruments the integration test run with OpenTelemetry. The gate runner starts an OTLP collector that receives spans from the application under test.

**Gyre's internal OTLP receiver** (gRPC, per OpenTelemetry Protocol spec) ingests spans from gate runs. Scoped to gate-time traces only, stored alongside gate results, linked to MR and commit SHA.

**Gate runner lifecycle:**
1. Start OTLP gRPC receiver on `otlp_port`
2. Run `test_command` with OTel env vars injected
3. Collect all received spans
4. Stop the receiver
5. Resolve span-to-graph-node linkage (post-capture)
6. Store the `GateTrace` via `TraceRepository::store`
7. Report gate pass/fail (trace gate always passes — observational)

**REST endpoints:**
- `GET /api/v1/merge-requests/:id/trace` — returns `GateTrace` for an MR
- `GET /api/v1/trace-spans/:span_id/payload` — returns full input/output for a span

## Implementation Plan

The `TraceRepository` port already exists in `crates/gyre-ports/src/trace.rs` and SQLite/Postgres adapters exist in `crates/gyre-adapters/src/sqlite/trace.rs` and `crates/gyre-adapters/src/postgres/trace.rs`. REST endpoints are registered in api/mod.rs. This task focuses on:

1. **Verify `GateTrace` and `TraceSpan` types in `gyre-common`:**
   - Ensure all fields from the spec are present: `mr_id`, `gate_run_id`, `commit_sha`, `spans`, `captured_at`
   - `TraceSpan` fields: `span_id`, `parent_span_id`, `operation_name`, `service_name`, `kind`, `start_time`, `duration_us`, `attributes`, `input_summary`, `output_summary`, `status`, `graph_node_id`
   - Add any missing fields

2. **Verify `TraceRepository` port methods:**
   - `store`, `get_by_mr`, `get_span_payload`, `promote_to_attestation`, `delete_by_mr`
   - Add any missing methods

3. **Implement `TraceCapture` gate type in the gate executor:**
   - Register `TraceCapture` as a gate type in the gate configuration parser
   - Gate config fields: `otlp_port` (default 4317), `test_command`, `max_spans` (default 10000), `capture_external` (default false), `env` (OTel env vars)
   - Gate runner lifecycle: start OTLP receiver → run test command → collect spans → stop receiver → resolve graph linkage → store trace

4. **OTLP gRPC receiver (lightweight):**
   - Implement minimal OTLP gRPC receiver in `gyre-server` (ingestion endpoint)
   - Accept spans from `OTEL_EXPORTER_OTLP_ENDPOINT`
   - Buffer spans during test run, emit as `GateTrace` on completion
   - Configurable via `GYRE_OTLP_ENABLED`, `GYRE_OTLP_GRPC_PORT`, `GYRE_OTLP_MAX_SPANS_PER_TRACE`

5. **Span-to-graph-node linkage (post-capture):**
   - HTTP spans → `Endpoint` nodes (matched by path pattern)
   - Function spans → `Function` nodes (matched by `qualified_name`)
   - DB spans → adapter nodes (matched by module path)
   - Unresolved spans stored with `graph_node_id: None`

6. **Input/output truncation:**
   - `input_summary` and `output_summary` truncated to 4KB each
   - Full payloads stored as zstd-compressed blob (max 1MB per trace)
   - Retrievable via `GET /api/v1/trace-spans/:span_id/payload`

7. **Storage lifecycle:**
   - One trace per open MR (replaces on re-run)
   - `promote_to_attestation` on MR merge
   - `delete_by_mr` on MR close without merge

## Acceptance Criteria

- [ ] `TraceCapture` gate type is parseable from gate configuration YAML
- [ ] Gate runner starts OTLP gRPC receiver before test command
- [ ] Test command receives `OTEL_EXPORTER_OTLP_ENDPOINT` env var
- [ ] Spans collected from OTLP receiver are stored as `GateTrace`
- [ ] Span-to-graph-node linkage resolves HTTP, function, and DB spans
- [ ] Input/output summaries truncated to 4KB; full payloads stored separately
- [ ] `GET /api/v1/merge-requests/:id/trace` returns complete GateTrace JSON
- [ ] `GET /api/v1/trace-spans/:span_id/payload` returns full payloads
- [ ] `TraceCapture` gate always passes (observational, not quality gate)
- [ ] Max spans per trace capped at configured limit
- [ ] Trace replaced on re-run for same MR
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/human-system-interface.md` §3 "Test-Time Trace Capture" for the full design. The `TraceRepository` port exists in `crates/gyre-ports/src/trace.rs`, adapters in `crates/gyre-adapters/src/sqlite/trace.rs`. REST handlers exist in `crates/gyre-server/src/api/traces.rs`. Start by reading these files to understand what's already implemented. The gate executor is in `crates/gyre-server/src/gate_executor.rs`. For the OTLP receiver, consider using the `opentelemetry-proto` crate for protobuf types and `tonic` for gRPC. The receiver should be minimal — accept `ExportTraceServiceRequest`, extract spans, buffer them. Server config env vars (`GYRE_OTLP_ENABLED`, etc.) go in `docs/server-config.md`.
