# Coverage: View Query Grammar

**Spec:** [`system/view-query-grammar.md`](../../system/view-query-grammar.md)
**Last audited:** 2026-04-13
**Coverage:** 0/16

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | task-assigned | task-062 | |
| 2 | Primitives | 2 | task-assigned | task-062 | |
| 3 | 1. Computed References | 3 | task-assigned | task-062 | |
| 4 | 2. Scope — What Subgraph to Show | 3 | implemented | task-063 | [2026-10-09 task-063, R3 re-verified against current tree] All 6 scope types resolve on both surfaces. Rust resolver (view_query_resolver.rs:722-818 Diff; unit tests :2532-2698, :4385-4451) — 11 scope tests pass. Client resolution (ExplorerCanvas.svelte:1940-2076) covers all/focus/filter/test_gaps/concept/diff, reads the real GraphNodeResponse fields (created_sha/last_modified_sha/created_at/last_modified_at) and mirrors server Diff semantics (temporal `~epoch` half-open, ≥7-char SHA prefix, from-exclusion; validator requires both refs non-empty so the JS both-refs guard is unreachable-divergence). R1 dead-field/all-inert breaks repaired; all-branch mutation probe re-confirmed this round (deleting it fails its targeted test). |
| 5 | 3. Emphasis — How to Color It | 3 | implemented | task-063 | [2026-10-09 task-063, R3 re-verified] highlight.matched color+label (ExplorerCanvas.svelte:3274-3287, label drawn in the matched color, LOD-gated sw>30 && sh>14 && zoom>=0.5), dim_unmatched :2117-2119 with tree-group inheritance :2094-2104, tiered_colors :2179-2186 by BFS depth, heat :2127-2177 + heatColor :2190, badges :3290-3321 with {{count}} substitution. All operate for every scope including `all`. |
| 6 | 4. Edges — What Relationships to Show | 3 | implemented | task-063 | [2026-10-09 task-063, R3 re-verified] Type filter (filter/exclude, renderEdges :964-987) + result-set restriction: edges dimmed unless both endpoints in queryMatchedIds (ExplorerCanvas.svelte:3575-3578). Operates for `all`-scope queries. |
| 7 | 5. Zoom | 3 | implemented | task-063 | [2026-10-09 task-063, R3 re-verified] `{"level": N}` clamped to MIN/MAX_ZOOM, `"current"` no-op, `"fit"` bbox of matched layoutNodes ×0.8 padding via animated targetCam (ExplorerCanvas.svelte:4769-4801), once per query instance (lastZoomedQuery identity guard). Fires for `all`-scope since the R1 fix. |
| 8 | 6. Annotation | 3 | implemented | task-063 | [2026-10-09 task-063, R3 re-verified] `$name`, `{{count}}` (queryMatchedIds.size), `{{group_count}}` (distinct parent prefixes) resolve at ExplorerCanvas.svelte:4992-5009; component tests assert all-scope and diff-scope counts resolve (not '?'). |
| 9 | 7. Interactive Bindings | 3 | implemented | task-063 | [2026-10-09 task-063, R3 re-verified] `$clicked` re-runs on canvas click with the clicked node's qualified_name substituted (ExplorerCanvas.svelte:4160-4171); `$selected` re-runs on selection change (:71-87); click-mode badge in the annotation UI (:5017-5019). Verified in R1; unchanged by the fix round. |
| 10 | Interaction Context | 2 | task-assigned | task-064 | |
| 11 | Composition Examples | 2 | task-assigned | task-064 | |
| 12 | Blast radius (interactive) | 3 | task-assigned | task-064 | |
| 13 | Test coverage gaps | 3 | task-assigned | task-064 | |
| 14 | High-risk untested code | 3 | task-assigned | task-064 | |
| 15 | Authentication concept | 3 | task-assigned | task-064 | |
| 16 | Relationship to Other Specs | 2 | task-assigned | task-064 | |
