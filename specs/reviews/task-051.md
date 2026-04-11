# Review: TASK-051 — Spec Links Impact Analysis View

**Reviewer:** Verifier  
**Round:** R3  
**Commit:** `63d1026d`  
**Verdict:** `complete`

---

## Findings

- [-] [process-revision-complete] **F1 (resolved R2): Response field name mismatch — `target_repo_id` used for dependent's repo.**  
  R2 fix: Added `source_repo_id` to `SpecLinkResponse` and used `depRepoMap` keyed by `source_path` to resolve the dependent's actual repo. Verified: `From<SpecLinkEntry>` correctly maps `source_repo_id`, and `DetailPanel.svelte:1213` now uses `depRepoMap.get(depPath)`.

- [-] [process-revision-complete] **F2 (resolved R2): Aspirational test name — "dims non-highlighted edges" tests no edge dimming.**  
  R2 fix: Added a non-impacted edge (`references: unrelated→deep`) to `IMPACT_EDGES`. Test renamed to "dims non-highlighted edges and highlights impacted edges" and asserts `dag-edge-3` has `opacity="0.15"`. Verified: `unrelated.md` is not in the BFS impact set (references excluded from `IMPACT_LINK_TYPES`), so edge 3 is correctly dimmed.

- [-] [process-revision-complete] **F3 (resolved R2): No component tests for SpecDashboard or DetailPanel impact features.**  
  R2 fix: Added 2 DetailPanel tests (tree rendering, grouped items) and 4 SpecDashboard tests (summary counts, grouped details, exit button, deselect toggle). All pass.

- [-] [process-revision-complete] **F4 (resolved R2): DetailPanel `loadImpactAnalysis` fetches `directDeps` but excludes `extends` links.**  
  R2 fix: Changed directness determination from API response membership to BFS depth (`info.depth === 1`). Comment added at line 1182-1184 explaining the rationale. The underlying server-side scope limitation (TASK-019) is correctly acknowledged.

- [-] [process-revision-complete] **F5 (resolved R3): DetailPanel `repoCount` counts "unknown" group as a repo — inconsistent with SpecDashboard.**  
  R3 verification: `DetailPanel.svelte:1227` now uses `new Set(deps.map(d => d.repo_id).filter(Boolean)).size` — identical formula to `SpecDashboard.svelte:169`. Test asserts "3 specs" / "2 repos" (deep.md null repo_id correctly excluded from count).

- [-] [process-revision-complete] **F6 (resolved R3): DetailPanel test lacks transitive dependents — R2 "depth N" branch untested.**  
  R3 verification: Test data now includes `system/deep.md` node with `extends` edge creating depth-2 chain. Test asserts: `deepItem` depth is `'depth 2'` (else branch exercised), `unknownGroup` contains `deep.md` (null repo_id path exercised), repo groups count is 3 (including unknown), total items is 3.
