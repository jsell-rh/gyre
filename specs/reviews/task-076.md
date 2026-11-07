# Review: task-076 — LSP Call Graph — Edge Types, LLM Integration & Performance

**Reviewer:** Independent review (pi runtime)
**Candidate:** aaf2e866010ecb349c4fea1808f536d1083bcf9d
**Base:** 66a5fc6791eeb259c247b9a96f2ea93668c999b1
**Spec ref:** lsp-call-graph.md §7–9, §12
**Status:** approved

---

## Scope inspected

Full diff base→candidate (13 files, +1025/−39): `gyre-common/src/call_graph.rs`
(`PASS2_EDGE_METADATA`, `is_pass2_calls_edge`, `CallGraphRunRecord`),
`gyre-domain/src/call_graph_resolve.rs` (Pass 2 provenance tag),
`gyre-domain/src/view_query_resolver.rs` (`CallGraphMetrics`,
`dry_run_with_call_graph`, `query_uses_calls_edges`, `is_call_graph_complete`,
`compute_graph_summary`), `gyre-server/src/graph_extraction.rs` (`Pass2Outcome`,
`record_pass2_telemetry`, `call_graph_complete_for_repo`, KV marker),
`gyre-server/src/mcp.rs` (both MCP tool handlers + round-trip tests),
`gyre-server/src/explorer_ws.rs` (system prompt, session context, `execute_tool`),
`gyre-server/src/metrics.rs` (`gyre_graph_pass2_duration_seconds` histogram),
call-site wiring in `api/repos.rs`, `git_http.rs`, `mirror_sync.rs`, and the
`explorer-implementation.md` tool-doc amendment.

## Acceptance-criteria verification

- **§7 EdgeType consistency:** Pass 1 and Pass 2 emit the same `EdgeType::Calls`
  variant; provenance is the `PASS2_EDGE_METADATA` tag on Pass 2 edges
  (`call_graph_resolve.rs:152-156`). Both adapters round-trip edge `metadata`
  (mem upsert `mem_graph.rs:93-97`, SQLite upsert `sqlite/graph.rs:437-448`) —
  verified by source inspection since this diff does not touch the adapters.
  Pass 2 dedups against existing (source,target) `Calls` pairs, so no
  double-counting despite the differing id schemes. Go and TS pipeline tests
  pin the tag on persisted edges.
- **§8 graph_summary completeness:** `call_graph {call_edges_pass1,
  call_edges_pass2, call_graph_complete}` flows through the real JSON-RPC
  dispatch (MCP) and the explorer `execute_tool`. Completeness derives from a
  durable `CallGraphRunRecord` KV marker written only on positive toolchain
  evidence (`extraction_ran`), compared against the newest live node
  `last_seen_at` — a toolchain-less run cannot fabricate completeness. Marker
  keys are consistent between writer (`Id::new(repo_id).as_str()`) and readers.
- **§8 dryrun warning:** `dry_run_with_call_graph` emits "Call graph
  incomplete — blast radius may be underreported…" only for calls-dependent
  queries (focus/concept traversal, `calls` filter, `$callers`/`$callees`/
  `$reachable`, call-count heat/badges) with known-incomplete status; unknown
  status never warns. Both MCP and explorer paths pass the real marker-derived
  status.
- **§8 explorer prompt:** `build_system_prompt()` documents
  `call_graph.call_graph_complete`, the warning, and blast-radius/coupling/
  dead-code guidance; the per-session context block reports live
  COMPLETE/INCOMPLETE with the Pass 2 edge count.
- **§9 performance metric:** per-language durations observed on the
  `gyre_graph_pass2_duration_seconds` Prometheus histogram (buckets around the
  spec targets), total duration logged with targets inline, all in
  `record_pass2_telemetry`.
- **§12:** cross-spec citations at each integration point;
  `explorer-implementation.md` example matches the shipped `GraphSummary` shape
  (serde field names verified).

## Probes (evidence in /tmp/stage/review-evidence/)

- `cargo test -p gyre-common` — exit 0.
- `cargo test -p gyre-domain --lib` — 404/404 ok.
- `cargo test -p gyre-server --lib -- graph_extraction` — 27/27 ok.
- `cargo test -p gyre-server --lib` (full) — **1319/1319 ok**; this also proves
  the prior verification failure (E0369 `Pass2Outcome ==` in the TS test,
  checks.sh exit 1 on candidate ae64cb7b) is repaired by test-only commit
  4e598065 (verified via `git show --stat`: only `graph_extraction.rs` test
  assertions changed).
- `cargo test -p gyre-server --lib -- explorer_ws` — 43/43 ok.
- Focused task-076 tests (MCP summary metrics, MCP dryrun warning, telemetry
  gating, prompt reference) — 4/4 ok.
- **Mutation 1:** replacing the MCP dryrun handler's completeness argument with
  `None` → `mcp_graph_query_dryrun_call_graph_warning_tool_call` FAILS. Test is
  wired to production behavior. Source restored (tree clean).
- **Mutation 2:** disabling the `extraction_ran` gate in
  `record_pass2_telemetry` → `pass2_telemetry_marker_gated_on_toolchain_evidence`
  FAILS. Marker gating is enforced, not decorative. Source restored (tree clean).
- `scripts/check-arch.sh` exit 0; `scripts/check-mcp-write-tools.sh` exit 0
  (graph tools are read-only, correctly outside `needs_write`).
- `python3 scripts/check-rustfmt-diff.py 66a5fc679…` — changed lines clean
  (11 Rust files). Workspace-wide `cargo fmt --all --check` is rejected by the
  executor's cargo wrapper (infrastructure restriction, recorded in
  fmt-note.txt); full-suite fmt drift adjudication deferred to verification —
  the diff touches no file outside the changed-lines-clean set.

## Non-blocking notes (for verification / future rounds)

1. **Marker timestamp heuristic race (transient):** if an older push's Pass 2
   finishes after a newer push's Pass 1 has bumped node `last_seen_at`, the
   marker's `completed_at` can be ≥ that `last_seen_at` and the graph briefly
   reads complete before the newer Pass 2 finishes. Self-correcting (each push
   spawns its own Pass 2) and consistent with the spec's own definition
   ("Pass 2 has run"); noted, not a defect against this task's contract.
2. **Coverage matrix:** `specs/coverage/system/lsp-call-graph.md` rows §7–§9
   still carry pre-task-076 "NOT implemented" text. Refreshing the matrix is
   owned by the audit process (task-075's merge likewise did not touch it;
   task-074's did). Not a code defect; flagging so the next audit refreshes
   the rows.
3. The REST view dry-run endpoint (`api/graph.rs`) uses plain `dry_run()` —
   outside this task's scope (MCP tools named in the acceptance criteria);
   noted in case a later task wants warning parity there.

## Verdict

approved — every acceptance criterion is backed by production code on the
candidate, the key behaviors are mutation-verified (warning path, marker
gating), and all focused probes pass. Full `cargo test --all`, all-target
Clippy, workspace-wide fmt, and GitHub CI remain with the verification stage
per assignment.
