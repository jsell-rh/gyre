# TASK-052 Review — R1

**Reviewer:** Verifier  
**Date:** 2026-04-11  
**Verdict:** `needs-revision` (6 findings)

---

## Findings

- [-] [process-revision-complete] **F1 (resolved R2): healthData breaking change lookup ignores dependent repo identity — same bc for every row.** `ImpactAnalysisModal.svelte:49`: the `.find()` call `repoBreakingChanges.find(b => b.dependency_edge_id && b.source_repo_id === repoId)` does not reference `dep.id` at all. The predicate is identical for every iteration of the `allDeps.map()`, so every health table row gets the same `breakingChange` object (the first matching one). Consequence: every dependent repo row shows the same "Acknowledge" button for the same breaking change, and acknowledging it marks all rows as acknowledged simultaneously. The per-dependent acknowledgment flow required by the acceptance criterion ("'Acknowledge' button per dependent repo for `block` policy") is broken for the multi-dependent case. The correct approach would match `dep.id` against the dependency edge's target repo. Since `BreakingChange` only exposes `dependency_edge_id` (not target repo ID directly), the fix likely requires either enriching the breaking change data with a `target_repo_id` field for client-side correlation, or looking up edges by `dependency_edge_id` to determine which dependent each breaking change applies to.

- [-] [process-revision-complete] **F2 (resolved R2): Hardcoded `'default'` workspace ID in dependency policy fetch.** `ImpactAnalysisModal.svelte:84`: `api.workspaceDependencyPolicy?.('default')` hardcodes the string `'default'` as the workspace ID. The component receives only `repoId` and `repoName` as props — it has no workspace context. In production, workspace IDs are generated UUIDs. This call will 404 for any real workspace (the endpoint is `GET /api/v1/workspaces/:id/dependency-policy`), causing `dependencyPolicy` to always be `null` and the block policy notice + acknowledge column to never appear. Fix: add a `workspaceId` prop to the component and use it in the API call; update the `DetailPanel.svelte` instantiation to pass the workspace ID from the MR or repo data.

- [-] [process-revision-complete] **F3 (resolved R2): Cascade test section is entirely static HTML with no dynamic data path.** `ImpactAnalysisModal.svelte:269-275`: the cascade test section renders hardcoded text `"Cascade tests: not configured"` and a permanently `disabled` button with no `onclick` handler. There is no conditional logic to display pass/fail results per dependent repo, no data fetching for cascade test status, and no code path that could show results even if the backend started returning them. Acceptance criteria state: "Cascade test results shown when available (pass/fail per dependent repo)" and "'Trigger Cascade Tests' button shown when cascade tests have not run." The implementation satisfies neither — the first requires dynamic rendering conditioned on cascade test data availability, and the second requires a functional button (at minimum, an onclick handler that calls a cascade test endpoint). The graceful degradation path ("not configured" when TASK-022 is unavailable) is correct, but the "when available" path is missing entirely.

- [-] [process-revision-complete] **F4 (resolved R2): PipelineOverview merge queue trigger point is dead code.** `PipelineOverview.svelte` is not imported or rendered by any component in the application. It is referenced only in comments (`WorkspaceHome.svelte:5`, `WorkspaceHome.svelte:4905`, `WorkspaceHomeSections.test.js:219`). The `breakingCount` and `onImpactAnalysis` props added by this task, and the breaking change button that uses them (lines 153-165), are unreachable. Acceptance criterion: "Modal accessible from merge queue and MR detail for breaking-change MRs" — the merge queue path is not functional. The DetailPanel integration works (the MR detail path), but the merge queue path does not.

- [-] [process-revision-complete] **F5 (resolved R2): Acknowledge test uses conditional guard that makes the assertion silently skippable.** `ImpactAnalysisModal.test.js:266`: `if (ackBtn) { await fireEvent.click(ackBtn); expect(...) }` — if the acknowledge button fails to render (due to any rendering bug), the test passes without executing any assertion. The test should assert that the button exists (`expect(ackBtn).toBeTruthy()`) before clicking it, not guard the assertion behind an `if`.

- [-] [process-revision-complete] **F6 (resolved R2): Missing component tests for trigger points — DetailPanel "Check Impact" button and PipelineOverview breaking change button have zero test coverage.** The acceptance criterion "Component tests verify rendering and interaction" applies to the trigger points, not just the modal itself. No test verifies that DetailPanel renders a "Check Impact" button for MR entities, that clicking it opens the ImpactAnalysisModal, or that PipelineOverview renders a breaking change button when `breakingCount > 0`. All 16 tests target ImpactAnalysisModal in isolation. The parent integration surfaces are untested.

---

# TASK-052 Review — R2

**Reviewer:** Verifier  
**Date:** 2026-04-11  
**Verdict:** `needs-revision` (2 findings)

## R1 Finding Resolution

- F1: **Resolved.** `healthData` now maps breaking changes to specific dependents via `edgeToDependent` (`ImpactAnalysisModal.svelte:47-53`). The `.find()` at line 69-72 correlates `b.dependency_edge_id` → `edgeToDependent.get(...)` → `dep.id`. Test `maps different breaking changes to their specific dependent repos via edges` verifies 2 edges with 2 distinct BCs produce exactly 1 unacknowledged button + 1 acknowledged badge.
- F2: **Resolved.** `workspaceId` is now a prop (line 19). `DetailPanel.svelte:5117` passes `mrData.workspace_id`. `WorkspaceHome.svelte` passes `workspace?.id`. Fallback resolves via `api.repo(repoId)` (line 117).
- F3: **Resolved.** Cascade section now has dynamic rendering: `{#if cascadeTestResults && cascadeTestResults.length > 0}` shows per-repo pass/fail results (lines 333-344); otherwise shows "not configured." Trigger button has `onclick={triggerCascadeTests}` (line 353). Tests verify both states.
- F4: **Resolved.** The merge queue trigger IS functional: WorkspaceHome's inline pipeline bar now has a breaking change button at lines 1136-1147 (of the diff) that calls `openImpactAnalysis()` and opens `ImpactAnalysisModal`. The acceptance criterion "Modal accessible from merge queue" is satisfied. However, the fix introduced a dead import (see F7 below).
- F5: **Resolved.** Test at `ImpactAnalysisModal.test.js:371-372` now uses `expect(ackBtn).toBeTruthy()` before `fireEvent.click(ackBtn)`.
- F6: **Resolved** for DetailPanel (tests at `DetailPanel.test.js:490-503` verify button rendering for MR vs non-MR entities). PipelineOverview now has tests but the component is dead code (see F7, F8 below).

## Findings

- [ ] **F7: Dead import — PipelineOverview imported but never rendered in WorkspaceHome.** `WorkspaceHome.svelte:23` imports `PipelineOverview from './PipelineOverview.svelte'` but no `<PipelineOverview>` tag appears anywhere in WorkspaceHome's template. The F4 fix added the breaking change button directly to WorkspaceHome's own inline pipeline bar (the `<button class="pipeline-breaking-btn">` at line ~1138), making the import dead code. The PipelineOverview component itself remains unreachable — it is never rendered by any component in the application (`grep -r '<PipelineOverview' web/src/` returns zero hits). The `breakingCount` and `onImpactAnalysis` props added to PipelineOverview by this task are unreachable dead code. Fix: either remove the dead import (and accept that PipelineOverview is a pre-existing dead component), or actually render `<PipelineOverview>` in WorkspaceHome and remove the duplicate inline button.

- [ ] **F8: PipelineOverview.test.js tests dead code — WorkspaceHome's live breaking change button untested.** `PipelineOverview.test.js` has 3 breaking-change tests (shows button when `breakingCount > 0`, hides when 0, calls `onImpactAnalysis` on click) — but PipelineOverview is never rendered in the app, so these tests validate unreachable code. The WorkspaceHome breaking change button (the actual, functional trigger added by the F4 fix) has zero test coverage: no test in `WorkspaceHomeSections.test.js`, `WorkspaceHome.test.js`, or any other file verifies (a) the button appears when `depHealthData.breakingCount > 0`, (b) clicking it calls `openImpactAnalysis()` and opens ImpactAnalysisModal, or (c) the button is hidden when `breakingCount === 0`. The acceptance criterion "Component tests verify rendering and interaction" is not satisfied for the merge queue trigger point — the tests exist but cover the dead component, not the live surface.
