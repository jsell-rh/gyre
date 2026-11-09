# Review: TASK-178 — Navigable Architecture (canvas layout, interaction model, lens selector)

**Reviewer:** Independent review agent
**Date:** 2026-04-13
**Candidate:** `314b7e5ea62ebd6384a32304ab88c50439e5c416` (base `77473763887f249f71d6402d4fc712d51bd247c5`)
**Verdict:** needs-revision

---

## What was verified as real

- **Lens selector** is a checkbox group per ui-navigation.md §3: Structural checked+locked, Evaluative additive toggle, Observable locked-off with telemetry notice banner (`ExplorerCanvas.svelte` toolbar).
- **Evaluative lens risk overlay**: `ExplorerView.svelte` fetches `GET /repos/:id/graph/risks` when the lens is active (endpoint verified server-side: `api/mod.rs:900`, `api/graph.rs:704`, ABAC mapping present), maps churn × complexity per node, and the canvas colors nodes through `getNodeMetricValue`/`evaluativeNodeColor`; `EvaluativeOverlay` renders the Risk heat button. The fetch test asserts the mock call directly — meaningful.
- **View selector dropdown**: built-in views → saved views → "Generate view…" that focuses the generator chat; saved views apply their stored query (annotation title + button label asserted).
- **View spec editor**: JSON editor with prefill, Apply → `viewSpecToViewQuery` → live canvas re-render, invalid JSON → inline `role=alert` and nothing applied.
- **Filter panel**: 200px collapsible left panel inside the Architecture content area, control-bar filter icon toggle, category checkboxes with counts, Concepts section from `filter-categories.js` (14 solid pure-function unit tests).
- Pre-existing pieces confirmed present: zoom/pan semantic-zoom canvas, coverage coloring, context menu (View spec/provenance/history/Open in code), `/` search, timeline scrubber, Ask input → `POST /workspaces/:id/explorer-views/generate`.
- Focused suites: 25 + 190 + 95 tests green across all Explorer suites (evidence: /tmp/stage/review-evidence/run{1,2,3}-*.txt).

## Findings

- [ ] **F1 (code): Boundaries category filter is a visible no-op at default zoom.** In the default containment layout, the boundary boxes on screen are synthetic tree-group containers (`buildPathTree` → `layoutPathTreeNode`, synthetic `node_type: 'package'/'module'`), which the draw-loop filter deliberately exempts (`if (!isTreeGroup && hiddenNodeIds?.size > 0)`, ExplorerCanvas.svelte ~2725); the boundary-typed leaf graph nodes are not drawn at default zoom. Probe (probe5-result.json): with a package→module→{type,endpoint} graph, the canvas label set after unchecking **Boundaries** is identical to the initial frame, while unchecking **Interfaces**/**Data** correctly hides the leaf nodes. The spec's first-listed filter category toggles nothing visible in the canonical default view (system-explorer.md §1 / ui-navigation.md §3).
- [ ] **F2 (test): the "hides nodes" assertions are vacuous and mask F1.** Mutation probe: deleting the production node-hiding block from `drawNodeRecursive` and running `ExplorerViewNavigableArchitecture.test.js` yields **11/11 passing** — including "unchecking Boundaries hides boundary nodes from the canvas draw" and the concept-focus test. `drawnLabels()` snapshots-and-clears the captured `fillText` output; `waitFor` polls see an empty snapshot whenever the canvas is idle between redraws, so `not.toContain(...)` passes vacuously, and the round-trip "restore" assertion is satisfied trivially since the node never left. The shipped notes' "non-vacuous" claim is disproven. Fix: force a redraw before each poll and assert against a positively-captured frame.
- [ ] **F3 (code, minor): saved-views dropdown depends on the chat WebSocket.** `explorerSavedViews` populates only via the explorer chat socket (`list_views` on open → `views` message). The REST list endpoint `GET /workspaces/:id/explorer-views` (`api.explorerViews`, api.js:614) is unused in production code; if the socket fails or the chat column is collapsed before the list arrives, the dropdown shows no saved views. Fetch via REST on dropdown open (or mount) instead.
- [ ] **F4 (code, minor): new a11y warnings.** The Observable checkbox is a `<label onclick>` (~line 5842), emitting `a11y_click_events_have_key_events` / `a11y_no_noninteractive_element_interactions` on every build/test run; the pre-change code used a `<button>`.

## Evidence

All probes, commands, outputs, and mutation/restore records under `/tmp/stage/review-evidence/` (summary.md indexes them). Working tree verified clean after temporary mutations were restored.
