---
title: "Explorer Visual Regression Tests"
spec_ref: "explorer-implementation.md §Migration Plan > Phase 4: Polish; §Testing > Visual Tests"
depends_on: 
  - task-029
  - task-030
  - task-031
  - task-032
  - task-033
  - task-037
  - task-053
progress: complete
review: specs/reviews/task-057.md
coverage_sections: []
commits: []
---

## Spec Excerpt

From `explorer-implementation.md` §Migration Plan > Phase 4: Polish:

> - Visual regression tests

From `explorer-implementation.md` §Testing > Visual Tests:

> - Semantic zoom at different zoom levels
> - View query rendering (groups, callouts, narrative markers)
> - Filter presets show correct subsets
> - Blast radius interactive mode

## Current State

- **Unit/integration tests exist:** `ExplorerCanvas.test.js`, `ExplorerChat.test.js`, `explorer_ws_integration.rs` — these test logic and WebSocket protocol, not visual rendering.
- **No visual regression tests exist.** No Playwright visual comparison tests, no screenshot snapshots, no visual diffing infrastructure.
- **Playwright is available:** E2E tests exist in the repo (from M17/HSI), so the Playwright infrastructure is in place.

## Implementation Plan

1. **Visual test infrastructure** — Create a Playwright visual regression test file (e.g., `web/e2e/explorer-visual.spec.ts` or `web/tests/visual/explorer.spec.ts`):
   - Use Playwright's `expect(page).toHaveScreenshot()` for visual comparison
   - Seed a test repo with a known graph (deterministic node positions via fixed ELK seed or snapshot data)
   - Navigate to the Explorer at repo scope before each test

2. **Test: Semantic zoom at different zoom levels** — Capture screenshots at 3+ zoom levels:
   - Zoomed out (Level 0: all packages) — verify packages are visible, labels readable
   - Mid zoom (Level 1: modules) — verify modules rendered inside packages
   - Zoomed in (Level 2: types) — verify type detail is visible

3. **Test: View query rendering (groups, callouts, narrative markers)** — Apply a view query with:
   - Groups (verify group bounding boxes render)
   - Callouts (verify callout text and arrows render)
   - Narrative markers (verify numbered narrative annotations render)
   - Compare against baseline screenshot

4. **Test: Filter presets show correct subsets** — For each preset (All, Endpoints, Types, Calls, Dependencies):
   - Apply the filter
   - Capture screenshot
   - Verify the visual matches the expected subset (e.g., Endpoints filter shows only endpoint nodes)

5. **Test: Blast radius interactive mode** — Load the "Blast Radius (click)" saved view:
   - Click a node
   - Verify tiered coloring renders (red → orange → yellow → gray)
   - Verify unmatched nodes are dimmed
   - Compare against baseline screenshot

6. **Deterministic rendering** — For visual regression to work, rendering must be deterministic:
   - Pin ELK layout seed or use pre-computed layout positions
   - Disable CSS animations during tests (Playwright's `reducedMotion: 'reduce'`)
   - Use a fixed viewport size (e.g., 1280×720)

## Acceptance Criteria

- [ ] Playwright visual regression test file exists with 4+ test cases
- [ ] Semantic zoom test captures 3 zoom levels and compares against baselines
- [ ] View query rendering test applies a query with groups, callouts, and narrative
- [ ] Filter preset test verifies all 5 presets show expected subsets
- [ ] Blast radius test verifies tiered coloring and dimming on node click
- [ ] Baseline screenshots are committed and reproducible
- [ ] `npx playwright test` passes (or equivalent Playwright visual test command)
- [ ] Visual diffs detect intentional rendering changes (update baselines on legitimate changes)

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `web/src/lib/ExplorerCanvas.svelte` for the canvas rendering implementation
3. Read existing Playwright E2E tests (e.g., `web/e2e/` or `tests/e2e/`) for project conventions
4. Read `web/src/lib/types/view-query.ts` for view query types (used in test assertions)
5. Ensure the test server seeds a repo with a known graph before visual tests run
6. Use `expect(page).toHaveScreenshot()` for Playwright visual regression
7. Set `reducedMotion: 'reduce'` and fixed viewport for deterministic rendering
8. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

- feat(explorer): add visual regression tests with Playwright (TASK-057)
- fix(explorer): rewrite visual regression tests to use real UI interactions (TASK-057)
- fix(process): prevent duplicate visual baselines and stale mechanism claims (TASK-057)
- fix(explorer): distinct mid-zoom baseline and accurate blast radius comments (TASK-057)
