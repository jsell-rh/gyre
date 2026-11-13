# Review — task-183 (Architectural Timeline, system-explorer.md §6)

Round 4 (re-review after merge-conflict rebase onto `48b3d47e`). Candidate
`44fa1e5f` on branch `pipeline/task-183/10099d8edf08410b97e9763047eb5c42-1`.
Prior round's findings F1 (backward ghosts never drawn) and F2
(reconciliation markers dropped by index-proximity dedupe) were repaired in
`b7666c45`; this round verifies those repairs by mutation and re-examines the
full diff from base.

Verdict: **approved**.

## Evidence (all under /tmp/stage/review-evidence/)

- `frontend-focused-tests.txt` — `npx vitest run` on
  `timeline-utils.test.js`, `TimelineScrubber.test.js`,
  `ExplorerCanvas.test.js` after `npm ci`: **226 passed, 0 failed**.
- `probe-f1-mutation.txt` — reverted the F1 fix (remove ghosts keyed by the
  synthetic `removed:<qn>` id again) and re-ran the focused suites:
  **4 tests fail**, including the rendering-level
  `draws the red-dotted strikethrough for backward ghosts` (asserts
  `setLineDash([4,4])` through the real ExplorerCanvas draw path) and
  `drawGhostRemoveNode`'s dotted-outline-then-solid-strikethrough stroke
  order. The fix is real and regression-protected; sources restored after.
- `probe-f2-mutation.txt` — reverted the F2 fix (back to the global
  index-proximity dedupe) and re-ran `TimelineScrubber.test.js`:
  **3 tests fail**, including the collision fixture (reconciliation event
  landing at the same timeline index as a >5-node structural delta) which
  asserts BOTH markers render, both stay clickable via vertical stacking
  (`markerRows`), and the reconciliation narrative
  ("7 tasks aligned code with architect v4 (1d)") is reachable by click.
  Sources restored after; `git diff` clean except the controller's staged
  task-file reset.
- `probe-adapters-list-at.txt` — `cargo test -p gyre-adapters --lib list_`:
  **48 passed**, including
  `sqlite::graph::tests::list_nodes_at_reconstructs_point_in_time` and
  `list_edges_at_reconstructs_point_in_time` (both-direction assertions:
  early/late/gone nodes and stable/late/gone edges across at=200/500/1000).
- `probe-cargo-check-server.txt` — `cargo check -p gyre-server` (lib):
  clean in 4m45s. (`--all-targets` is rejected by the build wrapper; test
  compilation belongs to the verifier.)
- `probe-arch.txt` — `scripts/check-arch.sh`: passed.

## F1 repair (approved)

`computeTimelineGhostOverlays` now keys each remove ghost by the historical
node's REAL id when the server `?at=` reconstruction resurrects the
soft-deleted node in the scrubbed-time graph (matched by `qualified_name`,
synthetic id only as the client-side-filter fallback, which cannot contain
the removed node). `ExplorerCanvas.drawGhostOverlays` resolves remove ghosts
through `layoutNodeMap.get(ghost.id)`, and ExplorerView passes
`timelineFilteredGraph` (the `?at=` graph once the debounced fetch lands) as
the canvas `nodes` — so `drawGhostRemoveNode` (red fill, red dotted
`setLineDash([4,4])` border, red solid strikethrough) now genuinely draws.
Verified end-to-end by the mutation probe above.

## F2 repair (approved)

Dedupe is now per-event-type (`lastIndexByType`), so different-type key
moments at the same index (reconciliation completing right after its tasks'
structural delta — the expected collision) are both kept; colliding markers
stack vertically via `markerRows` so each stays visible and clickable.
Same-type within-1-index merge behavior is preserved and tested. The
reconciliation narrative derives only from fields the production emitter
sends (`task_count`, `duration_secs`, `specs[]` provenance labels added to
`reconciliation_completed` payloads in `reconciliation.rs`, wave-scoped by
the `recon-wave:` identity label so prior waves' specs cannot bleed into a
later wave's payload — tested in the wave-2 scoping assertions).

## Other diff surfaces re-examined (no findings)

- **Point-in-time API** (`GraphPort::list_nodes_at`/`list_edges_at`,
  SQLite + mem adapters, `GET /repos/{id}/graph?at=`): explicit
  existed-at contract (`first_seen_at <= at` AND not deleted at or before
  `at`), enforced identically in both adapters (mem contract pinned by a
  server-side test per check-mem-port-contracts); epoch-seconds or RFC 3339
  input, invalid `at` → 400 (tested). No new routes, so no ABAC registry
  changes; `time` crate gains only the `parsing` feature.
- **Scrubber/debounce** (`createTimelineHistoryFetcher`): 200 ms debounce,
  superseded positions resolve null (keep client-side fallback), in-flight
  fetches settle only their own target (race regression tested), ExplorerView
  additionally pins the landed graph to the exact scrub timestamp and repo.
  "Now" (last index) shows the current graph with no ghosts (tested).
- **Delta panel**: per-type added/removed/modified counts rendered beside
  the scrubber (`tl-delta-stats`) plus the canvas ghost-mode chips; counts
  come from `computeTimelineDeltaStats` against the real historical graph.
- **Key-moment narrative**: reconciliation narratives from real payloads;
  delta-backed moments render the server-generated
  `generate_template_narrative` text in the popover "Why" field and the
  ExplorerView narrative panel (`timelineMomentDetail`).
- `specs/tasks/task-183.md` frontmatter records all task-labeled product
  commits including the repair `b7666c45` (attribution check satisfied).

## Deferred to the verifier

- `cargo test -p gyre-server --lib api::graph::tests::repo_graph_at_returns_point_in_time_graph`
- `cargo test -p gyre-server --lib api::graph::tests::test_mem_list_nodes_at_and_edges_at_contract`
- `cargo test -p gyre-server --lib reconciliation::` (label round-trips,
  provenance labels on wave tasks, completion-payload specs, wave-2 scoping)
- Full workspace test/clippy/fmt + web build (bundle regeneration is
  verifier-owned; this diff contains no `web/dist` changes, as expected)

## Minor (non-blocking, recorded for completeness)

- `graph.rs` `resolve_at`: a u64 epoch value above `i64::MAX` wraps negative
  in the `at as i64` cast and yields an empty graph rather than a 400.
  Cosmetic; no safety impact.
- ExplorerView's reconciliation events are lazy-loaded on first time-travel
  toggle per workspace; on workspace switch the previous workspace's events
  remain visible until the new fetch lands (transient, self-correcting, and
  the fetch failure path allows retry).
