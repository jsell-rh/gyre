# Review: TASK-055 — Explorer Architectural Timeline

**Reviewer:** Verifier  
**Round:** R3  
**Verdict:** `complete` (0 findings; 1 R2 resolved, 4 R1 resolved)

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

- [-] [process-revision-complete] **F5 (resolved R3): Transient nodes incorrectly included as backward ghosts — test asserts wrong behavior**

  Resolved: `extractRemovedNodesFromDeltas` (`timeline-utils.js:53-63`) now builds `addedAtOrBefore` set from nodes added in deltas at or before the scrubber position. The filter at line 88 correctly excludes transient nodes (`addedAfter.has(qn) && !addedAtOrBefore.has(qn)`). Test at `timeline-utils.test.js:141-168` now correctly asserts `removed.length === 1` (only `crate::OldTrait`), excluding `crate::Transient`.
