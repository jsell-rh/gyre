---
title: "Architectural Timeline — time scrubber for historical architecture views"
spec_ref: "system-explorer.md §6"
depends_on: [task-178]
progress: ready-for-review
coverage_sections:
  - "system-explorer.md §6. Architectural Timeline"
commits: ["6b9fb4e2be51bd1d6d702c959878a8f9331dc6fe", "cf8069ea85fed1a2737e5765cd150d4ceb61627e", "8405c8e479d1d1accbe7ab78ad2448c0a77eb97e", "f5a45295b4d7bbee06549f64427dcbc291843050"]
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
point-in-time graph API. Four commits: 6b9fb4e2b (implementation),
cf8069ea8 (review round-1 repairs), 8405c8e47 (review round-2 rustfmt
repair), f5a45295b (verification repair: clippy gate).

### Point-in-time graph API (6b9fb4e2b)

- `GraphPort` gains `list_nodes_at`/`list_edges_at` with an explicit
  existed-at contract (`first_seen_at <= at` AND (`deleted_at` is None OR
  `deleted_at > at`)), implemented in BOTH adapters (SQL filters in SQLite,
  the same guard in code in `MemGraphStore`).
- `GET /repos/{id}/graph?at=<epoch-seconds|RFC3339>` reconstructs the graph
  at that instant, including nodes soft-deleted since; invalid `at` returns
  400. Documented in `docs/api-reference.md`.

### Scrubber, ghosts, delta panel, key moments (6b9fb4e2b)

- `TimelineScrubber.svelte` at the bottom of the canvas: time axis, date
  label, key-moment markers (spec approvals, milestones, structural
  changes, reconciliations) with click-through narrative popover, per-type
  delta stats ("+2 types, -1 trait, 3 types modified").
- Scrubbing fetches the server `?at=` graph through
  `createTimelineHistoryFetcher` (200 ms debounce; superseded positions
  resolve null). Ghost overlays: forward ghosts (added since, green
  dotted), backward ghosts (removed since, red dotted, recovered from
  ArchitecturalDelta records), modified highlights (yellow).

### Review repairs (cf8069ea8, 8405c8e47)

- **Fetcher race fixed** (cf8069ea8): a fetch now settles only the waiters
  for its own target, and ExplorerView refuses to pin a late out-of-order
  response; regression test reproduces the interleaving and fails on the
  old logic.
- **Reconciliation narrative grounded in the real payload** (cf8069ea8):
  the production `reconciliation_completed` payload is enriched with
  `specs: [{name, version}]` (wave-scoped provenance labels); the frontend
  narrative derives only from production fields.
- **rustfmt drift** (8405c8e47): the point-in-time tests in
  `crates/gyre-adapters/src/sqlite/graph.rs` formatted; `cargo fmt --check`
  clean on gyre-adapters and gyre-server.

### Verification repair (f5a45295b)

- Verification round de578256a9b2458a97ef8c182bb9173a (base d31906c5)
  failed the changed-lines clippy gate: 26 `clippy::double_must_use`
  findings at `crates/gyre-ports/src/graph.rs:16`. Root cause: async-trait
  0.1.89 injects `#[must_use]` into every expanded trait method while the
  boxed-future return type is already `#[must_use]`, and the lint span
  covers the whole GraphPort trait that task-183's new methods fall
  inside. Fixed with the same targeted
  `#[allow(clippy::double_must_use)]` already established on
  `SpecApprovalRepository` (spec_approval.rs) — attribute only, no
  behavior change. Evidence:
  /tmp/stage/review-evidence/clippy-double-must-use-fix.txt (focused
  `cargo clippy -p gyre-ports --all-targets` exit 0, zero findings in
  src/graph.rs; `cargo fmt --check -p gyre-ports` exit 0).

### Test evidence

- Frontend (focused, run in round 1): `vitest run timeline-utils.test.js
  TimelineScrubber.test.js ExplorerViewNavigableArchitecture.test.js`
  32/32 passed; fetcher-race regression fails on old logic (mutation
  verified).
- Rust (focused, run in round 2): `cargo test -p gyre-adapters
  sqlite::graph::tests::list_` 3/3 passed, including both point-in-time
  tests.
- Round 3 (f5a45295b) is a lint-attribute-only change: focused clippy +
  fmt probes above; the verifier should re-run the full changed-lines
  clippy gate and the listed suites.
