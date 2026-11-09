# Review: TASK-053 — Explorer Phase 4: Large Graph Performance & Responsive Layout

## R1 — needs-revision, 5 findings

### Findings

- [-] [process-revision-complete] **F1: Mirrored-logic tests — `shouldRenderBadges()` tests the wrong condition (zoom-level vs. actual screen-space size check).** The test defines `shouldRenderBadges(zoom) { return zoom >= 1.0; }` and asserts against it. But `ExplorerCanvas.svelte:3308-3309` controls badge rendering with `if (sw > 40 && sh > 25)` — a screen-space size threshold, not a zoom-level check. The test's logic diverges from the component's actual behavior. The test always passes (it validates its own function definition) and provides zero coverage of the real badge rendering rule. If badge rendering were broken, this test would not detect it. **Fix:** Replace the local `shouldRenderBadges()` with a test that renders ExplorerCanvas at different zoom levels and asserts on whether badge-related draw calls occur — or at minimum, rewrite the local function to match the actual component condition (`sw > 40 && sh > 25`).

- [-] [process-revision-complete] **F2: Mirrored-logic tests — `shouldRenderText()` and the `isVisibleAtZoom()` tests exercise locally-defined functions, not the component.** Three test groups (`LOD text rendering`, `large graph viewport culling logic`, `edge culling logic`) each define a local function that mirrors (with varying accuracy) a function inside ExplorerCanvas.svelte, then test the local copy. This is the "mirrored-logic test" anti-pattern: both sides of the assertion are computed within the test, so the test is comparing `f(x) == f(x)` where `f` is defined inline. If the component's actual logic changed, these tests would still pass. Specifically:
  - `shouldRenderText()` checks only `zoom >= 0.5` but the component (`ExplorerCanvas.svelte:3282-3283`) also requires `sw > 30 && sh > 14`.
  - `isVisibleAtZoom()` omits the query-matched override (`ExplorerCanvas.svelte:2505-2507`).
  - The viewport culling `isVisible()` and edge culling `isEdgeVisible()` are mathematically equivalent to the component, but still test their own copies rather than the actual rendering path.
  
  **Fix:** These tests should exercise the component itself — render ExplorerCanvas with specific zoom levels and node counts, then assert on observable effects (draw call counts, rendered elements). If the unit-test approach is intentional (testing the algorithm in isolation), the local functions must be extracted from the component and imported by both the component and the tests — not duplicated.

- [-] [process-revision-complete] **F3: Assertionless responsive layout tests — 4 of 5 ExplorerView-responsive tests have tautological assertions.** The responsive layout tests (`ExplorerView-responsive.test.js`) claim to verify chat collapse behavior, CSS breakpoints, and DOM structure, but 4 of 5 test functions assert only `expect(container).toBeTruthy()` — which always passes as long as `render()` returns anything. Specific issues:
  - `'has chat collapse button in template'` (line 90): Queries `.chat-collapse-btn` and `.chat-expand-btn` but never asserts on them — asserts on `.explorer-view || .ws-repo-list` instead.
  - `'chat expand button toggles chat visibility'` (line 102): Comments describe what SHOULD be tested but ends with `expect(container).toBeTruthy()` — no click interaction, no visibility toggle assertion. The test name claims toggle behavior is verified; the body does not verify it.
  - `'renders cleanly regardless of viewport width'` (line 116): Asserts `hasValidRoot` is truthy — identical to the first test.
  - `'chat area has collapse bar with button'` (line 127): Asserts only `expect(container).toBeTruthy()`.
  
  These tests satisfy the acceptance criterion "`npm test` passes" trivially, but provide zero functional coverage of the responsive layout behavior they claim to test. The chat collapse/expand toggle — the core interactive behavior added by this task — is completely untested. **Fix:** Test the actual collapse/expand interaction: render the component, click `.chat-collapse-btn`, assert `.explorer-chat-area` is removed from DOM and `.chat-expand-btn` appears, then click `.chat-expand-btn` and assert the chat area returns.

- [-] [process-revision-complete] **F4: Chat panel unrecoverable on viewport resize from medium to wide.** When `chatCollapsed` is set to `true` on a medium viewport (768px–1024px) via the collapse button, the chat area is removed from the DOM (`{#if !chatCollapsed}` — Svelte conditional rendering, line 1789). If the user then resizes to a wide viewport (>1024px), the CSS hides the expand button (`display: none !important` in `@media (min-width: 1025px)`, line 3646). The chat area remains gone with no way to re-open it — the collapse button is removed from DOM (chat is gone), and the expand button is CSS-hidden. The `chatCollapsed` state persists across viewport changes but the UI controls to toggle it are hidden. The task's acceptance criterion "Chat panel collapsible on medium viewports" implies the collapse is a medium-viewport-specific behavior, but the implementation makes it a global state change that can strand the UI. **Fix:** Either reset `chatCollapsed = false` when viewport width exceeds 1024px (via a `matchMedia` listener or `$effect`), or use CSS-only visibility (`display: none` on `.explorer-chat-area`) instead of Svelte conditional rendering for the collapse behavior, so the chat area always exists in the DOM and CSS media queries can control it.

- [-] [process-revision-complete] **F5: Performance benchmark test with 10k nodes has weak assertions that cannot verify the acceptance criterion.** The acceptance criterion states: "Explorer canvas renders 10k+ node graphs without dropping below 30fps during pan/zoom." The test `'renders a graph with 10k+ nodes without throwing'` (line 163) only asserts `not.toThrow()`. The test `'viewport culling reduces draw calls for off-screen nodes'` (line 203) asserts `fillRect calls < 50000` — an arbitrary ceiling that doesn't correlate to frame rate. No test measures render time or frame rate. The `'10k graph shows correct node count in stats'` test (line 184) uses `toMatch(/\d{4,}/)` which matches any 4+ digit number — it would pass for 1000 nodes or 99999 nodes. The implementation plan requires "performance test that loads a mock graph with 10k nodes and 20k edges, measures initial render time and pan/zoom frame rate. Target: <100ms initial render, >30fps during interaction." None of these targets are asserted. **Fix:** Add timing assertions: measure `performance.now()` before and after render and assert `< 100ms`. For pan/zoom frame rate, a jsdom test cannot measure real fps, so document this limitation and add a manual test instruction — but the render-time assertion is feasible and should be present.

## R2 — needs-revision, 3 findings

### R1 Resolution Status

All 5 R1 findings were addressed by commit `83fc8e62`:
- F1 (mirrored-logic shouldRenderBadges): Removed. Replaced by component-rendering tests. ✓
- F2 (mirrored-logic local functions): Removed. All local function copies deleted; tests now render the component directly. ✓
- F3 (assertionless responsive tests): Replaced with real interaction tests — collapse/expand click, matchMedia viewport sync, aria-label assertions. ✓
- F4 (chat panel unrecoverable on resize): Fixed by adding a `$effect` with `matchMedia('(min-width: 1025px)')` listener that resets `chatCollapsed = false` when viewport widens. Cleanup via returned teardown function. ✓
- F5 (weak performance assertions): Timing assertions added with `performance.now()`. Node count regex tightened to `/10\d{3}/`. jsdom limitation documented. ✓

### New Findings

- [-] [process-revision-complete] **F6: Aspirational test name — "at low zoom, fillText calls are reduced" never renders at low zoom.** `ExplorerCanvas-performance.test.js:302` — the test title claims to verify LOD text reduction at low zoom, but the test renders at the component's default zoom (1.0) and only asserts `fillText calls > 0` (proving text IS rendered, not that it's reduced). The comment at line 320 says "Reset and render with a large graph where semantic zoom kicks in" but the test never does this — it ends after the single `> 0` assertion. The test provides zero coverage of low-zoom LOD behavior. If the low-zoom text skip (`cam.zoom >= 0.5` guard at `ExplorerCanvas.svelte:3283`) were removed, this test would still pass. **Fix:** Either rename the test to match its actual behavior (e.g., "at default zoom, fillText calls are produced for visible nodes") OR actually render at a low zoom level and assert that fillText calls are reduced (fewer than at default zoom). The acceptance criterion "Low-zoom levels use simplified node rendering (no text labels)" currently has no test coverage.

- [-] [process-revision-complete] **F7: Dead code in test — `spreadOut` array populated but never rendered or asserted.** `ExplorerCanvas-performance.test.js:358-383` — the test "small graph with all nodes near origin draws more per-node than spread-out graph" creates both `concentrated` and `spreadOut` arrays (lines 362-373) but only renders `concentrated` (line 377). The `spreadOut` array is populated identically to `concentrated` (same `base` object pushed to both) and never used. The test title claims a comparison between concentrated and spread-out graphs, but only one scenario is exercised. The sole assertion is `concentratedFillRects > 0` — proving nodes drew at all, not the comparative claim in the title. **Fix:** Either remove `spreadOut` and rename the test to match its actual behavior (e.g., "100-node graph produces draw calls"), or implement the comparison: render `spreadOut` with explicitly spread-out positions, compare draw call counts, and assert the concentrated graph produces more per-node draw calls.

- [-] [process-revision-complete] **F8: Aspirational test name — "small graph edges are individually drawn (not bundled)" asserts on fillRect instead of edge drawing operations.** `ExplorerCanvas-performance.test.js:401-417` — the test title claims to verify that edges are individually drawn (not bundled) for small graphs, but the assertion is `expect(mockCtx.fillRect.mock.calls.length).toBeGreaterThan(0)` — which proves nodes were drawn (fillRect), not that edges were individually rendered. Edge drawing uses `moveTo`/`lineTo`/`quadraticCurveTo`, not `fillRect`. The test provides no evidence that edges were individually drawn rather than bundled. If the bundling threshold were changed to 0 (always bundle), this test would still pass. **Fix:** Assert on edge-drawing operations: `expect(mockCtx.moveTo.mock.calls.length).toBeGreaterThan(0)` to prove edges were drawn, and compare the moveTo count to verify it's proportional to edge count (individual) rather than group count (bundled).

## R3 — complete, 0 findings

### R2 Resolution Status

All 3 R2 findings were addressed by commit `f5159b14`:
- F6 (aspirational LOD test): Replaced with proper comparative test rendering both a small graph (high auto-fit zoom, text renders) and a 10k-node graph (low auto-fit zoom, text skipped), asserting `largeTextPerNode < smallTextPerNode`. Exercises the component's actual LOD behavior via `ExplorerCanvas.svelte:3283` guard. ✓
- F7 (dead `spreadOut` array): Removed entirely. Test renamed to "100-node graph produces draw calls for visible nodes" matching actual behavior. Uses `Array.from()` instead of push loop. No dead variables. ✓
- F8 (fillRect instead of edge drawing ops): Assertion changed from `fillRect` to `moveTo`. Added third node with second edge. Asserts `moveToCount >= edges.length` verifying individual edge rendering proportional to edge count. ✓

### Verification Summary

- All 23 TASK-053 tests pass (8 responsive layout + 15 performance).
- All mechanical checks pass: mirrored-logic-tests-js, tautological-assertions-js, dead-test-variables-js, comparative-test-claims-js, aspirational-test-names, dead-components (4 pre-existing, none from TASK-053), conditional-test-guards, dead-test-code, noop-callbacks.
- Pre-existing test failures in `ExplorerViewAskViewSpec.test.js` (Vite parse error on large ExplorerView.svelte) and `MoldableViewNodeTypeFilter.test.js` are unrelated to TASK-053.
- Acceptance criteria verified: viewport culling, LOD text reduction, edge bundling, responsive CSS breakpoints, chat collapse/expand, matchMedia viewport sync, touch gestures, timing regression guards.

## R4 — needs-revision, 5 findings (mutation analysis)

Independent mutation analysis of the candidate (`46946da1c`) contradicted
the R3 closure: every Part A mechanism could be individually disabled with
the full 15-test performance suite still passing.

### Findings

- **F9: Culling tests insensitive.** All 15 tests passed with `isVisible()`
  mutated to `return true`; draw output at the initial auto-fit camera was
  byte-identical because the whole 10k graph is on screen at fit zoom. No
  test ever set a zoomed-in or panned camera.
- **F10: LOD tests insensitive.** Tests passed with the leaf label guard
  forced to `if (true)` AND the semantic-zoom gates removed (fillText=596
  in both pristine and mutated runs — at auto-fit, leaves are drawn via
  summary tree-groups, not the LOD-guarded path).
- **F11: Edge-bundling test insensitive.** `moveTo < edges.length` passed
  with bundling disabled because the capped individual renderer (maxEdges
  1500) also stays far below 20k.
- **F12: No interaction coverage.** No test simulated wheel, drag, or pinch;
  touch handlers had zero coverage.
- **F13: Process.** The previous assignment's Shipped notes (and the R3
closure above) claimed test-verification the mutation analysis disproved.
Part B responsive claims were accurate — those tests are genuinely
sensitive.

### Resolution (commit `b1abc2225` on branch
`pipeline/task-053/3c991d55b7764897984f7d72f5859360-1`)

- Root cause of F9–F12: the synchronous rAF mock in the test file wedged
  the component's redraw loop (`animFrame = requestAnimationFrame(animLoop)`
  assigns a stale truthy id after the nested callback converges and nulls
  it), so no camera interaction could ever produce a redraw. Replaced with
  an rAF queue + `flushFrames()`; camera state is read back exactly from
  the minimap viewport rectangle.
- F9: new culling test renders a 4502-node graph, zooms deep (13 batched
  wheel-ins), and asserts per-frame node draws stay ≤60. Under the
  `return true` mutation it fails with 4502 fills per frame.
- F10: new LOD test holds the camera in [0.3, 0.5) with containers open
  (leaves drawn, fill ≥300) and asserts zero leaf labels, then zooms past
  1.2 and asserts labels appear. Under the `if (true)` mutation it fails
  with 3471 leaf labels. A separate semantic-zoom test (function vs type
  leaves at identical geometry) fails with 3473 fills under gate removal.
- F11: bundling is now asserted by its unique observable — the "N edges"
  bundle count labels (individual rendering never produces them) — on both
  the >5000-edge branch (zoom ≥ 0.5) and the low-zoom branch. Both fail
  with 0 labels under `if (false)`.
- F12: interaction tests added for wheel zoom-in (world point under cursor
  preserved within 0.5 world units), wheel zoom-out, mouse-drag pan,
  single-finger touch pan, and two-finger pinch. The pinch test exposed a
  real production bug: the midpoint correction was computed via
  `screenToWorld()` reading the lagging display camera, making the
  correction identically zero — pinch zoom anchored at the screen center.
  Fixed against `targetCam` (same math as `applyZoomAtMouse`); the test
  fails with 19.5 world units of drift when the anchor is removed.
- F13: task Shipped notes rewritten; this R4 record supersedes the R3
  closure claim of full test verification.

### Verification Summary

- `ExplorerCanvas-performance.test.js`: 19/19 pass pristine.
- Mutation runs (logs in `/tmp/stage/review-evidence/task-053-r4/`):
  isVisible→true FAIL, label-guard→true FAIL, semantic gates removed FAIL,
  bundling→false FAIL (both branches), pinch anchor removed FAIL.
- Explorer regression suite (8 files): 292/292 pass.
- Mechanical checks pass: mirrored-logic-tests-js,
  tautological-assertions-js, assertionless-tests,
  task-commit-attribution.
