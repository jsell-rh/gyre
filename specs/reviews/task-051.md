# Review: TASK-051 — Spec Links Impact Analysis View

**Reviewer:** Verifier  
**Round:** R2  
**Commit:** `de992204`  
**Verdict:** `needs-revision`

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

- [ ] **F5: DetailPanel `repoCount` counts "unknown" group as a repo — inconsistent with SpecDashboard.**  
  `DetailPanel.svelte:1228` computes `repoCount: groups.size` where groups are keyed by `dep.repo_id ?? 'unknown'`. For transitive dependents (depth > 1) or direct `extends` dependents, `depRepoMap.get(depPath)` returns `undefined` → `repo_id` is null → grouped under key `'unknown'`. The `groups.size` count includes `'unknown'` as a counted repo, inflating the "across M repos" number.  
  Meanwhile, `SpecDashboard.svelte:169` correctly uses `new Set(impactDependentsList.map(d => d.repo_id).filter(Boolean)).size` — excluding null repo_ids from the count.  
  **Consequence:** The same impact analysis from two UI surfaces produces different "across M repos" counts when transitive dependents exist. The spec says "these 14 specs across 6 repos need review" — counting "unknown" as a repo produces an incorrect count. Example: 2 specs from known repos + 1 transitive spec with null repo_id → DetailPanel shows "3 specs across 3 repos" while SpecDashboard shows "3 specs across 2 repos."  
  **Fix:** In `loadImpactAnalysis()`, compute `repoCount` excluding null repo_ids: `repoCount: new Set(deps.map(d => d.repo_id).filter(Boolean)).size` — same pattern as SpecDashboard.

- [ ] **F6: DetailPanel test lacks transitive dependents — R2 "depth N" branch untested.**  
  The R2 fix changed the directness logic from API response membership to BFS depth (`info.depth === 1`, line 1211). The test data in `DetailPanel.test.js` has only direct (depth 1) dependents — `mockGraph.edges` contains `auth→core` and `billing→core` with no chain for BFS to find depth > 1 results. So:  
  (a) The "depth N" rendering branch (`DetailPanel.svelte:3704`: `dep.direct ? 'direct' : \`depth ${dep.depth}\``) — the `else` branch is never exercised by any test.  
  (b) The null `repo_id` grouping path (deps falling into the "unknown" group) is never tested.  
  (c) The `depRepoMap` miss path (`depRepoMap.get(depPath) ?? null` returning null for a transitive dep) is never tested.  
  **Fix:** Add a transitive chain to `mockGraph`: e.g., add node `system/deep.md` and edge `{ source: 'system/deep.md', target: 'system/auth.md', link_type: 'extends', status: 'active' }`. Then assert: `deep.md` item shows depth badge `"depth 2"` (not `"direct"`), and the tree header accounts for the transitive spec.

