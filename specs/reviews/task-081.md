# Review — task-081 (Explorer Canvas — Causal Flow Trace, explorer-canvas.md §6)

Candidate `37112f4a` (base `66a5fc6`). Verdict: **approved**.

## Evidence (independent, this round)

All probes at the exact candidate checkout; logs under `/tmp/stage/review-evidence/`:

- `npm ci` in `web/` (locked versions), then:
- `npx vitest run ExplorerCanvasTrace.test.js causal-trace.test.js` → **47/47 passed**, exit 0 (`ev1-vitest-trace.log`).
- Sibling suites (`ExplorerCanvas`, `ExplorerCanvasDrillDown`, `NodeDetailPanel`, `ExplorerViewScope`, `ExplorerViewDrillUrl`, `ExplorerView-responsive`, `ExplorerCanvas-performance`) → **245/245 passed**, exit 0 (`ev2-vitest-siblings.log`) — no regressions in touched components.
- Full acceptance probe `npm test` in `web/` → **66 files, 1673 passed, 0 failed** (41 pre-existing skips), exit 0 (`ev6-npm-test-full.log`).
- **Kill test 1** (dim factor): temporarily changed production `dim_unmatched: 0.2 → 0.12` in `startTraceFromNode` → trace suite **fails** (the activation test pins the issued query's dim constant); restored, suite green again (`ev3-kill-dim-factor.log`). Note: the *rendering* dim test feeds a hand-built query, so the 0.2 constant is pinned by the activation test — the pair together catches the production change, which is what the kill test demonstrated.
- **Kill test 2** (detail-panel emission): temporarily disabled the `emitTraceDetailFromEffect(order, pathEdges)` call → **exactly the 4 activation tests fail**, matching the shipped claim; restored, 26/26 green (`ev4-kill-emission.log`). Working tree verified clean after both kills (`git status` empty).
- **CI repair verification** (durable finding: E2E run 42339252706, `toolbar_renders_with_lens_toggle_and_stats` failed 1607px/0.03 vs old baseline): pure-Python PNG decode+compare (`cmp_baseline.py`, `ev5-baseline-comparison.log`):
  - new committed `toolbar-default-linux.png` vs the CI **actual** artifact of that run → **IDENTICAL, 0/66168 pixels differ**;
  - new baseline vs old baseline → 3824/66168 differ, bbox confined to the toolbar row band (x139-718, y44-57) — the Trace-button re-flow, nothing else;
  - `git diff 790dff2a(CI head)..37112f4a -- web/src web/tests` → **only the baseline PNG** (JS byte-identical), and `git diff base..candidate -- web/tests/e2e/` shows the E2E spec source untouched (same locator, same `maxDiffPixelRatio: 0.02`); sibling `toolbar-evaluative` baseline untouched. The regeneration therefore records exactly what CI renders from this source — not a masked regression.
  - `crates/gyre-server/build.rs` always rebuilds web from `web/src` when `SKIP_WEB_BUILD` is unset (as in `e2e.yml`), so the committed-dist state cannot affect the E2E run.
- `scripts/check-task-commit-attribution.sh` → OK (all task-labeled product commits in frontmatter).
- `check-conditional-test-guards.sh`, `check-aspirational-test-names.sh` → pass. `check-assertionless-tests.sh`, `check-comparative-test-claims-js.sh`, `check-dead-test-{code,variables-js}.sh` exit 2 **locally only**: mawk 1.3.4 chokes on the scripts' gawk regexes and dies on the pre-existing `AccordionItem.test.js` (alphabetically first) before ever reaching the candidate's files — environment restriction, not a code defect; CI (gawk) passed these at `790dff2a` whose JS is byte-identical to the candidate. The comparative-claims word list was also checked manually against the new test names: no matches.

## Verified working (spec §6, all six steps + acceptance criteria)

- **Traversal**: `web/src/lib/causal-trace.js` `computeTraceOrder` — real BFS over outgoing `calls`/`routes_to` (plus `routesto` serialization variant), cycle-safe visited set, alphabetical sibling tie-break, dangling endpoints and non-causal types excluded, deleted edges skipped. Consumed by the canvas `$effect` restricted to the query-matched subgraph so step numbers agree with dimming. 21 unit tests including the BFS-vs-DFS ordering kill test.
- **Activation**: right-click "Trace from here" (primary context menu, `ctx-trace`) and toolbar **Trace** button (disabled with hint when nothing selected), both through shared `startTraceFromNode`; production round-trip verified end-to-end (`onInteractiveQuery` → ExplorerView `activeViewQuery` → `activeQuery={activeViewQuery}` prop at ExplorerView.svelte:1417/1442).
- **Warning path**: zero causal edges → dismissible `role="alert"` banner with the exact spec text "Call graph not available — run code sync to extract call edges", no query issued; both activation paths tested.
- **Rendering**: numbered red step badges in BFS order; thick red (width 2.0) trace arrows with edge-type labels; unreachable nodes dim to 20% (`dim_unmatched: 0.2`, rendering test pins the 0.2 ratio against a no-query baseline of the same fixture); unmatched edges ×0.1 and non-causal types filtered out in the production draw path (ExplorerCanvas.svelte:3648-3655).
- **Spec annotation**: `governingSpecFor` resolves `spec_path` or `GovernedBy` edge target; badge beside the step number on canvas and per-step in the detail list.
- **Detail panel**: trace effect emits the start node enriched with `_traceSteps`/`_traceStep`/`_traceTotal` on activation (both flows), deduped per start node so data reloads don't clobber the selection; step clicks pan/zoom via `onTraceStepNavigate` → ExplorerView `focusRequest {nodeId, nonce}` → canvas `zoomToNode`, and `traceStepTargetNode` carries the step list onto the navigation target so the list persists across clicks (the prior round's regression, kill-verified fixed).
- **Exit**: toolbar "Clear trace", right-click "Clear trace", window-level `Escape` (inputs excluded, non-trace queries unaffected), and a new trace replaces the current one — all tested.
- **Coverage matrix** §6 → `implemented` with accurate notes; task checkboxes left untouched per the protected contract; commit attribution complete.

## Non-blocking notes

- The pipeline checkpoint commit `3474c930` restored committed `web/dist` to the base state (matching `dev-check.sh`'s `git restore --worktree -- web/dist`), so the committed dist predates the trace feature. No CI gate consumes committed dist (default builds run `build.rs`, which rebuilds from `web/src`; `SKIP_WEB_BUILD=1` is a Rust-only dev convenience with the same staleness posture as the base). Recorded for the verifier; not a task-081 defect.
- Local E2E re-run not performed (sandbox server/WebSocket transport restriction, consistent with prior rounds). The pixel-identity proof against the actual CI artifact of run 42339252706 — rendered by GitHub's runner from byte-identical JS — is the stronger evidence and stands in for it. Remote verification should re-run `npx playwright test explorer-visual` on the PR.
