# Review — task-183 (Architectural Timeline — time scrubber for historical architecture views)

Spec: `specs/system/system-explorer.md` §6 "Architectural Timeline".
Candidate: `3588d321e4d880e46ee3cb78dc1e5ac8bce92024` (base `e402e1ed`).
Commits under review: `6b9fb4e2b` (implementation), `cf8069ea8` (review round-1
repairs), `8405c8e47` (rustfmt repair), `f5a45295b` (clippy gate repair).
Verdict: **approved** — real implementation end-to-end, discriminating tests,
no defects found. Probe evidence: `/tmp/stage/review-evidence/probes.md`.

## What was verified

### Point-in-time graph API (real, both adapters)

- `GraphPort` gains `list_nodes_at`/`list_edges_at` with an explicit existed-at
  contract (`first_seen_at <= at` AND (`deleted_at` IS NULL OR `> at`)).
  SQLite enforces it in SQL; `MemGraphStore` enforces the identical guard in
  code (check-mem-port-contracts satisfied — one adapter's suite cannot cover
  the other's implementation, and both have tests).
- `GET /repos/{id}/graph?at=<epoch-seconds|RFC3339>` reconstructs the graph at
  that instant **including nodes soft-deleted since** — the key behavior that
  the pre-existing `list_nodes` (which always excludes soft-deleted rows)
  cannot provide. Invalid `at` returns 400 (real `ApiError::BadRequest` →
  400 mapping, error.rs:56). No new route, so the ABAC registry is untouched;
  `require_repo` scoping unchanged.
- Adapter tests assert exact membership at at=200/500/1000 across
  early/late/soft-deleted fixtures (run by this review: 3/3 passed). The
  API-level test (`repo_graph_at_returns_point_in_time_graph`) drives the real
  router, asserts epoch-seconds, RFC 3339 (URL-encoded), and the 400 path —
  type-checked here, run deferred to the verifier per the server-build budget
  rule.

### Scrubber, fetcher, ghosts, delta panel (frontend)

- `createTimelineHistoryFetcher`: 200 ms debounce, one fetch per window,
  superseded positions resolve null (client-side fallback retained), failed
  fetches reject. The race regression test (older in-flight fetch must not
  settle a newer position's waiter) was **mutation-verified by this review**:
  reintroducing the old `latestAt`-matching logic makes the test fail
  (`Received: null` at timeline-utils.test.js:466); the file was restored
  afterwards (working tree clean).
- ExplorerView wiring is complete and guarded: scrub index →
  `resolveScrubTimestamp` → debounced `api.repoGraphAt`, with late-response
  guards (repo match, still-active match, exact-`at` match) and a client-side
  `first_seen_at` fallback while the fetch is pending or failed. The server
  graph replaces the fallback only for the exact scrubbed position.
  `effectiveGraph = timelineFilteredGraph ?? graph` feeds the canvas.
- "Now" (last index) short-circuits to the current graph with no ghosts and
  no fetch — acceptance criterion satisfied by explicit guard conditions in
  both the fetch effect and `timelineFilteredGraph`.
- Ghost language (forward green dotted / backward red dotted / modified
  yellow) and per-type delta stats ("+2 types, -1 trait, 3 modified") come
  from the task-178-era utilities, now fed by the richer server
  reconstruction (backward ghosts no longer depend solely on delta_json).
- The second timeline toggle referencing `graphTimeline.length` sits inside
  `{#if graphTimeline?.length > 0}` — guarded, not a null-deref.

### Key moments and reconciliation narrative (grounded in real payloads)

- Spec approvals (delta `spec_ref` + node `spec_approved_at`), milestones
  (`milestone_completed_at`), structural changes (added/removed counts) —
  from timeline/node data; reconciliations from
  `GET /workspaces/:id/messages?kind=reconciliation_completed` (endpoint
  exists with membership enforcement; `created_at` is epoch milliseconds,
  matching the frontend's handling).
- The narrative derives ONLY from fields the production emitter sends:
  `reconciliation.rs` enriches `reconciliation_completed` with
  `specs: [{name, version}]` via provenance labels materialized on wave tasks
  at creation, wave-scoped by a millisecond wave-id label (so a prior wave's
  terminal tasks cannot bleed into a later wave's payload). The fallback
  narrative (`N tasks (duration)`) is used when a wave predates the labels —
  no fabricated persona/specs-evaluated story. Wave-scoping and label
  round-trip tests exist in reconciliation.rs (type-checked here; run
  deferred to verifier).

### Mechanical checks

- `check-arch.sh`, `check-mem-port-contracts.sh`,
  `check-byte-slice-truncation.sh`, `check-unbounded-external-http.sh`,
  `check-in-memory-state-stores.sh` — all OK.
- `cargo fmt --check` clean on the three touched crates; `cargo clippy` has
  zero findings in `gyre-ports/src/graph.rs` (the targeted
  `#[allow(clippy::double_must_use)]` matches the established
  `SpecApprovalRepository` pattern; remaining gyre-ports warnings are
  pre-existing baseline outside the changed lines).
- Frontend: 60/60 timeline tests, 176/176 neighboring ExplorerView/Canvas
  tests pass with `npm ci`-installed locked dependencies.

## Non-blocking observations

- The ExplorerView `$effect` seam that calls `api.repoGraphAt` has no direct
  component test (no test mocks `repoGraphAt`); it is simple glue over the
  mutation-verified fetcher and the derived-graph logic, and every item of
  the task's test plan is covered at the unit level. Not fakery, not a
  defect — noted for completeness.
- Coverage matrix rows remain `task-assigned` for all system-explorer
  sections including previously shipped ones; status flips are owned by the
  audit stage, not per-task.
- `web/dist/` was not rebuilt in the candidate commits — expected; the
  deterministic verifier owns bundle regeneration and publication binding.

## Verifier must run (server test binaries, deferred per budget rule)

    cargo test -p gyre-server repo_graph_at_returns_point_in_time_graph
    cargo test -p gyre-server test_mem_list_nodes_at_and_edges_at_contract
    cargo test -p gyre-server reconciliation
    cargo test -p gyre-adapters sqlite::graph::tests
    cd web && npm test
