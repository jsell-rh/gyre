# Coverage: View Query Grammar

**Spec:** [`system/view-query-grammar.md`](../../system/view-query-grammar.md)
**Last audited:** 2026-04-13
**Coverage:** 0/16

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | task-assigned | task-062 | |
| 2 | Primitives | 2 | task-assigned | task-062 | |
| 3 | 1. Computed References | 3 | task-assigned | task-062 | |
| 4 | 2. Scope — What Subgraph to Show | 3 | implemented | task-063 | [2026-10-09 task-063 R2 verified] All 6 scope types resolve on both surfaces. Rust resolver (view_query_resolver.rs:722-818 for Diff) has unit tests; client resolution (ExplorerCanvas.svelte:1933-2070) covers all/diff/focus/filter/test_gaps/concept, reads the real GraphNodeResponse fields, and mirrors server Diff semantics (temporal `~epoch` half-open, ≥7-char SHA prefix, from-exclusion). R1 dead-field/all-inert breaks repaired and mutation-verified. |
| 5 | 3. Emphasis — How to Color It | 3 | implemented | task-063 | [2026-10-09 task-063 R2 verified] highlight.matched color+label (ExplorerCanvas.svelte:3270-3287, label drawn in the matched color, LOD-gated), dim_unmatched :2117 with tree-group inheritance, tiered_colors :2179-2183 by BFS depth, heat :2090-2138, badges :3252-3267 with {{count}}. All operate for every scope including `all` (R1 inertness fixed). |
| 6 | 4. Edges — What Relationships to Show | 3 | implemented | task-063 | [2026-10-09 task-063 R2 verified] Type filter + result-set restriction: edges skipped unless both endpoints in queryMatchedIds (ExplorerCanvas.svelte:3575-3576, renderEdges :964-987). Operates for `all`-scope queries since the R1 fix. |
| 7 | 5. Zoom | 3 | implemented | task-063 | [2026-10-09 task-063 R2 verified] `{"level": N}` clamped, `"current"` no-op, `"fit"` bbox of matched nodes ×0.8 padding via animated targetCam (ExplorerCanvas.svelte:4769-4801), once per query instance. Fires for `all`-scope since the R1 fix. |
| 8 | 6. Annotation | 3 | implemented | task-063 | [2026-10-09 task-063 R2 verified] `$name`, `{{count}}` (queryMatchedIds.size), `{{group_count}}` (distinct parent prefixes) resolve at ExplorerCanvas.svelte:4992-4997; component tests assert all-scope and diff-scope counts resolve (not '?'). |
| 9 | 7. Interactive Bindings | 3 | implemented | task-063 | [2026-10-09 task-063 R2 verified] `$clicked` re-runs on canvas click with the clicked node's qualified_name substituted; `$selected` re-runs on selection change; click-mode badge in the annotation UI. Verified in R1; unchanged by the fix round. |
| 10 | Interaction Context | 2 | task-assigned | task-064 | |
| 11 | Composition Examples | 2 | task-assigned | task-064 | |
| 12 | Blast radius (interactive) | 3 | task-assigned | task-064 | |
| 13 | Test coverage gaps | 3 | task-assigned | task-064 | |
| 14 | High-risk untested code | 3 | task-assigned | task-064 | |
| 15 | Authentication concept | 3 | task-assigned | task-064 | |
| 16 | Relationship to Other Specs | 2 | task-assigned | task-064 | |
