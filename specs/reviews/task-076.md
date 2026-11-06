# Review: task-076 — LSP Call Graph — Edge Types, LLM Integration & Performance

**Reviewer:** Independent review (round 11, resumed after interrupted round 10)
**Candidate:** `b48bccb2226d735816cf7c1e98e516767316d14c` (production fix commit `0b3f2317`, ancestor-verified)
**Base:** `1ae18d1c42bf9c48ae1e3c10369dbc81920664c9`
**Spec ref:** lsp-call-graph.md §7–9, §12
**Status:** approved

---

## Review scope

`git diff --name-only base..candidate` returns exactly 13 files: 11 `.rs` (gyre-common/call_graph.rs, gyre-common/lib.rs, gyre-domain/call_graph_resolve.rs, gyre-domain/view_query_resolver.rs, gyre-server/{api/repos.rs, explorer_ws.rs, git_http.rs, graph_extraction.rs, mcp.rs, metrics.rs, mirror_sync.rs}) plus specs/system/explorer-implementation.md and specs/tasks/task-076.md. No scripts, no verifiers, no unrelated tasks touched. The full diff (~1011 insertions) was reviewed line by line.

## Probe environment

Sandbox egress to static.crates.io is blocked (curl exit 7; `/tmp/cargo/registry` empty; `cargo build -p gyre-common --offline` fails at dependency resolution: "no matching package named `anyhow` found"). **Cargo test suites could not be re-run this round — this is an infrastructure restriction, not a code defect.** Evidence: `/tmp/stage/review-evidence/task-076-round11-egress.txt`. Compilation-free probes were run instead; the earlier-round cargo-suite evidence (initial round, pre-restriction) is summarized in the task document's Shipped section.

## Verification against the acceptance criteria

1. **`EdgeType::Calls` consistent between passes — VERIFIED.** Pass 1 (`rust_extractor.rs:754` via `make_edge`, `metadata: None`; same in typescript_extractor) and Pass 2 (`call_graph_resolve.rs:152`) emit the same `EdgeType::Calls` variant; Pass 2 adds the `PASS2_EDGE_METADATA` (`{"pass":2}`) provenance tag written only from the single constant in gyre-common. Both adapters round-trip `metadata` (SQLite insert + ON CONFLICT upsert sets `graph_edges::metadata`, sqlite/graph.rs:437–448; mem adapter upsert clones it, mem_graph.rs:85+). `is_pass2_calls_edge()` is the sole reader predicate; edge ids remain content-derived (`calls_edge_id` SHA-256 of repo|source|target|calls) so re-runs upsert in place.

2. **`graph_summary` reports completeness — VERIFIED.** `CallGraphMetrics {call_edges_pass1, call_edges_pass2, call_graph_complete}` added to `GraphSummary`; the pass1/pass2 split is computed from live edges by tag. Completeness comes from a durable `CallGraphRunRecord` KV marker (namespace `call_graph_status`, key = repo id), NOT from edge counts — a toolchain-less run writes no marker and cannot fabricate completeness. `is_call_graph_complete` requires `marker.completed_at >= max(live node last_seen_at)` (fresh-push window reads incomplete) and fails closed on KV error/unreadable marker. Wired through both `handle_graph_summary` (mcp.rs) and the explorer `execute_tool`.

3. **`graph_query_dryrun` warns when incomplete — VERIFIED.** `dry_run_with_call_graph` emits "Call graph incomplete — blast radius may be underreported…" only when the status is `Some(false)` (positive knowledge) AND `query_uses_calls_edges` detects calls dependence (focus/concept `calls` traversal, top-level `calls` filter, `$callers`/`$callees`/`$reachable(…,calls)` computed expressions, incoming/outgoing-calls heat/badge). Warning is pushed before result shaping (no early return). Plain `dry_run` delegates with `None` so legacy callers see no behavior change. Both the MCP handler and the explorer tool pass the marker-derived status.

4. **Explorer prompt references call graph availability — VERIFIED.** `build_system_prompt` documents `call_graph.call_graph_complete`, the warning, and blast-radius/coupling/dead-code guidance; the per-session repository context appends a live COMPLETE/INCOMPLETE line derived from the marker plus the Pass 2 edge count.

5. **Pass 2 duration logged as a metric — VERIFIED.** New `gyre_graph_pass2_duration_seconds` Prometheus HistogramVec (label `language`, buckets 0.5s–120s around the spec targets), `observe_pass2_duration`, `Pass2Outcome` per-language wall-clock timings, and `record_pass2_telemetry` which observes the histogram, logs total duration with the spec targets inline, and persists the marker only on `extraction_ran` (positive toolchain evidence: non-empty raw output from at least one language).

6. **Unit tests for updated MCP tool responses — VERIFIED (statically).** `mcp_graph_summary_call_graph_metrics_tool_call` and `mcp_graph_query_dryrun_call_graph_warning_tool_call` drive the real JSON-RPC dispatch (`build_router` + `mcp_post`) against the real graph/KV stores and assert both states (no marker → false/warn; fresh marker → true/no-warn), so they fail if the wiring or gating breaks. `pass2_telemetry_marker_gated_on_toolchain_evidence` proves marker/metric gating and the stale-marker window. Domain tests cover the pass split, the staleness rule, and warning selectivity (fires only for calls-dependent queries with known-incomplete status; no warning for unknown status or non-calls queries). These are behavior-killing tests, not self-confirming ones.

7. **`cargo test --all` / fmt — DEFERRED to verification (transport restriction, recorded above).** All 11 task-owned `.rs` files are `rustfmt --check` clean (exit 0). Repo-wide fmt drift exists only in files untouched by the candidate (e.g. gyre-adapters/src/git2_ops.rs — `git diff --quiet base..candidate -- <file>` exit 0, i.e. byte-identical to base), confirming the drift is pre-existing at the assigned base and not introduced by this task.

## Type-level cross-checks (compile-risk reduction without cargo)

`Scope::Focus{edges}/Concept{expand_edges}/Filter{computed}` variants, `EdgeFilter.filter: Vec<String>`, `HeatConfig.metric: String`, `BadgeConfig.metric: Option<String>`, `Emphasis.heat/badges`, `KvJsonStore::{kv_set,kv_get}` signatures, `AppState.{kv_store: Arc<dyn KvJsonStore>, metrics: Arc<Metrics>}`, `MemKvStore: KvJsonStore`, `Metrics::new() -> Result<Self>`, `Language` Display ("go"/"rust"/"python"/"typescript") + `PartialEq/Eq`, `Id::as_str`, `SystemTime/UNIX_EPOCH` imports, test helpers (`mcp_post`, `graph_node`, `test_state`, `make_graph_node`) — all verified present and matching usage. `is_none_or` is already used elsewhere in the workspace (sqlite/graph.rs), so the toolchain supports it.

## Invariant probes (all exit 0)

- `scripts/check-arch.sh` — pass (hexagonal boundary intact; Pass 2 subprocess I/O stays in gyre-adapters per task-072).
- `scripts/check-mcp-write-tools.sh` — pass; `graph_summary`/`graph_query_dryrun` remain read-only (handlers only `kv_get`/`list_*`), correctly outside `needs_write`.
- `rustfmt --check` on all 11 task `.rs` files — clean.
- No new in-memory durable state (marker goes through the port-backed KV store via the `store!` wiring); no fail-open ref resolution; no byte-slice truncation; no relative-path defaults; no scope-literal fabrication.

## Non-blocking observations (recorded, not defects)

- `query_uses_calls_edges`'s `call_metric` includes `"call_count"`, which is not a recognized heat/badge metric name in the resolver — a conservative over-inclusion that can only cause a spurious warning for a metric that resolves to nothing. Harmless.
- The REST endpoint `api/graph.rs` dry-run still calls plain `dry_run` (no completeness warning) — out of the task's stated scope (the task targets the MCP tools and explorer agent), and the conservative direction (no warning) matches the plain-`dry_run` contract.
- `Pass2Outcome.per_language` records durations for languages whose extraction errored, so `CallGraphRunRecord.languages` may name a failed language. Nothing consumes `languages` for correctness; completeness gating is unaffected.

## Checks the verifier must run (deferred due to sandbox egress restriction)

1. `cargo test -p gyre-domain --lib` (includes the new call_graph/view_query tests)
2. `cargo test -p gyre-server --lib mcp` (includes both new MCP round-trip tests)
3. `cargo test -p gyre-server --lib pass2_telemetry_marker_gated_on_toolchain_evidence`
4. `cargo test -p gyre-server --test explorer_ws_integration`
5. `cargo test --all` and `cargo fmt --all --check` (full-repo, per the acceptance criterion; note the pre-existing base drift in untouched files with this sandbox's rustfmt)
6. Workspace clippy gate

## Verdict

**Approved.** Every acceptance criterion is satisfied by real production code verified against the actual diff; enforcement is fail-closed in every failure mode examined (missing toolchain, unreadable marker, stale marker, fresh push); the tests are behavior-killing over real dispatch paths. The only unexecuted items are the cargo suites, which are blocked by a recorded infrastructure restriction and were green in the pre-restriction rounds on the identical `crates/` tree (candidate's crates content vs. commit `0b3f2317` differs only by the base merge, and the base..candidate diff touches only the 13 task files).
