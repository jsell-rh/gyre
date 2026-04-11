# TASK-060: Remove Dead `MoldableViewNodeTypeFilter` Test File

**Spec reference:** `development/frontend-testing.md` (tests must pass), `explorer-canvas.md` (MoldableView superseded by ExplorerCanvas)  
**Depends on:** None  
**Progress:** `not-started`

## Problem

`web/src/__tests__/MoldableViewNodeTypeFilter.test.js` imports `MoldableView` from `../lib/MoldableView.svelte` (line 14), but `MoldableView.svelte` was removed when the Graph + Flow tabs were merged into the unified `ExplorerCanvas`. The file does not exist — the test fails at import time with a module-not-found error.

This is the only test file with a broken import. Other tests that previously referenced MoldableView (e.g., `ExplorerViewScope.test.js`, `ExplorerViewAskViewSpec.test.js`) import `ExplorerView` instead and are not affected.

## Current State

- `web/src/__tests__/MoldableViewNodeTypeFilter.test.js` — 4 test cases testing `nodeTypeFilter` prop behavior on the old MoldableView component
- `web/src/lib/MoldableView.svelte` — **does not exist** (superseded by ExplorerCanvas)
- The filtering logic these tests covered is now handled by view queries and the `filter` prop on `ExplorerCanvas`

## Implementation Plan

1. **Delete the dead test file**: Remove `web/src/__tests__/MoldableViewNodeTypeFilter.test.js`.
2. **Verify no other imports reference MoldableView**: Grep for `MoldableView` across the codebase to confirm no other production code references it.
3. **Run `npm test`** to confirm all remaining tests pass.

## Acceptance Criteria

- [ ] `web/src/__tests__/MoldableViewNodeTypeFilter.test.js` is deleted
- [ ] No other files import or reference `MoldableView.svelte`
- [ ] `cd web && npm test` passes

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Delete `web/src/__tests__/MoldableViewNodeTypeFilter.test.js`
3. Grep for `MoldableView` across `web/src/` to verify no remaining references
4. If any references are found in other test files (e.g., docblock comments), update them to reference `ExplorerCanvas` instead
5. Run `cd web && npm test` to verify all tests pass
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits
