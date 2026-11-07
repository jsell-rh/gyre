---
title: "Explorer Canvas — Causal Flow Trace (Structural Lens Feature)"
spec_ref: "explorer-canvas.md §6"
depends_on:
  - task-072
  - task-065
progress: ready-for-review
coverage_sections:
  - "explorer-canvas.md §6 Causal Flow Trace (Structural Lens Feature)"
commits: ["88197c55e960ab3f7a1405044de6e016657d155e", "3474c9305d43cbb375da658914d84ec162f2ae9d", "3cb7e9848ae663d5f8e6230daba3cdb7a54cd0d1", "e72c00a839b565abf02a68793a0c1ad7f87e7fb0", "540d2e6706d24a191f03df3d97623219b5a95944", "67005b8905fa1c5da5d089b1215972f4effb0c86"]
---

## Spec Excerpt

When the user selects "Trace from here" on any node (right-click or toolbar button), the canvas enters **trace mode**:

1. The selected node highlights as the starting point
2. All nodes reachable via outgoing `Calls`/`RoutesTo` edges are highlighted in sequence
3. Unreachable nodes dim to 20% opacity
4. The trace path is numbered: ① → ② → ③ showing execution order
5. Each step is annotated with its governing spec (if any)
6. The detail panel shows the full trace as a list, each step clickable

This is `system-explorer.md` §5 — "What happens when an agent pushes code?" answered visually.

**Data requirement:** The knowledge graph must contain `Calls` edges. This requires the LSP call graph extraction pipeline (task-072).

## Implementation Plan

1. **Add "Trace from here" to the canvas context menu** in `ExplorerCanvas.svelte`:
   - Right-click on a node → context menu item "Trace from here"
   - Also add a toolbar button that activates trace mode for the selected node

2. **Implement BFS/DFS trace computation:**
   - From the starting node, follow outgoing `Calls` and `RoutesTo` edges
   - Build an ordered list of reachable nodes (BFS for breadth-first ordering, which better represents execution flow)
   - Track the path depth for each node
   - This can run client-side using the graph data already loaded, or server-side via a new MCP tool/API endpoint

3. **Render trace mode on the canvas:**
   - Starting node: bold border, numbered ①
   - Reachable nodes: highlighted with numbered labels ②③④... in BFS order
   - Trace edges: thickened and colored (e.g., orange) to show the flow path
   - Unreachable nodes: dim to 20% opacity
   - Unreachable edges: dim to 10% opacity

4. **Spec annotation on trace steps:**
   - For each node in the trace, check for `GovernedBy` edges
   - If a governing spec exists, show it as a small badge on the node (e.g., "platform-model.md")
   - In the detail panel, list the spec for each step

5. **Detail panel trace view:**
   - When trace mode is active, the detail panel shows a numbered list of trace steps
   - Each step shows: number, node name, node type, governing spec (if any)
   - Clicking a step pans/zooms the canvas to that node

6. **Exit trace mode:**
   - Click "Clear trace" button in toolbar
   - Press `Escape`
   - Right-click → "Clear trace"
   - Select a different node's "Trace from here" (replaces current trace)

## Acceptance Criteria

- [ ] Right-click context menu has "Trace from here" option
- [ ] Toolbar button activates trace for selected node
- [ ] BFS traversal via `Calls`/`RoutesTo` edges produces ordered trace
- [ ] Trace nodes numbered ①②③... in execution order
- [ ] Unreachable nodes dimmed to 20% opacity
- [ ] Trace edges highlighted (thickened, colored)
- [ ] Governing specs shown as badges on trace nodes
- [ ] Detail panel shows clickable trace step list
- [ ] Escape/Clear button exits trace mode
- [ ] Graceful handling when no `Calls` edges exist (show warning: "Call graph not available — run code sync to extract call edges")
- [ ] Frontend tests for trace mode activation and rendering
- [ ] `npm test` passes in `web/`

## Agent Instructions

Read `specs/system/explorer-canvas.md` §6 (Causal Flow Trace) for the full spec. This depends on task-072 (LSP call graph pipeline for `Calls` edges) and task-065 (ExplorerCanvas component). The trace computation should work with whatever edges are available — if `Calls` edges don't exist yet, show a warning but don't crash. The canvas component is at `web/src/lib/ExplorerCanvas.svelte`. Check how the existing graph data is structured (nodes and edges arrays) to understand how to traverse outgoing edges. Use `edge.edge_type === 'calls' || edge.edge_type === 'routes_to'` for filtering.

## Shipped

Causal Flow Trace (explorer-canvas.md §6) is implemented end-to-end on the real production surface (ExplorerCanvas via ExplorerView); coverage matrix row §6 is `implemented`. The acceptance checkboxes remain unchecked per the assigned (protected) contract — completion is attested here, in the coverage matrix, and by the tests.

This round addressed the review finding on `web/dist` (id 48a7907a, category: review). The finding's facts were re-confirmed at candidate ea5fa887: `git diff 11b746049..ea5fa887 -- web/dist` is empty (the dist rebuild shipped in e72c00a8 was silently reverted by checkpoint 3474c930 and merge a89ddf5d4), so the committed dist carries none of the §6 surface. The **source** surface was then verified intact at head: `git diff 88197c55..ea5fa887 -- web/src` is empty (no source drift after the last attributed source commit; the commits after it are task-file-only process commits), the branch's web/src changes come solely from the attributed commits 67005b89/540d2e67/88197c55 plus two no-regression merges, and the full §6 surface is present (causal-trace.js, trace-navigation.js, ExplorerCanvas trace toolbar/context-menu/warning/"Clear trace", ExplorerView `windowEscape={false}` wiring, NodeDetailPanel step list, both test suites). Per the executor contract for this assignment round — the executor restores generated web/dist files from the assignment base, and final integration rebuilds and publishes those assets remotely — no web/dist rebuild or dist commit was made on the branch; the dist gap is closed by the controller's final integration stage, which rebuilds web/dist from this source. Focused probe at head after `npm ci` (lockfile): vitest on causal-trace + ExplorerCanvasTrace + ExplorerViewShortcuts → 3 files, 53/53 passed, exit 0 (log `/tmp/stage/review-evidence/task-081-dist-repair-source-probe.log`, round record `/tmp/stage/review-evidence/task-081-dist-repair-round.md`).

Earlier repair rounds (history): Escape-cascade fix in 88197c55 (ExplorerCanvas gains a `windowEscape` prop, default `true`; ExplorerView passes `windowEscape={false}` because it owns the window Escape cascade and already exits trace mode by clearing `activeViewQuery` — kill-verified both ways); detail-panel population on trace activation in 540d2e67; E2E toolbar-baseline regeneration in 3cb7e984 (environment proven pixel-identical to the CI runner; assertion strength unchanged).

### Behavior

- **Traversal** — pure module `web/src/lib/causal-trace.js`: `computeTraceOrder` runs BFS over outgoing `calls`/`routes_to` edges from the start node (alphabetical sibling tie-break for determinism, cycle-safe via visited set, ignores non-causal edge types and dangling endpoints). The canvas `$effect` consumes it, restricted to the query-matched subgraph so step numbers agree with the dimming.
- **Activation** — right-click "Trace from here" is a primary context-menu item, and a toolbar **Trace** button activates the trace for the selected node (disabled with a hint tooltip when nothing is selected). Both go through one shared `startTraceFromNode`.
- **No call graph** — when the loaded graph has zero `calls`/`routes_to` edges, activating a trace shows a dismissible `role="alert"` banner with the exact spec text "Call graph not available — run code sync to extract call edges" instead of entering a degenerate empty trace.
- **Rendering** — numbered step badges (①②③…) in BFS order; thick red trace-path arrows with edge-type labels; unreachable nodes dim to 20% (`dim_unmatched: 0.2` on the trace query; unmatched edges get the query filter's ×0.1 factor). Also fixed a real bug found en route: the trace `$effect` never requested a redraw, so the overlay only appeared on the next unrelated pan/zoom — it now schedules one (untracked).
- **Spec annotation** — `governingSpecFor` resolves each step's governing spec from the `spec_path` field *or* its `GovernedBy` edge target; shown as a badge beside the step number on the canvas and per-step in the detail list.
- **Detail panel** — the panel shows the trace from the moment trace mode activates: the trace `$effect` emits the start node enriched with the full numbered step list (`_traceStep`/`_traceTotal`/`_traceSteps`) via `emitTraceDetailFromEffect`, for both the toolbar and right-click activation flows. The start-node marker is a plain untracked var so the activating click cannot trigger an intermediate effect run that clears it before the query round-trips as `activeQuery`; emission is deduped per start node so effect re-runs (data reloads) keep the user's current detail selection, and starting a new trace resets the guard and replaces the panel content. Clicking a step pans/zooms the canvas (`onTraceStepNavigate` → ExplorerView `focusRequest {nodeId, nonce}` → ExplorerCanvas `zoomToNode`) *and* swaps the panel to that step's node with the step list carried over via the pure helper `web/src/lib/trace-navigation.js` `traceStepTargetNode`.
- **Exit** — toolbar "Clear trace" button (replaces the Trace button while a trace is active), `Escape` (window-level listener in the standalone canvas, inputs excluded; in ExplorerView via its window Escape cascade, which clears the trace query after overlays above it close — the canvas opts out with `windowEscape={false}` so the two listeners never compete), right-click "Clear trace", and starting a new trace replaces the current one.

### Test evidence

- `web/src/__tests__/causal-trace.test.js` — 21 tests on the pure traversal: BFS-vs-DFS ordering kill test (diamond graph a→b/c→d/e numbers b=2,c=3,d=4,e=5), `routes_to` traversal, non-causal edges excluded, deterministic sibling ordering, cycles terminate, unknown/leaf roots, governed_by spec resolution, step-list construction.
- `web/src/__tests__/ExplorerCanvasTrace.test.js` — 26 component tests: toolbar/right-click activation issues the exact causal query (focus/outgoing/calls+routes_to/`_trace`/dim 0.2) and emits the enriched start node with the full BFS step list; disabled without selection; both warning paths show the exact spec message and issue no query; "Clear trace" emits `null`; window Escape exits trace mode in the standalone canvas (and does not clear non-trace queries); step badges ①–⑤ drawn; unreachable node dims to exactly 0.2× its untraced opacity; trace-overlay edge labels drawn in trace red; `focusRequest` selects + zooms; NodeDetailPanel step list renders and clicks dispatch `onTraceStepNavigate`; no re-emission on effect re-runs; a new trace from a different node replaces the emission; `traceStepTargetNode` unit tests; panel persistence across two consecutive step clicks.
- `web/src/__tests__/ExplorerViewShortcuts.test.js` — the Escape-cascade regression test: with the query editor open over an active trace, the first Escape closes only the editor (annotation survives), the second exits trace mode (annotation gone, step badges stop drawing while node labels still redraw). Failed at candidate 37112f4; passes at head.
- This round (dist-repair verification, head ea5fa887): focused probe — vitest on the three §6-pinning suites (causal-trace, ExplorerCanvasTrace, ExplorerViewShortcuts) → 53 passed, exit 0, after `npm ci` with the lockfile (log: `/tmp/stage/review-evidence/task-081-dist-repair-source-probe.log`). The vitest run also compiles the trace components via vite-plugin-svelte (source-compile evidence without rebuilding dist).
- Prior round, at head 88197c55: the 17 suites covering the changed components (ExplorerCanvasTrace, causal-trace, ExplorerCanvas, ExplorerCanvasDrillDown, ExplorerCanvas.evaluative-lens, ExplorerCanvas-performance, NodeDetailPanel, ExplorerViewShortcuts, ExplorerViewScope, ExplorerViewDrillUrl, ExplorerView-responsive, ScopeTransitions, WorkspaceDrawerSectionNav, WorkspaceHome, WorkspaceHomeArchitecture, WorkspaceHomeScopeDrill, WorkspaceHomeSections) → 373 passed | 41 skipped, exit 0.
- E2E baseline evidence from the earlier round (`/tmp/stage/review-evidence/`): the failing toolbar test was reproduced locally with identical failure numbers as CI (1607 px, ratio 0.03), the local actual screenshot was pixel-identical to the CI actual from run 42339252706, and post-update both toolbar tests passed (baselines untouched except the regenerated `toolbar-default.png`). The sandbox required `--no-sandbox/--no-zygote/--single-process` chromium launch flags (OS-sandbox restriction, rasterization unaffected — proven by the pixel-identical comparison); the local WebSocket upgrade rejection is a sandbox transport restriction recorded for remote verification, not a code defect.

### Notes for review

- Dimming semantics: "dim to 20%" composes with the semantic-zoom LOD fade (unmatched opacity = LOD × 0.2; matched nodes are boosted to full opacity by the query composition). The test pins the 0.2 factor against a no-query baseline of the same fixture.
- The trace suite uses a queued-rAF mock (matching real `requestAnimationFrame` semantics) because a synchronous rAF mock inverts `animLoop`'s `animFrame = requestAnimationFrame(animLoop)` handshake, silently dropping later redraw requests.
- The `zoom: 'fit'` directive on the trace query is unchanged from the pre-existing context-menu behavior.
- The activation-emission tests feed the issued query back via `rerender({ activeQuery })`, exactly what ExplorerView does (`onInteractiveQuery` → `activeViewQuery` → `activeQuery` prop); the start-node marker being an untracked plain var means the click itself triggers no intermediate effect run, so the ordering holds in both the test and production (where the query update and the click land in the same effect flush).
- Escape ownership: `windowEscape` defaults to `true` so the standalone canvas keeps the §6 affordance (covered by ExplorerCanvasTrace.test.js); ExplorerView is the only production embedder and opts out because its own window handler already clears trace queries through the overlay cascade. Any future host that does not route Escape itself can keep the default.
- web/dist: the committed dist at the candidate matches the assignment base (confirmed this round; see the opening). Under this assignment's executor contract the dist is restored from base and rebuilt/published remotely by final integration, so dist contents on the branch are not the delivery vehicle for the §6 UI surface — the source in `web/src` is. Verification of a served UI built from this head belongs to the remote final-integration build.
