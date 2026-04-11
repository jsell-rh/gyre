# TASK-055: Explorer Architectural Timeline — Time Scrubber & History Navigation

**Spec reference:** `system-explorer.md` §6 (Architectural Timeline)  
**Depends on:** TASK-029 (lens toggle, complete), TASK-031 (drill-down, complete)  
**Progress:** `ready-for-review`

## Spec Excerpt

From `system-explorer.md` §6:

> A time-scrubber at the bottom of the canvas. Drag it to see the system at any point in history.
>
> **What changes as you scrub:**
> - The canvas shows the knowledge graph at that point in time
> - Ghost outlines show elements that have been added since (forward ghosts) or will be removed (backward ghosts)
> - The sidebar shows the delta: "Between then and now: +12 types, -3 types, +2 traits, 8 types modified"
>
> **Key moments** are marked on the timeline:
> - Spec approvals (spec X approved → implementation began)
> - Milestone completions
> - Reconciliation events (persona v3 → v4)
> - Major structural changes (new crate added, trait split)
>
> Click a key moment → the delta panel shows what changed and why (narrative from the knowledge graph).

## Current State

- **Backend endpoints exist:**
  - `GET /api/v1/repos/:id/graph/timeline?since=<epoch>` — returns `ArchitecturalDelta` records with timestamps
  - `GET /api/v1/repos/:id/graph/diff` — returns structural diff between graph snapshots
- **No frontend time-scrubber component exists.** The Explorer canvas has no temporal navigation.
- **Ghost overlay rendering** exists for spec prediction (`graph/predict` ghost nodes), but not for historical diffs.
- **`ArchitecturalDelta` records** are stored on every push-triggered graph extraction, providing the raw data for key moments.

## Implementation Plan

### Part A: Time Scrubber Component

1. **Create `TimelineScrubber.svelte`** — a horizontal range slider at the bottom of the Explorer canvas.
   - Time range: from the repo's oldest `ArchitecturalDelta` timestamp to now.
   - Key moment markers rendered as dots/ticks on the timeline bar.
   - Dragging the scrubber updates the canvas to show the graph at that historical point.
   - Current position labeled with human-readable date/time.

2. **Fetch timeline data** — On mount, call `GET /api/v1/repos/:id/graph/timeline` to retrieve all `ArchitecturalDelta` records. Extract timestamps and event types for marker placement.

3. **Key moment markers** — Render distinct markers for:
   - Spec approvals (from spec approval ledger events)
   - Graph extraction events with significant node count changes
   - Reconciliation events
   - Markers are clickable — clicking opens the delta detail.

### Part B: Historical Graph Rendering

1. **Graph-at-time-T** — When the scrubber is at a historical position, filter the current graph by walking `ArchitecturalDelta` records backwards:
   - Start with current graph nodes/edges.
   - For each delta between now and the scrubber position, reverse `nodes_added` (remove them) and reverse `nodes_modified` (restore old values).
   - Render the resulting historical graph on the canvas.

2. **Ghost overlays for changes** — Show ghost outlines on the historical view:
   - **Forward ghosts** (dotted green outlines): nodes that exist now but didn't exist at the scrubber time (will be added in the future relative to the scrubber).
   - **Backward ghosts** (red strikethrough): nodes that existed at the scrubber time but have since been removed.
   - **Modified highlights** (yellow border): nodes that exist at both times but have changed.

3. **Delta summary panel** — A collapsible panel showing the diff summary:
   - "+N types, -M types, +P traits, Q types modified"
   - Clickable entries navigate to the affected node on the canvas.

### Part C: Key Moment Detail

1. **Click a key moment marker** on the timeline → show a detail popover:
   - Event type (spec approval, extraction, reconciliation)
   - Timestamp, agent, spec reference
   - Delta summary (nodes added/modified/removed)
   - Narrative text from the knowledge graph (if available)
   - "View full diff" button opens the delta in the detail panel.

## Acceptance Criteria

- [ ] Time scrubber renders at the bottom of the Explorer canvas
- [ ] Dragging the scrubber updates the graph to show historical state
- [ ] Key moment markers are visible and clickable on the timeline
- [ ] Forward ghosts (green dotted) show nodes added since the scrubber time
- [ ] Backward ghosts (red strikethrough) show nodes removed since the scrubber time
- [ ] Modified nodes highlighted with yellow border
- [ ] Delta summary panel shows "+N types, -M types" counts
- [ ] Clicking a key moment shows event detail (type, timestamp, agent, spec, delta)
- [ ] Scrubber handles repos with no deltas gracefully (hidden or disabled)
- [ ] Component test for scrubber rendering, ghost overlay, and delta summary
- [ ] `npm test` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `web/src/lib/ExplorerCanvas.svelte` for the current canvas rendering
3. Read `web/src/lib/ExplorerView.svelte` for the container layout (where to place the scrubber)
4. Read `crates/gyre-server/src/api/graph.rs` — find `get_graph_timeline` and `get_graph_diff` handlers to understand the response shapes
5. Read the `ArchitecturalDelta` type in `gyre-common` for the delta structure
6. Check the existing ghost overlay implementation (from spec prediction) for visual consistency
7. Implement Part A (scrubber) first, then Part B (historical rendering + ghosts), then Part C (key moments)
8. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- `feat(explorer): add architectural timeline scrubber (TASK-055)`
