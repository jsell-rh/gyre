---
title: "Architectural Timeline — time scrubber for historical architecture views"
spec_ref: "system-explorer.md §6"
depends_on: [task-178]
progress: complete
coverage_sections:
  - "system-explorer.md §6. Architectural Timeline"
commits: ["eabc1f5c1849b9b2e7100647f5cd45b14a746ff7", "b7666c450c7dc70983886653e3b1956200dc48df", "f793571c271a16432ba5dda02865ce2258a39996", "c8e33c3c33c38a3f4aaabf8998d02c98fa8d946f", "f5a45295b4d7bbee06549f64427dcbc291843050", "8405c8e479d1d1accbe7ab78ad2448c0a77eb97e", "cf8069ea85fed1a2737e5765cd150d4ceb61627e", "6b9fb4e2be51bd1d6d702c959878a8f9331dc6fe", "b2d1ec4b850950e2fae50376595b84702feb9760"]
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
real point-in-time graph API. This round resolved the rebase against
current main (`4dc0eb9`, task-139 forge enforcement policies) via merge
commit `f89cfe9b`, preserving the retained implementation intact on top
of main's spec-policy surface.

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
  strikethrough, keyed by the historical node's real id when the `?at=`
  reconstruction resurrects it), modified highlights (yellow). Key
  moments derive from real payloads: spec approvals, milestone
  completions, structural changes, and reconciliation events (the
  `reconciliation_completed` payload carries `specs: [{name, version}]`
  provenance labels for the narrative); colliding markers stack
  vertically so every narrative stays reachable.

### Rebase resolution (f89cfe9b)

- Merged main `4dc0eb9` (task-139: `require_signed_approval` spec
  policy, migration 000059, gate/merge-processor enforcement) into this
  branch. Overlap was limited to `docs/api-reference.md` (both sides'
  route docs combined) and `web/dist` (taken from main; regenerated
  assets are owned by the executor/remote integration). No source file
  required semantic reconciliation — task-183 and task-139 touch
  disjoint Rust modules.

### Test evidence

- `cargo test -p gyre-adapters --lib graph::tests` — 22 passed on the
  merged tree, including `list_nodes_at_reconstructs_point_in_time` and
  `list_edges_at_reconstructs_point_in_time` (SQL existed-at filters).
- `npx vitest run` on `TimelineScrubber.test.js`, `timeline-utils.test.js`,
  `ExplorerCanvas.test.js` — 226 passed (remove-ghost id keying and
  red-dotted strikethrough stroke sequence, marker-collision stacking
  and reconciliation-narrative reachability, fetcher race/debounce,
  delta stats); `RepoSettings.test.js` (main's task-139 surface) — 64
  passed, confirming the merge did not regress main's frontend.
- `cargo check -p gyre-server -p gyre-adapters -p gyre-ports -p gyre-domain`
  — clean on the merged tree.
- Mechanical checks on the merged tree: `check-arch.sh`,
  `check-abac-route-registry.sh`, `check-migration-versions.sh`,
  `check-mem-port-contracts.sh` — all pass.
- Deferred to independent verifier (gyre-server test codegen exceeds
  the sandbox build budget): `cargo test -p gyre-server --lib
  api::graph::tests` — covers `repo_graph_at_returns_point_in_time_graph`
  (HTTP `?at=` suite: epoch + RFC3339 forms, 400 on invalid, resurrected
  soft-deleted nodes) and `test_mem_list_nodes_at_and_edges_at_contract`
  (mem adapter existed-at contract).
