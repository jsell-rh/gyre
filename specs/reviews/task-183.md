# Review — task-183 (Architectural Timeline — time scrubber for historical architecture views)

Spec: `specs/system/system-explorer.md` §6 "Architectural Timeline". Task plan: point-in-time graph API (`?at=`), `TimelineScrubber` scrubber with 200 ms debounced fetch, ghost overlays (green/red dotted, modified yellow), delta panel, key-moment markers (spec approvals, milestones, reconciliations, structural changes), key-moment narrative on click.

Commits under review: `6b9fb4e2b` (original implementation), `cf8069ea8` (review repairs for round `1dd7441246b74416b1bc06d19d55520e`), plus process commits `273df8a1b`/`5c3e6399d` (task file only). Cumulative diff vs assigned base `ceefc14af`: 15 files, +1378/−33 (see `/tmp/stage/review-evidence/full-diff.txt`).

Verdict: **needs-revision** — one blocking finding (rustfmt gate); both round-1 defects are genuinely repaired and the feature is real.

## Round 2 verification of the two durable findings

**1. Fetcher race (`web/src/lib/timeline-utils.js`) — fixed.**
`settleTarget(target, ...)` now settles only waiters whose `at` equals the fetch's own target; `releaseSuperseded(exceptAt)` resolves superseded-window positions with `null` (client-side fallback keeps rendering) instead of hanging. `ExplorerView.svelte` additionally refuses to pin a response whose `at` no longer equals the current `timelineScrubTimestamp`. Mutation probe: reintroducing the pre-fix `latestAt`-matched settle made the regression test (`does not settle a newer position with an older in-flight fetch (race regression)`) fail (1 failed / 31 skipped, failed at the p500/p1000 assertions); the committed fix passes. File restored byte-identical after the probe (`git diff` empty).

**2. Reconciliation narrative from fabricated fields — fixed.**
The frontend now derives the narrative only from fields the production emitter sends: `task_count`, `duration_secs`, and the new `specs: [{name, version}]`. The server side is a real enrichment, not a frontend re-reading of the old payload: `run_reconciliation` stamps every wave task at creation with `meta-spec:{name}@v{version}` provenance and a `recon-wave:{millis}` wave-identity label (threaded through `PendingRolloutTask` so deferred rolling-rollout batches keep the original wave's identity), and `maybe_emit_reconciliation_completed` scopes `specs` to the current wave via `task_wave_id` — prior waves' terminal tasks cannot bleed their versions into a later wave's event (pinned by the wave-2 test asserting v5-only after wave 1 emitted v4). The frontend fixture pins the exact production payload shape; a legacy-payload test asserts the truthful `task_count`/`duration_secs` fallback and that no persona/specs-evaluated story is fabricated. `emit_reconciliation_completed` passes the payload through `state.emit_event` unchanged; `GET /api/v1/workspaces/:id/messages` (membership-enforced, `Json<Vec<Message>>`, `created_at` epoch ms, object payload) matches what `TimelineScrubber`'s marker code reads.

## Feature verification

- **Point-in-time API is real.** `GraphPort::list_nodes_at`/`list_edges_at` with an explicit existed-at contract (`first_seen_at <= at` AND not deleted at or before `at`), implemented in SQLite (SQL filters) and `MemGraphStore` (same guard in code); both adapters' tests pin the contract (`check-mem-port-contracts` passes). `GET /repos/{id}/graph?at=<epoch-seconds|RFC3339>` reconstructs soft-deleted-since nodes, returns 400 on unparseable `at`, documented in `docs/api-reference.md`.
- **Scrubbing drives the canvas.** `ExplorerView.effectiveGraph = timelineFilteredGraph ?? graph` feeds canvas nodes/edges/layout/density; `timelineFilteredGraph` prefers the server `?at=` graph pinned for the exact scrub position (position-equality guard) with the client-side `first_seen_at` filter as instant fallback; the "Now" position (last index) renders the current graph with no ghosts and no fetch.
- **Tests (all run here):** `timeline-utils` + `TimelineScrubber` 60/60; 3 wiring suites 37/37; 12 ExplorerView suites 70/70. `cargo check -p gyre-server --tests` clean (9 min); `cargo test -p gyre-adapters list_nodes_at` / `list_edges_at` pass; `check-arch.sh`, `check-mem-port-contracts.sh`, `check-dead-message-kinds.sh` pass.
- **Attribution:** candidate tip lists both product commits in `commits:` frontmatter (the working-tree reset to the pristine template is a review-stage artifact, not a candidate defect).

## Blocking finding

- **rustfmt drift on candidate-added lines** in `crates/gyre-adapters/src/sqlite/graph.rs`: `python3 scripts/check-rustfmt-diff.py ceefc14af` exits 1 — "changed lines need formatting: 1073, 1074, 1075, 1083, 1138, 1146", all inside the two tests this candidate adds (`list_nodes_at_reconstructs_point_in_time`, `list_edges_at_reconstructs_point_in_time`): single-line `assert!`/`assert_eq!` macros with message strings that rustfmt splits. `cargo fmt --check -p gyre-adapters` fails likewise (`gyre-server`/`gyre-ports` are clean). The Shipped note claims "cargo-fmt drift in graph.rs from the original candidate commit fixed (would fail the CI rustfmt job)" — but only `crates/gyre-server/src/api/graph.rs` was formatted; the identical drift in the adapters crate remains, so the claim is inaccurate and the repo's changed-lines fmt gate fails against the task base (and against a mainline merge in the CI fmt job, which diffs `HEAD^1` = previous main). Fix: `cargo fmt -p gyre-adapters` (or split the six macros by hand) and correct the Shipped note.

## Verifier must run (too heavy for this sandbox)

- `cargo test -p gyre-server --lib reconciliation` (label round-trips, provenance labels on created tasks, payload `specs` assertions, wave-2 scoping)
- `cargo test -p gyre-server --lib repo_graph_at_returns_point_in_time_graph`
- `cargo test -p gyre-server --lib test_mem_list_nodes_at_and_edges_at_contract`
- Workspace clippy gate (`scripts/check-clippy-diff.py`) and the full web vitest suite
