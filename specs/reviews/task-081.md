# Review — task-081 (Explorer Canvas — Causal Flow Trace, Structural Lens Feature)

Spec: `specs/system/explorer-canvas.md` §6 (Causal Flow Trace). Candidate `527854113c15b8b833050201fcfeb4872d44b6f0` on `pipeline/task-081/abb57627b2a6480b844088ec11fbd629-21`, diff against base `7f8b5d9c`. Product commit in scope per `commits:` frontmatter: `67005b89` (recovered checkpoint; the merge with base and process commits carry no product changes).

Verdict: **needs revision** (2 findings, both against §6 step 6 "The detail panel shows the full trace as a list, each step clickable").

## What was verified as real and working

- **Traversal** — `web/src/lib/causal-trace.js` is a genuine pure BFS module (outgoing `calls`/`routes_to` only, deleted-edge and dangling-endpoint skipping, cycle-safe visited set, alphabetical sibling tie-break, `governed_by` spec fallback in `governingSpecFor`). The canvas `$effect` (ExplorerCanvas.svelte:4851+) consumes it restricted to the query-matched subgraph, and the en-route redraw bug fix (the trace overlay previously only appeared on the next unrelated pan/zoom) is real and necessary.
- **Activation + warning** — toolbar Trace button (disabled without selection) and right-click "Trace from here" (promoted to the primary context-menu section) share `startTraceFromNode`; when the loaded graph has zero causal edges it shows a `role="alert"` banner with the exact spec text "Call graph not available — run code sync to extract call edges" and issues no query.
- **Exit** — toolbar "Clear trace" (replaces Trace while active), window-level Escape (inputs excluded; does not clear non-trace queries), right-click "Clear trace", new trace replaces.
- **Rendering** — numbered badges in BFS order, red trace arrows with edge labels, `dim_unmatched: 0.2` on the trace query (unmatched edges ×0.1 via the query edge filter).
- **Test quality (probed, not assumed):**
  - Focused run `npx vitest run causal-trace.test.js ExplorerCanvasTrace.test.js ExplorerCanvas.test.js NodeDetailPanel.test.js` after fresh `npm ci` → **249/249 passed** (`/tmp/stage/review-evidence/task-081-focused-vitest-rerun.log`, exit 0).
  - ExplorerView-dependent regression suites (ExplorerView-responsive/Scope/DrillUrl, RepoMode, RepoModeHeader) → **62/62 passed** (`explorerview-regression.log`, exit 0).
  - **Mutation probe 1**: BFS→DFS in `computeTraceOrder` (queue.shift → queue.pop) → 5/21 tests fail, including the BFS-vs-DFS ordering kill test (`mutation1-dfs.log`, exit 1; file restored, tree clean).
  - **Mutation probe 2**: `dim_unmatched: 0.2` → `0.12` in `startTraceFromNode` → toolbar activation test fails on the pinned 0.2 (`mutation2-dim.log`, exit 1; file restored, tree clean).
- **Bookkeeping** — coverage row §6 `task-assigned` → `implemented` with detailed evidence note; task `commits:` frontmatter lists the product commit; protected acceptance checkboxes left unchecked per the assigned contract (durable finding from the prior round honored); the un-numbered-badge rendering (Arabic numerals in red circles rather than literal ①②③ glyphs) satisfies the criterion's substance.

## Findings

### F1 — Trace step list is destroyed by the first step click (code, `web/src/components/ExplorerView.svelte:1653-1660`)

`onTraceStepNavigate` sets `detailNode` to the **raw** graph node (`(graph.nodes ?? []).find(n => n.id === step.nodeId)`) — no `_traceSteps` — and `NodeDetailPanel` renders the trace list only when `node._traceSteps?.length > 0` (NodeDetailPanel.svelte:790). After one navigation click the list unmounts; the user cannot click a second step. The pan/zoom itself works (`focusRequest` → `zoomToNode`), but the specced interaction — walking the full trace via the panel — is one-shot.

Probe (evidence `/tmp/stage/review-evidence/probe-trace-nav.log`, exit 0): temporary test replicating the handler exactly — render NodeDetailPanel with an enriched 3-step node, click step 2, re-render with the raw node the handler selects → `.trace-step-item` count drops to **0** (the test asserts the disappearance and passes). Probe file deleted afterwards; working tree clean.

### F2 — Trace activation never populates the detail panel (code, `web/src/lib/ExplorerCanvas.svelte:4205-4214`)

`_traceSteps` is attached only inside the canvas **left-click** handler; `startTraceFromNode` (toolbar button and right-click action) issues the query but never emits a detail node, and right-click does not select the node. Spec §6 step 6 / plan item 5: "When trace mode is active, the detail panel shows a numbered list of trace steps." In both activation flows the panel does not show the trace — the list appears only after an additional left-click on a traced node, and in the toolbar flow the panel still holds the pre-trace plain node.

Probe (evidence `/tmp/stage/review-evidence/probe-trace-activation.log`, exit 0): temporary test — render ExplorerCanvas with a selected node, click `toolbar-trace`, then right-click → `ctx-trace`, flush animation frames; `onNodeDetail` is **never** called with a node carrying `_traceSteps` (assertion `total === 0` passes). Probe file deleted afterwards; working tree clean.

## Notes for the reviser

- Both findings share a root cause: the trace step list lives as node-attached enrichment produced only by the canvas click handler. Carrying the step list on the navigation target (F1) and emitting it from `startTraceFromNode` once the trace effect settles (F2) closes §6 step 6 without touching the traversal or rendering layers, which are solid.
- The existing ExplorerCanvasTrace tests pass because they either render NodeDetailPanel with a hand-attached `_traceSteps` fixture or assert the click dispatch — neither covers the host (`ExplorerView`) side of the contract. A regression test for F1/F2 should exercise the handler swap (as the probes did) rather than the panel in isolation.
- Full `npm test` in `web/` was not run here (focused probes only, per review scope); the verifier should run the complete suite. All evidence is under `/tmp/stage/review-evidence/`.
