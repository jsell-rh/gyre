# Review: TASK-051 — Spec Links Impact Analysis View

**Reviewer:** Verifier  
**Round:** R1  
**Commit:** `798a0647`  
**Verdict:** `needs-revision`

---

## Findings

- [ ] **F1: Response field name mismatch — `target_repo_id` used for dependent's repo.**  
  `DetailPanel.svelte` line ~1205 assigns `repo_id: directDep?.target_repo_id ?? null` to each dependent spec. But `target_repo_id` in `SpecLinkResponse` is the repo of the link's *target* — which, for the `/dependents` endpoint, is the *queried spec itself*, not the dependent. The dependent spec's repo would be `source_repo_id`, but `SpecLinkResponse` does not expose `source_repo_id` (it's in `SpecLinkEntry` but dropped by the `From` impl). Consequence: all dependents are grouped under the queried spec's repo (or `null`/`"unknown"`), not their actual repos. The summary panel's "across M repos" count and the grouped-by-repo detail list will be wrong whenever dependents span multiple repos.  
  **Fix:** Either add `source_repo_id` to `SpecLinkResponse` (preferred — TASK-019 scope, but required for correctness), or look up the dependent spec's repo from the graph nodes via `allNodes.find(n => n.path === depPath)` — though `SpecGraphNode` also lacks a `repo_id` field. As a minimum client-side fix: use `specData?.repo_id` from a ledger lookup (same pattern as `SpecDashboard` uses), or fetch the spec list and join on `path`.

- [ ] **F2: Aspirational test name — "dims non-highlighted edges" tests no edge dimming.**  
  `SpecGraphDAG.test.js` line ~511: `it('dims non-highlighted edges', ...)` uses `IMPACT_EDGES` where every edge connects nodes that are dependents of `core.md` (auth→core, billing→core, deep→auth). When `impactPath = 'system/core.md'`, all three edges are highlighted. The test iterates all edges and asserts `opacity === '1'` — proving edges are NOT dimmed. To test dimming, the test data needs an edge between `system/unrelated.md` and a non-dependent node (e.g., add `{ source: 'system/unrelated.md', target: 'system/deep.md', link_type: 'references', status: 'active' }`). The current test would pass even if edge dimming was deleted entirely.  
  **Fix:** Add a non-impacted edge to `IMPACT_EDGES` and assert its containing `<g>` has `opacity === '0.15'`.

- [ ] **F3: No component tests for SpecDashboard or DetailPanel impact features.**  
  The acceptance criteria state "Component tests verify rendering and interaction." The 8 new tests all target `SpecGraphDAG`. There are zero tests for:
  - SpecDashboard: the "Analyze Impact" button, impact mode toggle, `impact-prompt` rendering, `impact-summary` panel with grouped-by-repo details, "Exit Impact Analysis" button, node click deselect toggle
  - DetailPanel: the "Analyze Impact" button in the links tab, the `impact-tree` rendering, tree view with depth indicators and approval badges, error state, loading state  
  The SpecDashboard and DetailPanel are the primary user-facing surfaces for impact analysis — their rendering is untested.  
  **Fix:** Add at least one test per component verifying: (a) SpecDashboard renders the impact summary panel when `impactPath` is set, with correct spec/repo counts; (b) DetailPanel renders the impact tree after loading, with grouped items showing link type and approval status.

- [ ] **F4: DetailPanel `loadImpactAnalysis` fetches `directDeps` but excludes `extends` links.**  
  The `get_spec_dependents` server endpoint (specs.rs:1139-1142) only returns links with `link_type == DependsOn || link_type == Implements`. But the client-side BFS in `loadImpactAnalysis` includes `extends` in `IMPACT_LINK_TYPES`. A direct `extends` dependent (spec A extends spec B, analyzing B's impact) will be found by the client-side BFS but will NOT appear in `directDeps` from the API. As a result, `info.direct` is false for direct extends dependents, and `dep.direct` is false — the UI incorrectly displays "depth 1" instead of "direct" for these specs.  
  **Note:** This is partially a server-side issue (TASK-019 scope), but the client should be aware of the limitation. At minimum, mark extends dependents found at depth 1 in the BFS as direct, since depth 1 IS direct by definition. Or: don't use the `directDeps` API response to determine directness — use BFS depth instead (depth === 1 → direct).

