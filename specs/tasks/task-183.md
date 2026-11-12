---
title: "Architectural Timeline — time scrubber for historical architecture views"
spec_ref: "system-explorer.md §6"
depends_on: [task-178]
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §6. Architectural Timeline"
commits: ["cf8069ea85fed1a2737e5765cd150d4ceb61627e", "6b9fb4e2be51bd1d6d702c959878a8f9331dc6fe"]
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

The Architectural Timeline (system-explorer.md §6) runs on a real
point-in-time graph API, and the two defects from review round
1dd7441246b74416b1bc06d19d55520e are repaired. Commits:
6b9fb4e2b (original implementation) and cf8069ea8 (review repairs).

### Point-in-time graph API (6b9fb4e2b)

- `GraphPort` gains `list_nodes_at`/`list_edges_at` with an explicit
  existed-at contract (`first_seen_at <= at` AND (`deleted_at` is None OR
  `deleted_at > at`)), implemented in BOTH adapters (SQL filters in SQLite,
  the same guard in code in `MemGraphStore`).
- `GET /repos/{id}/graph?at=<epoch-seconds|RFC3339>` reconstructs the graph
  at that instant, including nodes soft-deleted since; invalid `at` returns
  400. Documented in `docs/api-reference.md`.

### Scrubber, ghosts, delta panel, key moments (6b9fb4e2b)

- `TimelineScrubber.svelte` at the bottom of the canvas: d3-scale time
  axis, date label, key-moment markers (spec approvals, milestones,
  structural changes, reconciliations) with click-through narrative
  popover, per-type delta stats ("+2 types, -1 trait, 3 types modified").
- Scrubbing fetches the server `?at=` graph through
  `createTimelineHistoryFetcher` (200 ms debounce; superseded positions
  resolve null so the client-side `first_seen_at` filter is the instant
  fallback). Ghost overlays: forward ghosts (added since, green dotted),
  backward ghosts (removed since, red dotted, recovered from
  ArchitecturalDelta records because removed nodes are absent from the
  current graph), modified highlights (yellow).

### Review repairs (cf8069ea8)

- **Fetcher race fixed**: a fetch completion used to settle waiters matched
  against `latestAt`, so scrubbing again while a fetch was in flight
  resolved the newer position with the older position's graph, pinning the
  wrong point in time on the canvas. A fetch now settles only the waiters
  for its own target, and ExplorerView refuses to pin a late out-of-order
  response whose position is no longer current. Regression test reproduces
  the exact interleaving and fails on the old logic (verified by mutation:
  1 failed on old code, 32/32 on fixed).
- **Reconciliation narrative grounded in the real payload**: the narrative
  no longer reads fields no emitter sends (persona_name,
  specs_changed, ...). The production `reconciliation_completed` payload is
  enriched with `specs: [{name, version}]` — the changed meta-specs of the
  current wave, materialized as structured provenance labels on each wave
  task at creation (`meta-spec:{name}@v{version}` plus a
  `recon-wave:{millis}` wave identity threaded through rolling-rollout
  pending state, so prior waves cannot bleed into a later wave's event).
  The frontend narrative derives only from production fields
  (task_count/duration_secs/specs), with a true fallback for pre-label
  waves. Tests pin the exact production payload shape (frontend fixture +
  server payload assertions incl. wave-2-scoped specs).
- cargo-fmt drift in `graph.rs` from the original candidate commit fixed
  (would fail the CI rustfmt job).

### Test evidence

- Frontend (focused, run here): `vitest run timeline-utils.test.js
  TimelineScrubber.test.js ExplorerViewNavigableArchitecture.test.js
  ExplorerViewLivenessWiring.test.js canvas-state.test.js` — 97/97 pass;
  both repaired defects have mutation-verified regression tests (each
  fails on the pre-fix code).
- Server (type-checked here via `cargo check -p gyre-server --tests`, fmt
  and diff-scoped clippy clean): the verifier must run
  `cargo test -p gyre-server --lib reconciliation` — covers the label
  round-trips, provenance-label assertions on created tasks, the payload
  specs assertions in `reconciliation_completed_fires_when_all_tasks_terminal`,
  and wave scoping in `second_wave_emits_its_own_completion`.
