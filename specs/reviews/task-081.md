# Review — task-081 (Explorer Canvas — Causal Flow Trace, Structural Lens Feature)

Spec: `specs/system/explorer-canvas.md` §6 "Causal Flow Trace (Structural Lens Feature)".
Candidate under review: `ea5fa887` (base `11b746049`, branch `pipeline/task-081/48a7907ae608445eaa3eef11b7087032-1`).
Verdict: **needs-revision** — one artifact-consistency finding (F1). The feature code itself is real, specced, and well tested; every kill probe passed.

## Round 1 — evidence

Environment: `npm ci` in `web/` (exit 0). Evidence under `/tmp/stage/review-evidence/`.

Test runs (candidate tree, `/tmp/stage/bin/npm` / `npx vitest`):

- Focused suites (`causal-trace`, `ExplorerCanvasTrace`, `ExplorerViewShortcuts`, `NodeDetailPanel`, `--no-file-parallelism`) → **113 passed**, exit 0 (`task-081-focused-suites.log`).
- Full acceptance run `npm test -- --no-file-parallelism` → **69 files / 1715 passed | 41 skipped, 0 failed**, exit 0 (`task-081-full-npm-test.log`). The prior round's durable verification failure (ExplorerViewShortcuts "Escape exits trace mode", candidate 37112f4) is repaired by 88197c55.

Kill probes (mutate → fail → restore; tree confirmed clean at `ea5fa887` after each, `task-081-kill-probes.log`):

- `dim_unmatched: 0.2 → 0.5` in `startTraceFromNode` → activation test fails (pins the production query shape).
- Dim engine disabled (`nodeOpacity` query arm → `return 1.0`) → "dims unreachable nodes to 20%" rendering test fails (the 20% dim is really rendered, not mirrored fixture logic).
- BFS → DFS (`queue.shift()` → `queue.pop()` in `causal-trace.js`) → 6 failures across the pure-traversal suite and the component step-list emission.
- `windowEscape={false} → {true}` in ExplorerView → the exact prior CI failure reproduces ("Escape exits trace mode (active view query with `_trace: true)"), confirming 88197c55's fix and that the regression test has teeth.

Verified working (no findings):

- **Traversal** — `web/src/lib/causal-trace.js` `computeTraceOrder`: BFS over outgoing `calls`/`routes_to` (plus `routesto` serialization variant), alphabetical sibling tie-break, visited-set cycle safety, dangling-endpoint and deleted-edge filtering; consumed by the canvas `$effect` restricted to the query-matched subgraph. Directly unit-tested (21 tests) with a BFS-vs-DFS kill assertion.
- **Activation** — right-click "Trace from here" (primary context-menu section) and toolbar Trace button (disabled with hint when nothing selected) both route through `startTraceFromNode`; ExplorerView's `bind:canvasState` carries `selectedNode` from canvas clicks, so the toolbar button is live in production, not just in tests.
- **Rendering** — numbered step badges in BFS order (red circle + white number = the spec's ①②③), thick red `drawArrow(..., '#ef4444', ..., 2.0)` path arrows with edge-type labels, `dim_unmatched: 0.2` on the trace query (kill-verified at both the query-shape and rendering layers). The redraw-scheduling bug fix (trace overlay appearing only after an unrelated pan/zoom) is present and exercised via the queued-rAF mock.
- **Spec annotation** — `governingSpecFor` resolves `spec_path` OR the `governed_by` edge target; drawn as a badge beside the step number and per-step in the detail list.
- **Detail panel** — activation emits the start node enriched with the full step list (`emitTraceDetailFromEffect`, deduped per start node); `NodeDetailPanel` renders the clickable list; `onTraceStepNavigate` → ExplorerView `focusRequest {nodeId, nonce}` → canvas `zoomToNode` (tracked layout lookup, untracked zoom), with `traceStepTargetNode` carrying `_traceSteps` onto the navigation target so the list survives step clicks.
- **Exit** — toolbar "Clear trace", right-click "Clear trace", window Escape in the standalone canvas (`windowEscape` default true, inputs excluded), ExplorerView's overlay cascade when embedded (opt-out verified by kill probe), and a new trace replacing the current one.
- **No call graph** — `hasCausalEdges` gate shows the exact spec text "Call graph not available — run code sync to extract call edges" as a dismissible `role="alert"` banner on both activation paths, issuing no query.
- **Coverage matrix** — §6 row updated to `implemented` with fresh code evidence; counts updated 2/7.

Findings:

- [-] **F1 (major, artifact): the candidate's committed `web/dist` is the pre-task-081 build — the verified asset build e72c00a8 was silently dropped by later commits on the branch.** `git diff 11b74604..ea5fa887 -- web/dist` is empty: the candidate's committed bundle is byte-identical to the base and contains none of the §6 surface (`toolbar-trace`, `trace-warning`, `Clear trace`, `causal` all absent from `web/dist/assets/*.js`; probe log `task-081-dist-staleness.log`). The branch *did* ship the rebuild — e72c00a8 "build(web): regenerate verified assets (task-081)" (attributed in the task's `commits:` frontmatter, and its `index-DOKLFDJR.js` bundle does contain `toolbar-trace`) — but checkpoint 3474c9305 and merge a89ddf5d4 replaced `web/dist` with the base-side state, and no later commit restored it. Consequences: `crates/gyre-server/src/spa.rs` rust-embeds `web/dist` at compile time, so every `SKIP_WEB_BUILD=1` build (the documented Rust-only build path in AGENTS.md; `scripts/pipeline-verify.py:120` builds this way) serves a UI with **no trace feature at all**, and the task frontmatter attributes a commit whose content is absent from the final tree. CI stays green only because the `e2e`/`build-release` workflows rebuild web from source. Repo precedent treats this as a defect class (task-172 round a44ba4737: "repair … stale dist"). Remedy: `cd web && npm run build`, commit the regenerated `web/dist` as a new attributed `build(web)` commit (re-doing e72c00a8 on top of the current head), and re-check `git diff <base> HEAD -- web/dist` is non-empty and contains the §6 markers.

Items recorded for remote verification (sandbox transport/build restrictions, not code defects):

- Playwright E2E (`cd web && npx playwright test explorer-visual`): needs a release server build; the prior round's local evidence (CI-failure reproduction with identical numbers, pixel-identical actual screenshot vs run 42339252706, both toolbar tests passing post-baseline-update) is consistent with this review's area estimate — the new Trace button is ~1.9% of the 919×103 evaluative-toolbar screenshot, under the 0.02 `maxDiffPixelRatio`, which explains why only `toolbar-default-linux.png` needed regeneration. The verifier should confirm both toolbar tests still pass at the repaired candidate.
