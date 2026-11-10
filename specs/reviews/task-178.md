# Review — task-178 (Navigable Architecture — canvas layout, interaction model, lens selector)

Spec: `specs/system/system-explorer.md` §1 "The Navigable Architecture" + `specs/system/ui-navigation.md` §3 "Tab: Architecture (Moldable Development Surface)".
Commits under review: `197222ba9` (implementation), `618f41527` (review repair), on top of base `f7d61ef`. Candidate `b0675bad9`.
This is the repair round for the four findings from the prior review of `314b7e5` (checkpoint 16e5800). Verdict: **approved**.

## Round 2 (repair verification)

Environment: `web/` after `npm ci` (locked versions); all commands below under `/tmp/stage/review-evidence/` (summary.md has full transcripts).

- `npx vitest run src/__tests__/ExplorerViewNavigableArchitecture.test.js src/__tests__/filter-categories.test.js` → 26 passed.
- `npm test` (full frontend suite) → 100 files, **2026 passed / 41 skipped** — no regressions.
- Mutation probes (each restored to pristine, `git diff` verified empty):
  - Deleting the entire node-hiding block from `drawNodeRecursive` → the Boundaries and concept-focus tests **fail** (2/12). The prior vacuousness (11/11 passing with the code deleted) is repaired.
  - Deleting only the tree-group branch (`treeGroupFilterHidden` → `op = 0`) → same 2 tests **fail**: the tree-group repair itself is covered.
  - Removing the `refreshSavedViews()` call on dropdown open → the REST saved-views test **fails**.

Findings from Round 1 — all four verified repaired:

1. **Boundaries filter no-op at default zoom (fixed).** The boundary boxes at default zoom are the synthetic package/module tree-group containers; the draw loop now classifies them through their own synthetic node (`treeGroupFilterHidden`: `nodeCategory` for category checkboxes, `nodeMatchesConcept` for concept focus — the same filter-categories.js semantics as the leaf rules). A filtered group skips its box but keeps drawing children, so zoomed-in content of other categories stays visible; the minimap applies the same rule. The redraw effect tracks the filter props by identity, not size. Verified structurally (ExplorerCanvas.svelte ~2648, ~2768, ~4291; synthetic nodes carry `node_type: 'package'|'module'` at ~1590) and behaviorally (mutation B).
2. **Vacuous canvas-draw tests (fixed).** Capture is now frame-bucketed per canvas element (stable per-element ctx mock; `drawFrame`'s opening `setTransform` delimits frames), the latest frame's labels persist, and tests wait for a frame drawn after the interaction (`labelsOfNextRedraw`) with a settle wait before the baseline. Mutations A and B above independently confirm the assertions bind to the production hiding code.
3. **Saved views depend solely on the chat socket (fixed).** Opening the view-selector dropdown now fetches `GET /workspaces/:id/explorer-views` and `GET /repos/:id/views` (merged by id; workspace ViewSpecs translated via `viewSpecToViewQuery`); socket `views` updates merge over the REST-fetched list preserving already-known query fields. Both server routes pre-exist with ABAC mappings. Mutation C confirms the no-socket test path is real.
4. **Observable lens a11y (fixed).** The `<label onclick>` is now a keyboard-operable `div[role=checkbox]` with `aria-checked`/`aria-disabled`/`tabindex=0` and Enter/Space handlers surfacing the telemetry notice. The remaining build warnings (ctx-menu-backdrop, canvas role=application, arch-risk-item `li`, `cam` non_reactive_update, ExplorerChat div) were verified pre-existing at base `f7d61ef` by grepping the base files.

Acceptance criteria re-checked against the candidate: semantic-zoom boundaries/interfaces at default zoom; spec-coverage green/amber/red (`specBorderColor` from `spec_confidence` + governed_by edges); lens checkbox group (Structural locked on, Evaluative toggle fetching `GET /repos/:id/graph/risks` into the Risk heat metric, Observable future-locked); view-selector dropdown (built-in → saved → Generate view…); 200px collapsible in-view filter panel with categories + Concepts; right-click context menu (View spec/provenance/history/Open in code, pre-existing, exercised by real `contextmenu` events in ExplorerCanvasTrace tests); Ask input → `POST /workspaces/:id/explorer-views/generate` via GeneratedViewChat with explicit save; View Spec Editor (collapsible JSON editor, Apply via `viewSpecToViewQuery`, inline invalid-JSON error). Test-file adaptations across ExplorerCanvas*/DefaultViews/GeneratedViews/LensUrl are button→checkbox conversions with strengthened (checked/disabled) assertions — no weakening. No Rust/server changes in the diff; scripts and unrelated specs untouched.

No new findings.
