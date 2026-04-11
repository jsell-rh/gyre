# Review: TASK-055 — Explorer Architectural Timeline

**Reviewer:** Verifier  
**Round:** R1  
**Verdict:** `needs-revision` (4 findings)

---

## R1 Findings

- [ ] **F1 (HIGH): Backward ghosts structurally impossible — `removed` always returns `[]`**

  `timelineFilteredGraph` is derived by filtering `graph.nodes` on `first_seen_at <= cutoff` (ExplorerView.svelte:984-989). Since `timelineFilteredGraph.nodes` is a strict subset of `graph.nodes`, every node in the historical graph is also in the current graph by construction.

  The backward ghost computation at ExplorerView.svelte:1052 does:
  ```js
  for (const n of timelineFilteredGraph.nodes) {
    if (!currentIds.has(n.id)) { /* push remove overlay */ }
  }
  ```
  Since `timelineFilteredGraph.nodes ⊆ graph.nodes` and `currentIds = Set(graph.nodes.map(n => n.id))`, the condition `!currentIds.has(n.id)` is NEVER true. `removed` is always `[]`. Backward ghosts can never appear.

  The same issue affects `timelineDeltaStats.removed` at line 1011.

  **Violates:**
  - Acceptance criterion: "Backward ghosts (red strikethrough) show nodes removed since the scrubber time"
  - Spec §6: "Ghost outlines show elements that... will be removed (backward ghosts)"

  **Fix direction:** To detect removed nodes, the implementation must consult the `ArchitecturalDelta` records (specifically `nodes_added` arrays in `delta_json` from deltas AFTER the scrubber position). Nodes that appear in `nodes_added` of deltas between the scrubber time and now but are NOT in the current graph were added then removed. Alternatively, if the current graph's nodes include `deleted_at` or a removal marker, filtering on that would work. The current approach of filtering the current graph by `first_seen_at` can only identify forward ghosts (added since) and modified nodes — never removed nodes, because removed nodes are absent from `graph.nodes`.

- [ ] **F2 (MED): Delta summary shows only aggregate counts — missing per-type breakdown**

  Spec §6 says: *"The sidebar shows the delta: 'Between then and now: +12 types, -3 types, +2 traits, 8 types modified'"* — the delta is categorized by node type.

  Acceptance criterion: *"Delta summary panel shows '+N types, -M types' counts"* — note the type category labels.

  Both display sites show only aggregate counts without type categories:
  - ExplorerView header (line 1241): `+{timelineDeltaStats.added}`, `-{timelineDeltaStats.removed}`, `Δ{timelineDeltaStats.modified}`
  - TimelineScrubber delta stats (line 210): `+{deltaStats.added}`, `-{deltaStats.removed}`, `Δ{deltaStats.modified}`

  Additionally, `byType` in `timelineDeltaStats` (ExplorerView.svelte:1021-1022) is computed for added nodes only — not for removed or modified — and is never rendered anywhere in the UI.

  **Violates:** Acceptance criterion "Delta summary panel shows '+N types, -M types' counts"; spec §6 per-type breakdown format.

- [ ] **F3 (MED): "Preview Mode" banner shows "predicted changes" for confirmed historical diffs**

  When timeline ghosts are active, ExplorerCanvas renders its existing "Preview Mode" bar (line 5055) with text "N predicted changes" (line 5059). Timeline ghost overlays are confirmed historical diffs (`confidence: 'confirmed'`), not predictions.

  The `timelineActive` prop is available in ExplorerCanvas (line 32) but is not used to differentiate the banner text. The user sees contradictory UI: "Preview Mode" + "predicted changes" displayed simultaneously with the timeline's "Time Travel" toggle and historical date label.

  **Violates:** Spec §6 ghost semantics — "Ghost outlines show elements that have been added since (forward ghosts) or will be removed (backward ghosts)" describes confirmed historical facts, not predictions. The banner's "predicted" language is factually incorrect for this context.

  **Fix direction:** When `timelineActive` is true and `hasGhosts` is true, the banner should display time-travel-appropriate text (e.g., "Time Travel Mode" / "N historical changes") instead of "Preview Mode" / "predicted changes."

- [ ] **F4 (MED): Compound acceptance criterion partially satisfied — no ghost overlay or delta computation test**

  Acceptance criterion: *"Component test for scrubber rendering, ghost overlay, and delta summary"*

  This compound criterion has 3 sub-items:
  1. **Scrubber rendering** — 21 tests in `TimelineScrubber.test.js` cover this adequately.
  2. **Ghost overlay** — ghost overlay computation (`timelineGhostOverlays` in ExplorerView.svelte:1031-1083) has zero test coverage. No test verifies forward/backward/modified ghost generation.
  3. **Delta summary** — delta stats display is tested via props in TimelineScrubber.test.js, but the computation logic (`timelineDeltaStats` in ExplorerView.svelte:1003-1025) has zero test coverage.

  A test for backward ghosts would have immediately revealed F1 (backward ghosts always empty).

  **Violates:** Acceptance criterion — 2 of 3 sub-items have zero computation-level test coverage.
