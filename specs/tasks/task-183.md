---
title: "Architectural Timeline — time scrubber for historical architecture views"
spec_ref: "system-explorer.md §6"
depends_on: [task-178]
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §6. Architectural Timeline"
commits: ["b7666c450c7dc70983886653e3b1956200dc48df", "f793571c271a16432ba5dda02865ce2258a39996", "c8e33c3c33c38a3f4aaabf8998d02c98fa8d946f", "f5a45295b4d7bbee06549f64427dcbc291843050", "8405c8e479d1d1accbe7ab78ad2448c0a77eb97e", "cf8069ea85fed1a2737e5765cd150d4ceb61627e", "6b9fb4e2be51bd1d6d702c959878a8f9331dc6fe"]
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

The Architectural Timeline (system-explorer.md §6) is implemented on a
real point-in-time graph API, with both review-repair findings (F1, F2)
closed in b7666c450.

### Behavior

- **Point-in-time graph API:** `GraphPort` gains
  `list_nodes_at`/`list_edges_at` with an explicit existed-at contract
  (`first_seen_at <= at` AND not soft-deleted at or before `at`),
  enforced in BOTH adapters (SQL filters in SQLite, the same guard in
  code in `MemGraphStore`). `GET /repos/{id}/graph?at=<epoch-seconds|
  RFC3339>` reconstructs the graph at that instant, including nodes
  soft-deleted since; invalid `at` returns 400. Documented in
  `docs/api-reference.md`.
- **Scrubber, ghosts, delta panel, key moments:**
  `TimelineScrubber.svelte` at the bottom of the canvas (d3-scale time
  axis, date label, key-moment markers with click-through narrative
  popover, per-type delta stats). Scrubbing fetches the server `?at=`
  graph through `createTimelineHistoryFetcher` (200 ms debounce;
  superseded positions resolve null; late out-of-order responses are
  refused by ExplorerView). Ghost overlays: forward ghosts (added since,
  green dotted), backward ghosts (removed since, red dotted with
  strikethrough, recovered from ArchitecturalDelta records), modified
  highlights (yellow). Key moments derive from real payloads: spec
  approvals, milestone completions, structural changes, and
  reconciliation events (the `reconciliation_completed` payload carries
  `specs: [{name, version}]` provenance labels for the narrative).

### Review repair (b7666c450)

- **F1 — backward ghosts now render on the canvas.**
  `computeTimelineGhostOverlays` keys each remove ghost by the
  historical node's REAL id when the server `?at=` reconstruction
  resurrects it in the scrubbed-time graph (synthetic
  `removed:<qualified_name>` remains only as the client-side-filter
  fallback). ExplorerCanvas resolves remove ghosts through
  `layoutNodeMap.get(ghost.id)`, so the red-dotted outline +
  strikethrough previously never drew for timeline ghosts.
- **F2 — colliding reconciliation markers are no longer discarded.**
  The key-moment dedupe now runs within event type (same-type moments
  within 1 index still merge), and markers sharing a track position
  stack vertically (10 px steps) so both stay visible and clickable —
  a reconciliation wave completing right after its tasks land a large
  structural delta is the expected collision, and its narrative was
  previously unreachable.

### Test evidence

- `web/src/__tests__/timeline-utils.test.js` — remove-ghost id keying
  (real id from the historical graph; synthetic-id fallback), delta
  stats, fetcher race/debounce.
- `web/src/__tests__/TimelineScrubber.test.js` — collision fixture
  (reconciliation + structural moment at the same index): both markers
  render, stack vertically, and the reconciliation narrative is
  reachable by click; same-type moments still dedupe.
- `web/src/__tests__/ExplorerCanvas.test.js` — rendering-level F1
  regression: rendering the canvas with the exact overlay set
  `computeTimelineGhostOverlays` produces for a historical scrub
  position draws the `[4, 4]` red-dotted outline followed by the
  solid-red strikethrough (fails on the pre-fix synthetic id).
- Full frontend suite: 111 files, 2263 passed / 41 skipped
  (`npm test`, vitest).
