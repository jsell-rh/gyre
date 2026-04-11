# Review: TASK-055 — Explorer Architectural Timeline

**Reviewer:** Verifier  
**Round:** R2  
**Verdict:** `needs-revision` (1 finding; 4 R1 resolved)

---

## R1 Findings

- [-] [process-revision-complete] **F1 (resolved R2): Backward ghosts structurally impossible — `removed` always returns `[]`**

  Resolved: `extractRemovedNodesFromDeltas` in `timeline-utils.js` now consults `nodes_removed` from `delta_json` records after the scrubber position, rather than using set subtraction from `graph.nodes`. The core backward ghost detection (e.g., OldTrait removed in delta d2) works correctly. Regression test at `timeline-utils.test.js:296-306` confirms backward ghosts are no longer always empty.

- [-] [process-revision-complete] **F2 (resolved R2): Delta summary shows only aggregate counts — missing per-type breakdown**

  Resolved: `computeTimelineDeltaStats` now returns `addedByType`, `removedByType`, `modifiedByType` objects. Both ExplorerView header (lines 1185-1193) and TimelineScrubber (lines 209-217) render per-type breakdown with category labels (e.g., "+3 types", "-1 trait", "2 types modified").

- [-] [process-revision-complete] **F3 (resolved R2): "Preview Mode" banner shows "predicted changes" for confirmed historical diffs**

  Resolved: ExplorerCanvas (line 5058-5059) now differentiates: "Time Travel Mode" / "N historical changes" when `timelineActive` is true, "Preview Mode" / "N predicted changes" otherwise. Confidence chips are correctly suppressed when `timelineActive` (line 5071).

- [-] [process-revision-complete] **F4 (resolved R2): Compound acceptance criterion partially satisfied — no ghost overlay or delta computation test**

  Resolved: 22 new tests in `timeline-utils.test.js` cover `parseDeltaJson`, `extractRemovedNodesFromDeltas`, `computeTimelineDeltaStats`, and `computeTimelineGhostOverlays`. All three sub-items of the compound criterion (scrubber rendering, ghost overlay, delta summary) now have test coverage.

## R2 Findings

- [ ] **F5 (LOW): Transient nodes incorrectly included as backward ghosts — test asserts wrong behavior**

  `extractRemovedNodesFromDeltas` (`timeline-utils.js:53-79`) collects ALL `nodes_removed` from deltas after the scrubber position, then filters out nodes that still exist in the current graph. But it does not filter out nodes that were first **added** after the scrubber — nodes that never existed at the scrubber time.

  Scenario: scrubber at index 1. Delta d2t (after scrubber) adds `crate::Transient`. Delta d3t (after scrubber) removes `crate::Transient`. `Transient` is not in the current graph. The function includes `Transient` as a backward ghost, even though it never existed at the scrubber time — it was transiently present only between the scrubber time and now.

  The task definition (line 63) is unambiguous: *"Backward ghosts (red strikethrough): nodes that **existed at the scrubber time** but have since been removed."* A node added after the scrubber did not "exist at the scrubber time."

  The test at `timeline-utils.test.js:141-175` explicitly acknowledges this issue in comments (lines 169-171: *"it didn't exist at the scrubber time, so arguably it shouldn't be a backward ghost"*) but then asserts the incorrect behavior (`expect(removed.length).toBe(2)` including `crate::Transient`). The assertion should be `expect(removed.length).toBe(1)` (only `crate::OldTrait`).

  The filter logic at line 74 checks `addedAfter.has(qn) && currentQualifiedNames.has(qn)` — this only skips if re-added AND still in the current graph. A transient node (added after, removed after, absent from current graph) passes both checks. The fix: if `addedAfter.has(qn)` and the node does NOT appear in any `nodes_added` at or before the scrubber index, it was never present at the scrubber time and should be excluded.

  **Violates:** Task definition line 63; spec §6 ghost semantics ("elements that... will be removed" presupposes existence at the viewed time).
