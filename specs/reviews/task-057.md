# Review: TASK-057 — Explorer Visual Regression Tests

**Reviewer:** Verifier  
**Round:** R2  
**Verdict:** `needs-revision`

---

## R1 Findings

- [-] [process-revision-complete] **F1 (resolved R2): 8 of 15 baseline screenshots are byte-identical — filter presets, view query, and blast radius tests capture the default unfiltered canvas, not the claimed scenarios.** Resolved: the rewrite uses `applyQueryViaEditor()` (real UI interactions) to apply view queries. 13 of 15 baselines are now visually distinct. Only `zoom-level-1-modules` and `filter-all` share an MD5 hash — see R2-F1 below.

- [-] [process-revision-complete] **F2 (resolved R2): Filter preset tests register route overrides AFTER page navigation — the override never fires.** Resolved: `setupGraphIntercept()` is now in `beforeEach()` before navigation. Filter tests use `applyQueryViaEditor()` instead of route overrides.

- [-] [process-revision-complete] **F3 (resolved R2): View query rendering uses a non-functional custom event mechanism — no component listens for it.** Resolved: tests use `applyQueryViaEditor()` with real UI interactions (open editor, fill textarea, click Run Query). No custom events or window globals.

- [-] [process-revision-complete] **F4 (resolved R2): Blast radius tiered coloring test does not activate blast radius mode.** Resolved: blast radius query is now applied via `applyQueryViaEditor()`. Baseline `blast-radius-tiered-linux.png` is visually distinct from `filter-all-linux.png` (4820 vs 10255 bytes).

- [-] [process-revision-complete] **F5 (resolved R2): Blast radius node click test uses conditional guard — silent skip if canvas has no bounding box.** Resolved: line 546 now uses `expect(box).toBeTruthy()` assertion instead of conditional guard.

- [-] [process-revision-complete] **F6 (resolved R2): Dead code — `applyViewQuery()` function defined but never called.** Resolved: function removed. `applyQueryViaEditor()` replaces it.

- [-] [process-revision-complete] **F7 (resolved R2): Blast radius node click test does not verify blast radius behavior — it verifies node selection.** Resolved: blast radius query is now applied via `applyQueryViaEditor()` before clicking. The fixed focus query with `tiered_colors` produces tiered coloring. See R2-F2 for remaining comment staleness.

---

## R2 Findings

- [ ] **R2-F1: `zoom-level-1-modules` baseline is byte-identical to `filter-all` — the "mid zoom" test captures the default state with no zoom adjustment.** Both screenshots share MD5 hash `f0f44edb9a66fdd867a34fbb970e271a` (10255 bytes each). The `zoom_level_1_modules` test (line 331-339) explicitly says "Default zoom shows the full graph — no zoom adjustment needed" and performs no zoom interaction. The `filter_all_shows_complete_graph` test (line 414-422) also captures the default state with no modifications. These two tests capture the exact same visual state. The acceptance criterion says "Semantic zoom test captures **3** zoom levels" — only 2 visually distinct zoom levels are captured (level 0 with wheel-zoom-out at 3754 bytes, level 2 with wheel-zoom-in at 6209 bytes). Level 1 adds no additional coverage.

- [ ] **R2-F2: Stale comment in `blast_radius_tiered_coloring_on_node_click` claims `$clicked` mode but query uses a fixed node.** Lines 536-538 say "The blast radius query uses $clicked as scope.node, which makes ExplorerCanvas store it as an interactive query template. The tiered coloring only activates after clicking a node." But `BLAST_RADIUS_QUERY` (mock-graph.js:347) sets `node: 'fn-spawn-agent'` — a fixed node, not `$clicked`. The query does NOT become an interactive template and the tiered coloring activates immediately from the fixed focus query, not from clicking. The click on line 549 does not trigger blast radius BFS re-evaluation — it produces node selection behavior instead. The spec says "Blast radius **interactive** mode" and the acceptance criterion says "on node click" — but the test's tiered coloring is from a fixed focus query, not the interactive `$clicked` mechanism. The baselines confirm tiered coloring IS rendered (visually distinct screenshots), so the visual regression value is present, but the stale comment misrepresents the mechanism and the interactive `$clicked` mode is not exercised.
