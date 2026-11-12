# Review — task-183 (Architectural Timeline — time scrubber for historical architecture views)

Spec: `specs/system/system-explorer.md` §6 "Architectural Timeline".
Commits under review: `6b9fb4e2` (product), `2ce8bce1` (process).
Verdict: **needs-revision** — two blocking findings (F1 fetcher race, F2 fabricated reconciliation narrative); the backend point-in-time API and the rest of the scrubber wiring verified real and correct.

## Round 1

Probes (candidate `2ce8bce1`, evidence in `/tmp/stage/review-evidence/task-183-evidence.md`):

- `cargo test -p gyre-adapters --lib sqlite::graph::tests::list_` → **3 passed** (both new point-in-time suites included).
- `cargo check -p gyre-server --lib --tests` → clean (the two new server tests compile; execution deferred to verifier — see below).
- `cd web && npm ci && npx vitest run src/__tests__/TimelineScrubber.test.js src/__tests__/timeline-utils.test.js` → **58/58 passed**.
- `bash scripts/check-arch.sh`, `bash scripts/check-mem-port-contracts.sh` → OK.
- Two adversarial probes against the new debounce fetcher and the reconciliation marker narrative — both reproduce defects (below).

Verified working (no findings):

- **Point-in-time graph API is real and correct.** `GraphPort::list_nodes_at`/`list_edges_at` define a clear existed-at contract (`first_seen_at <= at` AND (`deleted_at` None OR `> at`)), implemented in BOTH adapters — SQL filters in SQLite, the same guard in code in `MemGraphStore` — satisfying the mem-port-contract invariant. The SQLite tests are genuine kill-bug tests: they pin the soft-deleted-resurrection behavior (a node deleted at 300 is present at 200, absent at 500) that `list_nodes` can never produce, and the boundary conditions. `GET /repos/{id}/graph?at=` accepts epoch seconds and RFC 3339, returns 400 on unparseable input (tested), composes correctly with `?concept=`, and is documented in `docs/api-reference.md`. No new route → no ABAC registry change needed (confirmed: no route-table edit in the diff).
- **Scrubber wiring is real.** The `$effect` fetch + `timelineFilteredGraph` guard (`timelineHistoricalGraph.at === timelineScrubTimestamp`) correctly prefers the server reconstruction over the client-side `first_seen_at` filter, and the "Now" position (last index) shows the current graph with no ghosts and no fetch — acceptance criterion met. The unguarded-looking `graphTimeline.length - 1` in the insights-panel toggle (ExplorerView.svelte:2851) sits inside `{#if graphTimeline?.length > 0}` — safe.
- **Endpoint wiring is correct.** `GET /workspaces/{id}/messages?kind=` returns `Json<Vec<Message>>` with `created_at` in epoch milliseconds — the frontend's array handling and ms→s conversion are right; marker positioning by timestamp works.
- **Delta-backed key-moment narratives are grounded server-side**: `GET /repos/{id}/graph/timeline` returns `DeltaResponse` with a `narrative` field (`generate_template_narrative`), so the popover "Why" field and the new dismissible narrative panel (previously the marker-click callback was a no-op) work for delta-backed moments.

Findings (both blocking):

- [-] **F1 (major, code): `createTimelineHistoryFetcher` hands a newer scrub position the graph fetched for an older position.** `web/src/lib/timeline-utils.js` `settle()`: when the debounce timer fires it captures `target = latestAt` and starts `fetchAt(target)`; if a new `request(at)` arrives while that fetch is in flight, `latestAt` changes but `pending` keeps accumulating. When the in-flight fetch settles, every waiter with `w.at === latestAt` receives the **old target's** graph — including the waiter for the newer position. Reproduced (probe `probe-fetcher-race.test.js`): `request(500)` → window elapses → `request(1000)` during flight → `request(1000)` resolves with `{"nodes":[{"id":"at-500"}]}`. In ExplorerView this becomes `timelineHistoricalGraph = {at: 1000, graph: <500-graph>}`, passes the `at === timelineScrubTimestamp` guard, and the canvas renders the wrong point in time (wrong ghosts, wrong delta-panel counts) for as long as the user stays at that position; the follow-up fetch for 1000 drains an empty `pending` list and its correct result is discarded. Trigger: scrub, pause >200 ms, scrub again while the fetch is in flight (typical HTTP latency) — a routine interaction. Fix: settle only waiters whose `at` matches the fetch's target, and leave newer waiters pending for the next timer fire (or track an in-flight generation and ignore its result when superseded).
- [-] **F2 (major, code/fakery): the reconciliation key-moment narrative reads payload fields the server never sends — every production reconciliation event renders a false narrative.** `web/src/components/TimelineScrubber.svelte` (keyMoments derivation) reads `payload?.persona_name`, `persona_id`, `specs_changed`, `specs_evaluated`. The only production emitter of `reconciliation_completed` messages (`reconciliation.rs` `maybe_emit_reconciliation_completed` → `emit_reconciliation_completed`, lib.rs:1496) sends `{workspace_id, wave_started_at, task_count, duration_secs}` — none of those fields (grep-verified: no other non-test emitter of `MessageKind::ReconciliationCompleted`). Reproduced (probe `probe-recon-payload.txt`): with the exact production payload (`task_count: 7, duration_secs: 86400`), clicking the marker shows "Why Reconciliation of persona: 0 specs evaluated, no changes." — a hardcoded fallback standing in for looked-up data that misstates what happened. The shipped test fabricates a payload shape production never emits, so it passes while production behavior is wrong — mirrored-logic test inflation. Fix: derive the narrative from the real payload (task_count/duration_secs/wave_started_at, e.g. "Reconciliation wave complete: 7 tasks over 24h") or enrich the server payload with persona/specs-changed counts; extend the test to pin the production payload shape.

Deferred to the independent verifier (server test linking exceeds the model-stage memory budget):

- `cargo test -p gyre-server --lib api::graph::tests::repo_graph_at_returns_point_in_time_graph`
- `cargo test -p gyre-server --lib api::graph::tests::test_mem_list_nodes_at_and_edges_at_contract`
- full `cargo test --all`; full `cd web && npm test`

Generated-output note (separate, non-blocking): `cargo check -p gyre-server` runs build.rs, which regenerated `web/dist/` in the review working tree; reverted to the committed state. Dist freshness is the verifier's ownership.

Test-inflation check: the SQLite point-in-time suites, the server HTTP suite (deferred but inspected — real router, real store, 400-on-invalid asserted), and the fetcher debounce/cancel suites are genuine. The reconciliation-marker test is inflated per F2 (fabricated payload shape). The "popover shows the grounded narrative" test asserts real server-provided narrative fields.

Scope check: no out-of-scope edits — diff touches only the graph port/adapters, graph API, timeline frontend, tests, docs, and the task file.
