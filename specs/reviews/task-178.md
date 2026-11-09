# Review: TASK-178 — Navigable Architecture (canvas layout, interaction model, lens selector)

**Reviewer:** Independent review (Gyre pipeline)
**Date:** 2026-11-09 (repair round)
**Comparison base:** f7d61ef5865b27ed7e11998ee4df53890fdde6f6
**Candidate:** b0675bad96590227f579b76c68a59cbe750daeab
**Verdict:** approved — all four findings from checkpoint 16e5800 are resolved; no new defects found.

---

## Scope

Diff base→candidate is frontend-only (`web/src/**`, `docs/ui.md`, task spec,
e2e visual baselines). Product commits 197222ba (implementation) and 618f415
(review repair) are both recorded in the task's `commits:` frontmatter;
process/docs commits touch no product surface. Server endpoints referenced by
the UI exist and are registered (`GET /repos/:id/graph/risks`,
`GET/POST/PUT/DELETE /workspaces/:id/explorer-views`,
`POST .../explorer-views/generate` — api/mod.rs:906,960-970).

## Resolution of the four prior findings

1. **Boundaries filter reached tree-group containers** — resolved.
   `treeGroupFilterHidden` (ExplorerCanvas.svelte:2648) classifies each
   synthetic container through its own node (`nodeCategory` for unchecked
   categories, `nodeMatchesConcept` for concept focus — the same
   filter-categories.js semantics as the leaf rules); a filtered group sets
   `op = 0`, which gates the entire container draw (`if (op > 0.01)` wraps
   `drawTreeGroup` in the save/globalAlpha/restore block) while children
   still recurse, and the minimap applies the same rule (line 4292).
   Edges touching hidden leaf nodes are skipped (line ~3980).

2. **Canvas-draw assertions were vacuous** — resolved. The capture is now
   frame-bucketed per canvas element (stable per-element ctx mock;
   `setTransform` delimits frames), tests wait for a frame drawn after the
   interaction (`labelsOfNextRedraw`) with a settle wait before the
   baseline. **Mutation-verified independently** (evidence:
   /tmp/stage/review-evidence/mutation-probes.md):
   - whole draw-loop filter block removed → 2 failed (Boundaries + concept
     tests), 10 passed;
   - only the tree-group branch removed → 2 failed (same two tests);
   - restored candidate source → 12/12 passed, working tree clean.

3. **Saved views load via REST when the dropdown opens** — resolved.
   `refreshSavedViews` (ExplorerView.svelte) fetches
   `api.explorerViews(wsId)` + `api.savedViews(repoId)` on dropdown open,
   merges by id (workspace records translated via `viewSpecToViewQuery`,
   repo records carry the ViewQuery), and socket `views` messages merge over
   the REST list preserving known query fields. Both api functions map to
   registered server routes. New end-to-end test covers the no-socket path
   (list → select → canvas annotation applied).

4. **Observable lens keyboard-operable, no new a11y warnings** — resolved.
   The `<label onclick>` is now a `<div role="checkbox" aria-checked
   aria-disabled tabindex="0">` with onclick + onkeydown (Enter/Space).
   Compile output of the full touched surface shows only warnings that are
   pre-existing at base (ctx-menu backdrop, canvas role, arch-risk-list
   `<li>`, `cam` non-reactive, ExplorerChat div listeners — verified present
   in base source / untouched by the diff).

## Test evidence (independently re-run, after `npm ci`)

- `ExplorerViewNavigableArchitecture.test.js` — 12/12.
- ExplorerCanvas/DrillDown/Liveness + ExplorerDefaultViews +
  ExplorerGeneratedViews + ExplorerViewLensUrl + filter-categories — 204/204.
- ExplorerView-{responsive,DrillUrl,LargeGraph,LayoutRenderers,
  LivenessWiring,Scope,Shortcuts,ViewEvents} + ExplorerChat* (3) +
  GeneratedViewChat + WorkspaceHomeArchitecture + RepoMode — 218/218.
- Related test-file updates (button→checkbox selectors) track the UI change
  and are not weakened assertions.

## Notes

- E2E visual baselines regenerated in-tree for the toolbar DOM change.
- Coverage rows for the two assigned sections remain `task-assigned` pending
  the controller's coverage-matrix update; that is outside review scope.
