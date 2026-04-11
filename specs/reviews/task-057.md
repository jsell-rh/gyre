# Review: TASK-057 — Explorer Visual Regression Tests

**Reviewer:** Verifier  
**Round:** R1  
**Verdict:** `needs-revision`

---

## R1 Findings

- [ ] **F1: 8 of 15 baseline screenshots are byte-identical — filter presets, view query, and blast radius tests capture the default unfiltered canvas, not the claimed scenarios.** The MD5 hash `c06a42501c6d380951755530c2a5356b` is shared by: `filter-all`, `filter-calls`, `filter-dependencies`, `filter-endpoints`, `filter-types`, `view-query-base-graph`, `zoom-level-1-modules`, and `blast-radius-tiered`. These 8 baselines are the same pixel-identical image — the default canvas with no filters, no view query annotations, and no blast radius tiered coloring. The tests will "pass" by comparing the default state against itself, providing zero visual regression coverage for filters, view queries, or blast radius. This violates acceptance criteria 3 ("View query rendering test applies a query with groups, callouts, and narrative"), 4 ("Filter preset test verifies all 5 presets show expected subsets"), and 5 ("Blast radius test verifies tiered coloring and dimming on node click").

- [ ] **F2: Filter preset tests register route overrides AFTER page navigation — the override never fires.** In `filter_endpoints_shows_only_endpoints` (line 453-489) and all other filter tests, `navigateToExplorer(page)` is called first (line 454), which triggers page load and the views API call (handled by `setupGraphIntercept`'s empty-array response at line 179). The route override for views is registered afterward (line 458) with `{ times: 1 }`, but no subsequent navigation or API call occurs, so the override never matches. Even if the ordering were fixed, providing a saved view via the API only populates the saved views list — it does not auto-apply the view query to the canvas. The test would need to click the saved view to load it.

- [ ] **F3: View query rendering uses a non-functional custom event mechanism — no component listens for it.** The `view_query_with_groups_callouts_narrative` test (line 361) dispatches `window.dispatchEvent(new CustomEvent('explorer-apply-query', ...))` and sets `window.__testViewQuery`. Grep confirms zero files in `web/src/` listen for `explorer-apply-query` or read `window.__testViewQuery`. The view query is never applied. The test's own screenshot name — `view-query-base-graph.png` — acknowledges it captures the base graph, not a view query with groups/callouts/narrative. Acceptance criterion 3 is not satisfied.

- [ ] **F4: Blast radius tiered coloring test does not activate blast radius mode.** The `blast_radius_dimmed_unmatched_nodes` test (line 634) provides a saved view containing the blast radius query via route override, but (per F2) the route override is registered after navigation and never fires. The baseline `blast-radius-tiered-linux.png` is byte-identical to `filter-all-linux.png`, confirming no blast radius coloring was rendered. Acceptance criterion 5 is not satisfied.

- [ ] **F5: Blast radius node click test uses conditional guard — silent skip if canvas has no bounding box.** In `blast_radius_tiered_coloring_on_node_click` (line 611), the click is inside `if (box)` (line 623). If `canvas.boundingBox()` returns null (e.g., canvas not visible, zero dimensions), the click is silently skipped and the test captures the unclicked state. The test should use an assertion (`expect(box).toBeTruthy()`) before interacting, not a conditional guard.

- [ ] **F6: Dead code — `applyViewQuery()` function defined but never called.** The function at lines 254-269 is intended to apply view queries to the canvas but is never invoked by any test. All tests use either the broken custom event dispatch or the broken route-override-after-navigation pattern instead.

- [ ] **F7: Blast radius node click test does not verify blast radius behavior — it verifies node selection.** The test clicks the canvas center and captures a screenshot. Even if the click hits a node, default click behavior is node selection (highlight), not blast radius tiered coloring (red → orange → yellow → gray). Blast radius requires the blast radius view query to be active. The acceptance criterion says "Blast radius test verifies tiered coloring and dimming on node click" — clicking a node in default mode does not produce tiered coloring.
