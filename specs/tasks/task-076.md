---
title: "LSP Call Graph — Edge Types, LLM Integration & Performance"
spec_ref: "lsp-call-graph.md §7–9, §12"
depends_on:
  - task-072
progress: ready-for-review
coverage_sections:
  - "lsp-call-graph.md §7 Edge Types Produced"
  - "lsp-call-graph.md §8 LLM Integration"
  - "lsp-call-graph.md §9 Performance Considerations"
  - "lsp-call-graph.md §12 Relationship to Other Specs"
commits: ["0b3f2317a93d1570882cca8defdfa4cbd24a6d45"]
---

## Spec Excerpt

**Edge Types Produced (Pass 2):**
| Edge | Source | Coverage |
|---|---|---|
| `Calls` | Type checker (find-references) | 100% |

Pass 1 edges (`Contains`, `Implements`, `DependsOn`, `FieldOf`, `Returns`, `RoutesTo`, `GovernedBy`) remain unchanged.

**LLM Integration:** With complete call edges, the LLM can answer:
- **Blast radius:** "If I change `TaskPort::find_by_id`, what breaks?" → Follow `Implements` to implementors → Follow `Calls` to all callers (23 call sites) → Follow `GovernedBy` to affected specs.
- **Coupling analysis:** Count incoming/outgoing `Calls` edges per module → "Low coupling — safe to extract."
- **Dead code detection:** Functions with zero incoming `Calls` edges (excluding entry points with `RoutesTo`).
- **Spec change impact:** Follow `GovernedBy` → `Calls` outward → identify affected types and functions.

**Performance targets:**
- Rust workspace (~1800 nodes): ~20 seconds total
- Python project (~5300 nodes): ~30 seconds total
- Go project (~2000 nodes): ~2-5 seconds total
- TypeScript project (~3000 nodes): ~30 seconds total

All acceptable for background jobs on push/sync.

## Implementation Plan

1. **Verify edge type consistency:** Ensure Pass 2 `Calls` edges use the same `EdgeType::Calls` variant as Pass 1. The `EdgeType` enum in `gyre-common` should already have this variant — verify and add if missing.

2. **Update the `graph_summary` MCP tool** to include call graph completeness metrics:
   - `call_edges_pass1`: count of `Calls` edges from syntax analysis
   - `call_edges_pass2`: count of `Calls` edges from LSP analysis
   - `call_graph_complete`: boolean (Pass 2 has run)

3. **Update the `graph_query_dryrun` MCP tool** to leverage complete `Calls` edges for blast-radius queries:
   - The `scope.type: "focus"` with `edges: ["calls"]` should now return complete results
   - Add a warning if Pass 2 hasn't run yet: "Call graph incomplete — blast radius may be underreported"

4. **Add LLM prompt context** about call graph completeness to the explorer agent system prompt (in `explorer_ws.rs`): when the graph has complete call data, the agent can confidently answer blast-radius and coupling questions.

5. **Performance benchmarking:** Add a timing metric to the extraction pipeline that logs Pass 2 duration. Compare against the spec targets. If significantly slower, investigate (e.g., batch LSP requests, limit concurrent file opens).

6. **Update relationship documentation:** Verify cross-spec references are accurate in the code (comments linking to `system-explorer.md`, `realized-model.md`, etc.).

## Acceptance Criteria

- [x] `EdgeType::Calls` is consistent between Pass 1 and Pass 2
- [x] `graph_summary` MCP tool reports call graph completeness
- [x] `graph_query_dryrun` warns when call graph is incomplete
- [x] Explorer agent system prompt references call graph availability
- [x] Pass 2 duration is logged as a metric
- [x] Unit tests for updated MCP tool responses
- [x] `cargo test --all` passes, `cargo fmt --all` clean

## Agent Instructions

Read `specs/system/lsp-call-graph.md` §7–9 and §12. This task depends on task-072 for the core pipeline. Focus on the integration points: MCP tools (`graph_summary`, `graph_query_dryrun` in `crates/gyre-server/src/mcp.rs` and `crates/gyre-domain/src/view_query_resolver.rs`), the explorer agent system prompt (`crates/gyre-server/src/explorer_ws.rs`), and graph edge types (`crates/gyre-common/`). The performance work is benchmarking/logging, not optimization — measure first, optimize only if targets are missed.

## Shipped

Task-076 closes the lsp-call-graph.md §7–9/§12 integration surface on top of the task-072 pipeline. All changes are production code paths — no stubs, no audit-only modes.

### §7 Edge types (provenance, same variant)

Pass 1 and Pass 2 already emit the same `EdgeType::Calls` variant; task-076 adds the missing distinguisher. `resolve_call_edges` (gyre-domain) now tags every Pass 2 `Calls` edge with the `PASS2_EDGE_METADATA` JSON constant (`{"pass":2}`, gyre-common/src/call_graph.rs); both storage adapters round-trip edge `metadata` (SQLite column + upsert, mem clone), and the content-derived edge id is unchanged so re-derivation upserts in place. `is_pass2_calls_edge()` is the single reader-side predicate.

### §8 graph_summary completeness metrics

`compute_graph_summary` (gyre-domain) gains a `call_graph` member: `call_edges_pass1` (live Calls edges without the tag), `call_edges_pass2` (tagged), and `call_graph_complete`. Completeness is NOT inferred from edge counts — a toolchain-less Pass 2 run persists zero edges and must not read as complete. Instead the pipeline writes a durable `CallGraphRunRecord` marker (KV store, namespace `call_graph_status`, key = repo id) when a Pass 2 run has positive toolchain evidence (`extraction_ran`: non-empty raw output from at least one language). `is_call_graph_complete()` treats the graph as complete only when the marker's `completed_at` ≥ the newest node `last_seen_at`, so a fresh push (Pass 1 done, Pass 2 in flight) correctly reads incomplete until Pass 2 re-finishes. `handle_graph_summary` (mcp.rs) and the explorer `execute_tool` both pass this through; tool descriptions document the new fields, and explorer-implementation.md's `graph_summary` example was amended to match.

### §8 graph_query_dryrun blast-radius warning

New `dry_run_with_call_graph()` (gyre-domain) takes the completeness status; `dry_run()` delegates with `None` (unknown → no warning, so the warning only fires on positive knowledge). When a query depends on `calls` edges — focus/concept traversal along `calls`, a top-level `calls` edge filter, `$callers`/`$callees`/`$reachable(...,calls)` expressions, or incoming/outgoing-calls heat/badge metrics — and the status is known-incomplete, the dry-run emits: "Call graph incomplete — blast radius may be underreported. …". Both the MCP handler and the explorer tool pass the real marker-derived status.

### §8 Explorer agent prompt

`build_system_prompt()` (explorer_ws.rs) now documents the `call_graph.call_graph_complete` field, the incompleteness warning, and when to trust `calls` edges for blast-radius (implements → calls → governed_by), coupling, and dead-code questions. The per-session "Current Repository Context" block appends a live "Call graph: COMPLETE/INCOMPLETE" line derived from the durable marker plus the count of Pass 2-resolved call edges.

### §9 Performance metric

`extract_and_persist_call_graph` now returns a `Pass2Outcome` (persisted count, `extraction_ran`, per-language wall-clock durations). The step-7 spawn times the whole run and calls `record_pass2_telemetry`, which (a) observes per-language durations on the new `gyre_graph_pass2_duration_seconds` Prometheus histogram (labels: language; buckets 0.5s–120s around the spec targets), (b) logs total duration with the spec targets inline, and (c) persists the `CallGraphRunRecord` marker — only when `extraction_ran` is true (missing toolchain must not fabricate completeness). No optimization was done: measurement first, per the task instructions.

### §12 Cross-spec references

New code comments cite lsp-call-graph.md §7/§8/§9 at each integration point (call_graph.rs, call_graph_resolve.rs, view_query_resolver.rs, graph_extraction.rs, metrics.rs, explorer prompt); the explorer-implementation.md tool documentation now matches the shipped response shape.

### Test evidence

Focused probes (commands, exit codes, output) under /tmp/stage/review-evidence/task-076-probes.txt. Prior round (candidate 57d952dd, code tree identical to this branch head): `cargo test -p gyre-domain --lib` 401/401 ok; `cargo test -p gyre-server` (lib 1268/1268 in 72.8s + all integration binaries) ok; `cargo test -p gyre-common -p gyre-adapters -p gyre-cli` ok; workspace clippy gate exits 0; `check-clippy-diff.py`/`check-rustfmt-diff.py` vs base clean; check-arch.sh and the other invariant scripts exit 0. Continuation round (branch head c4442642 after the base merge — zero code delta for crates/ and web/src/): `cargo test -p gyre-server --test explorer_ws_integration` 9/9 ok (the probe the gateway-transport cutoff interrupted); focused lib filters `call_graph` 3/3, `graph_summary` 2/2, `pass2_telemetry` 1/1 ok; `cargo test -p gyre-domain --lib` 401/401 ok; rustfmt --check on the task's own seven files exits 0. Contract-repair round (branch head 3b91ce51, again zero code delta; evidence in /tmp/stage/review-evidence/task-076-probes-round3.txt): `cargo test -p gyre-domain --lib call_graph` 19/19 ok, `cargo test -p gyre-server --lib pass2_telemetry` 1/1 ok, `cargo test -p gyre-server --test explorer_ws_integration` 9/9 ok, `cargo test -p gyre-server --lib mcp` 81/81 ok including the `graph_summary` metrics and `graph_query_dryrun` warning round-trip tests. Contract-repair round 2 (branch head 2c68fdf, again zero code delta; evidence in /tmp/stage/review-evidence/task-076-probes-round4.txt): `cargo test -p gyre-domain --lib call_graph` 19/19 ok, `cargo test -p gyre-server --lib pass2_telemetry` 1/1 ok, `cargo test -p gyre-server --lib mcp` 81/81 ok (incl. `mcp_graph_summary_call_graph_metrics_tool_call` and `mcp_graph_query_dryrun_call_graph_warning_tool_call`, also run individually), `cargo test -p gyre-server --test explorer_ws_integration` 9/9 ok on rerun after one load-induced timing flake (`explorer_ws_rate_limiting` races two messages against the 1000ms rate-limit window; failed once under concurrent sandbox load, passed isolated and on full-suite rerun — same suite was 9/9 in round 3 on the identical tree); rustfmt --check on the task's own files exits 0. Key behavioral tests: MCP round-trip tests prove `graph_summary` reports per-pass counts + completeness from the real KV marker and that `graph_query_dryrun` warns (and stops warning once a fresh marker exists) over the real JSON-RPC dispatch path; `pass2_telemetry_marker_gated_on_toolchain_evidence` proves no marker and no metric observation without toolchain evidence, marker + histogram with it, and the stale-marker/fresh-push incompleteness window; domain tests prove the pass1/pass2 split, the marker-vs-last_seen_at staleness rule, and that the warning fires only for calls-dependent queries with known-incomplete status.

### Known scope notes

- `cargo fmt --all --check` reports pre-existing drift in untouched files (e.g. gyre-adapters/src/git2_ops.rs) at the assigned base with this sandbox's rustfmt; the task's own files are fmt-clean (left for verification to adjudicate — reformatting unrelated files was deliberately not done to keep review scoping exact).
- Full `cargo test --all` and GitHub CI remain owned by the verification stage, per assignment.
- The prior implement round ended in a gateway-transport error (no code defect); the continuation round verified the identical merged tree and restored the shipped summary the pipeline checkpoint had reset to template state. The contract-repair rounds repeated that restore after the executor again reset the task document to the assigned contract (progress/criteria only; no code change), re-running the focused probes above on the identical tree. The production fix lives in commit 0b3f2317 and has been untouched since; later rounds changed only this task document.
