# Review — task-183 (Architectural Timeline — time scrubber for historical architecture views)

Spec: `specs/system/system-explorer.md` §6 "Architectural Timeline".
Candidate: `78994918` (branch `pipeline/task-183/f49a62e9fdc4408e83303db20cacfac4-1`, merged with main `a8bd7d09` in `2044643a`).
Verdict: **needs-revision** — two material findings (F1 backward ghosts never render; F2 reconciliation key moments silently dropped by index dedupe). The point-in-time graph API, scrubber wiring, delta panel, and tests are otherwise real and verified.

## Round 1

Probes run (checkout at candidate `78994918`; evidence in `/tmp/stage/review-evidence/`):

- `cargo check -p gyre-server -p gyre-ports -p gyre-adapters` → clean (7m23s; cargo-check.txt).
- `cargo test -p gyre-adapters sqlite::graph::tests::list_` → **3/3 passed**, including both point-in-time reconstruction tests (sqlite-graph-tests.txt).
- `cargo clippy -p gyre-ports -p gyre-adapters` → graph.rs clean; pre-existing traits warn `double_must_use` under this sandbox's clippy 1.99 (toolchain artifact; CI Clippy was green on the published PR — verifier should re-run on its toolchain).
- `cd web && npm ci && npx vitest run timeline-utils.test.js TimelineScrubber.test.js` → **60/60 passed** (vitest-timeline.txt).
- `npx vitest run MoldableViews.test.js ExplorerCanvas.test.js` → **181/181 passed** — merged-in task-179 views intact (vitest-explorer-coexistence.txt).
- Invariant scripts: `check-arch.sh`, `check-mem-port-contracts.sh`, `check-abac-route-registry.sh`, `check-dead-message-kinds.sh`, `check-inert-enforcement.sh` → all OK.
- Empirical canvas probes (temp test files, deleted after run; sources retained in evidence dir): see F1/F2.
- gyre-server test compilation is blocked by the executor's 4 GiB build wrapper in this sandbox — server tests recorded for the verifier below.

Verified working (no findings):

- **Point-in-time graph API is real and contract-pinned on both adapters.** `GraphPort::list_nodes_at`/`list_edges_at` with the explicit existed-at contract (`first_seen_at <= at` AND (`deleted_at` is null OR `> at`)), enforced in SQL in the SQLite adapter and in code in `MemGraphStore`; the SQLite tests (`list_nodes_at_reconstructs_point_in_time`, `list_edges_at_reconstructs_point_in_time`) drive real storage with soft-deleted/late-added fixtures and assert exact membership at three instants. The server handler `repo_graph_at_returns_point_in_time_graph` (verified by inspection; verifier must run) drives the real router, asserts epoch-seconds and RFC3339 forms, the soft-deleted-node resurrection, and the 400 on unparseable `at`. The mem-contract test pins the in-code guard separately, satisfying the cross-adapter contract invariant.
- **Debounced fetcher is correct including the race repair.** `createTimelineHistoryFetcher` settles waiters per fetch target (not per `latestAt`); the regression test ("does not settle a newer position with an older in-flight fetch") fails against the pre-repair matching-by-latestAt logic and passes now. ExplorerView additionally refuses late out-of-order responses by position check.
- **Reconciliation narrative provenance is real.** `reconciliation.rs` materializes `meta-spec:{name}@v{version}` and `recon-wave:{id}` labels on wave tasks at creation (threaded through `PendingRolloutTask` for deferred rolling batches), and `maybe_emit_reconciliation_completed` scopes the payload's `specs` array to the current wave by wave-id label — no fabricated persona narrative; the frontend renders only fields the production emitter sends. The wave-scoping test (wave 1 v4 vs wave 2 v5, same second) covers the bleed case.
- **Delta summary panel, scrubber-at-now, key-moment popover** verified by inspection and the passing component tests: per-type added/removed/modified counts render when scrubbing; last index = present (no fetch, no ghosts); marker click surfaces the narrative (delta narratives come from the real timeline API's grounded template narratives).
- No test inflation found: the new tests assert exact membership/counts/ids and would fail if the production behavior broke; the race-regression and legacy-payload-narrative tests encode real failure modes.

Findings:

- [-] **F1 (major): backward ghosts (removed-since nodes) are never drawn on the canvas — the "removed (red dotted)" acceptance criterion is not met.** `computeTimelineGhostOverlays` (`web/src/lib/timeline-utils.js:176`) keys remove-ghosts as `removed:${qualified_name}`, but `drawGhostOverlays` (`web/src/lib/ExplorerCanvas.svelte:3077`) only invokes `drawGhostRemoveNode` when `layoutNodeMap.get(ghost.id)` resolves — and layout nodes are keyed by graph node id, so the synthetic id never matches. Empirical probe (probe-backward-ghost.txt): rendering the canvas with the exact overlay set produced for a historical scrub position (server `?at=` graph, which resurrects the soft-deleted node) draws **zero** red-dotted strokes; the identical overlay re-keyed to the node's real id draws the red dotted outline + strikethrough (control passes), proving the render path works and the defect is the synthetic ghost id. Net user-visible behavior: removed-since elements render as plain historical nodes with no indication they are gone now (the `− Removed (N)` chip and delta counts still show). This fails task-183 AC "Ghost outlines for nodes added since (green dotted) and removed (red dotted)" and the Shipped note's claim "backward ghosts (removed since, red dotted, recovered from ArchitecturalDelta records)" — they are recovered into overlay data but not rendered. The `removed:` id scheme predates this candidate (task-178 base), but §6 is this task's coverage section and its AC explicitly owns the backward-ghost visual. Repair: key the remove ghost by the historical node's real id when it can be found in `timelineFilteredGraph` (fall back to the synthetic id), and add a rendering-level test that fails when the red-dotted strikethrough is not drawn.
- [-] **F2 (major): reconciliation key-moment markers are silently dropped when they collide with any nearby key moment — and collision is the expected case.** The new reconciliation-moment loop in `TimelineScrubber.svelte` pushes moments into the pre-existing index-proximity dedupe ("merge moments within 1 index"), which keeps the first-sorted moment and discards the rest. Empirical probe (probe-recon-marker-dedupe.txt): a timeline with a structural-change moment (8 nodes added) at the index nearest a `reconciliation_completed` event's `created_at` renders **1** marker — the structural one — and the reconciliation narrative ("Reconciliation wave complete: 7 tasks aligned code with architect v4") is unreachable. Because a reconciliation wave completes right after its tasks land large code-alignment deltas, the wave's marker will frequently map to the same timeline index as the structural change it caused. This degrades AC "Key moments marked on timeline (spec approvals, milestones, reconciliations)" and "Click key moment shows delta narrative" for exactly the events this task added. Repair: dedupe within event type (or merge narratives instead of dropping), and extend the marker test with a collision fixture.

Minor (non-blocking, recorded for completeness):

- `TimelineScrubber` marker positions are timestamp-scaled but dedupe is index-granular — with sparse timelines one index can span months, so distinct moments months apart can merge; conversely dense timelines over-merge. F2's repair should address the granularity source.
- The `#[allow(clippy::double_must_use)]` on `GraphPort` is documented and scoped; under this sandbox's clippy 1.99 every pre-existing async trait in `gyre-ports` warns the same lint — verifier should confirm workspace clippy on the CI toolchain.
- Working-tree `web/dist/` churn observed during probes is generated output from the sandbox npm wrapper; restored. Owned by the deterministic verifier — not a source defect.

Server tests the independent verifier must execute (build blocked in review sandbox by the 4 GiB wrapper):

- `cargo test -p gyre-server api::graph::tests::repo_graph_at_returns_point_in_time_graph`
- `cargo test -p gyre-server api::graph::tests::test_mem_list_nodes_at_and_edges_at_contract`
- `cargo test -p gyre-server reconciliation::` (label round-trips, `reconciliation_creates_task_for_bound_repo`, wave payload scoping / two-wave isolation tests)
- Full workspace suite + web vitest suite, plus `cargo clippy` on the CI toolchain.
