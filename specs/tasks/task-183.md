---
title: "Architectural Timeline — time scrubber for historical architecture views"
spec_ref: "system-explorer.md §6"
depends_on: [task-178]
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §6. Architectural Timeline"
commits: ["6b9fb4e2be51bd1d6d702c959878a8f9331dc6fe"]
---

## Spec Excerpt

system-explorer.md §6 defines an Architectural Timeline — a time-scrubber at the bottom of the canvas that lets the user see the system at any point in history.

**What changes as you scrub:**
- The canvas shows the knowledge graph at that point in time
- Ghost outlines show elements that have been added since (forward ghosts) or will be removed (backward ghosts)
- The sidebar shows the delta: "Between then and now: +12 types, -3 types, +2 traits, 8 types modified"

**Key moments** are marked on the timeline:
- Spec approvals (spec X approved → implementation began)
- Milestone completions
- Reconciliation events (persona v3 → v4)
- Major structural changes (new crate added, trait split)

Click a key moment → the delta panel shows what changed and why (narrative from the knowledge graph).

**Visual language:** The scrubber is a horizontal slider. Key moments are marked as dots/ticks on the timeline. The current position shows a date label. Ghost nodes use the same visual language as system-explorer.md §3 (dotted outlines).

## Implementation Plan

1. **Timeline scrubber component** (`TimelineScrubber.svelte`):
   - Horizontal range slider at the bottom of the canvas area
   - Date range: first commit → current date
   - Dragging updates the canvas to show the graph at that point in time
   - Current date label shown above the scrubber handle
   - Smooth scrubbing with debounced graph fetching (200ms debounce)

2. **Historical graph API**:
   - The knowledge graph needs to support point-in-time queries
   - Check if `GET /repos/:id/graph` accepts a `?at=<timestamp>` or `?commit=<sha>` parameter
   - If not, add a `?at=<iso-timestamp>` query parameter that filters the graph to nodes/edges that existed at that timestamp (based on `created_at` and `deleted_at` fields)

3. **Ghost node rendering**:
   - Nodes that exist now but didn't exist at the scrubbed time → forward ghosts (dotted green outline)
   - Nodes that existed at the scrubbed time but don't exist now → backward ghosts (dotted red outline with strikethrough)
   - Modified nodes (exist at both times but changed) → yellow highlight

4. **Delta panel**:
   - When scrubber is not at "now", show a delta summary panel
   - Count of added/removed/modified nodes by type
   - "Between [scrubbed date] and now: +12 types, -3 types, +2 traits, 8 types modified"
   - Collapsible, appears above or beside the scrubber

5. **Key moments markers**:
   - Fetch key events from the timeline: spec approvals, milestone completions, reconciliation events
   - Show as colored dots on the scrubber track
   - Hover → tooltip with event description
   - Click → delta panel shows what changed at that moment

6. **Tests**:
   - Scrubber renders with correct date range
   - Debounced graph fetch on scrub
   - Ghost nodes rendered correctly for historical views
   - Delta summary counts match expected values
   - Key moment markers appear at correct positions

## Acceptance Criteria

- [ ] Timeline scrubber appears at bottom of canvas
- [ ] Dragging scrubber updates canvas to show historical graph state
- [ ] Ghost outlines for nodes added since (green dotted) and removed (red dotted)
- [ ] Delta summary panel shows added/removed/modified counts
- [ ] Key moments marked on timeline (spec approvals, milestones, reconciliations)
- [ ] Click key moment shows delta narrative
- [ ] Scrubber at "now" position shows current graph (no ghosts)
- [ ] Tests pass

## Agent Instructions

Read `system-explorer.md` §6 "Architectural Timeline" for the full specification. The canvas is built in task-178 — check `ExplorerView.svelte` for how to add the scrubber below the canvas. For historical graph queries, check `graph.rs` for existing query parameters on `GET /repos/:id/graph`. The ghost node visual language should match the ghost overlay styles defined in `system-explorer.md` §3 (dotted outlines, color coding). Key moments data may need a new endpoint or could be derived from existing spec approval and milestone data — check `specs.rs` for approval history and the spec lifecycle events. The `timeline` layout type is already in the view spec grammar (task-170) — the timeline scrubber is a different feature (it controls the temporal state of any layout, not a layout type itself).

## Shipped

The Architectural Timeline (system-explorer.md §6) now runs on a real
point-in-time graph API instead of a client-side `first_seen_at` guess, and
the key-moment interactions the spec requires (reconciliation markers, click
→ narrative) are wired end to end. Commit 6b9fb4e2be51bd1d6d702c959878a8f9331dc6fe.

### Backend: point-in-time graph reconstruction

- `GraphPort` gains `list_nodes_at`/`list_edges_at` with an explicit
  existed-at contract: a node/edge existed at `at` iff `first_seen_at <= at`
  AND (`deleted_at` is `None` OR `deleted_at > at`). Implemented in BOTH
  adapters — SQL filters in the SQLite adapter, the same guard in code in
  `MemGraphStore` — so the contract holds regardless of which adapter serves
  the request (mem-port-contract invariant).
- `GET /repos/{id}/graph?at=<epoch-seconds|RFC3339>` reconstructs the graph
  at that instant, including nodes soft-deleted since (invisible to
  `list_nodes`, which always excludes them — this was the gap that made
  backward-looking views impossible server-side). Invalid `at` values return
  400 rather than being silently ignored. The `time` crate's `parsing`
  feature is now an explicit dependency (it was only transitively enabled).
- Documented in `docs/api-reference.md`.

### Frontend: scrubbing, ghosts, delta panel, key moments

- Scrubbing fetches the server `?at=` graph through
  `createTimelineHistoryFetcher` (200 ms debounce, rapid positions coalesce
  into one request for the latest position, superseded positions resolve
  null so the instant client-side `first_seen_at` filter stays until the
  server graph lands). The "Now" position (slider max) shows the current
  graph with no ghosts and no fetch.
- Ghost rendering (green dotted forward / red backward / yellow modified)
  and the per-type delta summary ("+12 types, -3 types, 8 types modified")
  now run against the server-reconstructed historical graph, so nodes
  deleted after the scrub time are correctly absent rather than shown.
- Key moments now include reconciliation events: workspace
  `reconciliation_completed` messages (persona v3 → v4) are lazy-loaded on
  first time-travel activation via `GET /workspaces/{id}/messages?kind=...`
  and rendered as purple markers positioned by timestamp on the d3-scale
  axis, alongside the existing spec-approval, milestone, and structural
  change markers.
- Clicking a key moment surfaces the delta's grounded narrative: the
  TimelineScrubber popover gained a "Why" field and ExplorerView renders a
  dismissible narrative panel above the canvas (previously the marker-click
  callback was a no-op — the acceptance criterion was unimplemented).

### Test evidence

- `cargo test -p gyre-adapters --lib sqlite::graph::tests::list_` — 3 passed
  (new point-in-time node/edge suites) in this sandbox.
- `cargo check -p gyre-server --lib --tests` — clean; the two new
  gyre-server tests (`api::graph::tests::repo_graph_at_returns_point_in_time_graph`,
  `api::graph::tests::test_mem_list_nodes_at_and_edges_at_contract`) are
  deferred to the independent verifier (server test codegen exceeds the
  model-stage memory budget); exact commands recorded in
  /tmp/stage/review-evidence/task-183-evidence.md. The HTTP suite covers
  epoch and RFC 3339 `at` forms, soft-deleted-node reconstruction, and the
  400 on unparseable input.
- `cd web && npm test` — 106 files, 2151 passed | 41 skipped, including the
  extended `TimelineScrubber.test.js` (reconciliation markers, narrative
  popover) and `timeline-utils.test.js` (`resolveScrubTimestamp`,
  `createTimelineHistoryFetcher` debounce/stale/cancel suites).
- Mechanical checks pass: check-arch, abac-route-registry,
  mem-port-contracts, byte-slice-truncation, relative-path-defaults,
  inert-enforcement, api-auth.
