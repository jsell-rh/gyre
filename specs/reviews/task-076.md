# Review: task-076 — LSP Call Graph — Edge Types, LLM Integration & Performance

**Reviewer:** Independent review (round 8)
**Candidate:** 7ee84f03730499f3ec54bfbb2b99cc9f1d4e4013 (base 5df14f3449d27bbcaf0cc2298f19a3ea99135f0f; production commit 0b3f2317a93d1570882cca8defdfa4cbd24a6d45, later commits docs-only)
**Spec ref:** lsp-call-graph.md §7–9, §12
**Status:** approved

## Verdict

Approved. Every acceptance criterion is backed by real production code verified
by independent probes, including mutation tests that disable the production
behavior and confirm the shipped tests fail. No findings.

## What was verified

- **§7 Edge types.** Pass 1 and Pass 2 emit the same `EdgeType::Calls` variant;
  provenance is the `PASS2_EDGE_METADATA` tag (`gyre-common/src/call_graph.rs`),
  written in `resolve_call_edges` and read by `is_pass2_calls_edge`. Both
  storage adapters round-trip edge `metadata` (SQLite `GraphEdgeRow` column +
  upsert at `sqlite/graph.rs:437/448`; mem upsert at `mem_graph.rs:95`), so
  per-pass counts are real in production storage, not just in tests.
- **§8 graph_summary.** `compute_graph_summary` reports
  `call_graph {call_edges_pass1, call_edges_pass2, call_graph_complete}`.
  Completeness derives from a durable `CallGraphRunRecord` KV marker
  (namespace `call_graph_status`, key = repo id) written only on positive
  toolchain evidence, compared against the newest node `last_seen_at` so an
  in-flight re-extraction reads incomplete. The MCP handler and explorer
  `execute_tool` both pass the real marker-derived status. KV store is wired
  via the `store!` pattern (SQLite/PG in DB mode) — no orphaned in-memory state.
- **§8 graph_query_dryrun.** `dry_run_with_call_graph` warns
  "Call graph incomplete — blast radius may be underreported" for
  calls-dependent queries (focus/concept traversal along `calls`, top-level
  `calls` filter, `$callers`/`$callees`/`$reachable(...,calls)` expressions,
  incoming/outgoing-calls heat/badge metrics) only on known-incomplete status.
  Warning-surface coverage matches the actual dry-run query surface (Focus-node
  computed refs are not resolved by dry-run; groups/callouts are name-based).
- **§8 Explorer prompt.** `build_system_prompt` documents
  `call_graph.call_graph_complete`, the warning, and blast-radius/coupling/
  dead-code guidance; the per-session context block appends a live
  COMPLETE/INCOMPLETE note from the durable marker.
- **§9 Performance metric.** `extract_and_persist_call_graph` returns
  `Pass2Outcome` (persisted count, `extraction_ran`, per-language durations);
  `record_pass2_telemetry` observes `gyre_graph_pass2_duration_seconds`
  (label `language`, buckets 0.5s–120s around the spec targets), logs total
  duration with the targets inline, and persists the marker only on positive
  toolchain evidence. Measurement only, no speculative optimization — per the
  task instructions.
- **§12 Cross-spec references.** Spec-section citations present at each
  integration point; `explorer-implementation.md` `graph_summary` example
  amended to match the shipped response shape.

## Probe evidence

Under `/tmp/stage/review-evidence/` (round-8 files). Highlights:

- `cargo test -p gyre-domain --lib` — 401/401 ok (incl. the three new
  task-076 tests).
- `cargo test -p gyre-server --lib` — 1268/1268 ok (incl.
  `mcp_graph_summary_call_graph_metrics_tool_call`,
  `mcp_graph_query_dryrun_call_graph_warning_tool_call`,
  `pass2_telemetry_marker_gated_on_toolchain_evidence`,
  `test_system_prompt_references_call_graph_availability`).
- `cargo test -p gyre-server --test explorer_ws_integration` — 9/9 ok.
- `cargo test -p gyre-common -p gyre-adapters` — 347+111 ok.
- Mutation A (warning gate disabled in `view_query_resolver.rs`): domain
  warning test AND MCP JSON-RPC round-trip test both FAIL — restored.
- Mutation B (marker written without toolchain evidence): telemetry/marker
  gate test FAILS — restored.
- Mutation C (pass2 provenance filter forced to 0): domain summary test AND
  MCP `graph_summary` round-trip test both FAIL — restored.
  Tree verified clean (`git status` empty) after each restore.
- `scripts/check-arch.sh`, `check-mcp-write-tools.sh`,
  `check-in-memory-state-stores.sh`, `check-inert-enforcement.sh`,
  `check-migration-versions.sh` — all exit 0.
- `cargo fmt --check`: all 11 task-owned `.rs` files clean; the only drift is
  in 7 files untouched between base and candidate (audit.rs,
  merge_processor.rs, etc.) — pre-existing at the assigned base, left for
  verification to adjudicate.

## Notes for verification (not defects)

- Full `cargo test --all`, workspace clippy, and GitHub CI remain
  verification-owned per the assignment.
- A KV-store read error in `call_graph_complete_for_repo` resolves to
  "incomplete", so the MCP dry-run handler warns under store failure —
  conservative over-warning in the fail-safe direction.
- The `call_count` name in `query_uses_calls_edges`'s metric predicate matches
  no current heat/badge metric (resolver metrics are `incoming_calls`/
  `outgoing_calls`); it is a harmless over-inclusion that would only make the
  predicate more inclusive if such a metric were added.
