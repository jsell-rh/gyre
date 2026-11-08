# Review — task-081 (Explorer Canvas — Causal Flow Trace, explorer-canvas.md §6)

Candidate: `714d1fc6d8337c5bd0086a37e4a54987107ab239` (assignment base
`39c7759f6b8fbc55adcdf00072cf644201fb790e`). Verdict: **approved**.

## Round 1 (this review)

Independent re-review after the base merge. All probes run at the candidate
HEAD; evidence and logs under `/tmp/stage/review-evidence/`
(round summary: `task-081-review-714d1fc6d.md`).

### Scope and merge integrity

- All six frontmatter commits (88197c55, 3474c930, 3cb7e984, e72c00a8,
  540d2e67, 67005b89) are ancestors of the candidate.
- The eight §6 source/test files at HEAD are byte-identical to the previously
  reviewed candidate 88197c55 (`git diff --stat 88197c55 HEAD -- <surface>`
  → empty); the base-side delta (task-085) touches web/src only in
  Inbox/locales, so the merge introduced no conflicts on this surface.
- The candidate delta from base is exactly the §6 surface, one regenerated
  toolbar visual baseline (`toolbar-default-linux.png`, attributable to the
  added Trace toolbar button), the base merge, and a process commit touching
  only the task's shipped notes.

### Behavior verified against §6 and the acceptance criteria

Real production surface (ExplorerCanvas via ExplorerView), not a stand-in:
pure BFS traversal module (`web/src/lib/causal-trace.js` — cycle-safe,
deterministic sibling ordering, deleted-edge and dangling-endpoint handling)
consumed by the canvas `$effect` restricted to the query-matched subgraph;
right-click "Trace from here" (primary context-menu section) and toolbar
Trace button share `startTraceFromNode`; "Clear trace" toolbar/context-menu
exits; window-level Escape in the standalone canvas with a `windowEscape`
opt-out for ExplorerView's own Escape cascade; exact spec warning text
"Call graph not available — run code sync to extract call edges" as a
dismissible `role="alert"` banner when the graph has no causal edges;
numbered step badges with governing-spec badges (`governingSpecFor` checks
`spec_path` and `GovernedBy` edges); thick red trace-path arrows with
edge-type labels; unreachable nodes dim to 20% (`dim_unmatched: 0.2`);
detail panel populated on activation (enriched start node emission, deduped
per start node) with a clickable step list whose clicks pan/zoom the canvas
(`focusRequest` → `zoomToNode`) and carry the step list across navigation
(`trace-navigation.js`).

### Probes

- Focused: the five trace-surface suites (causal-trace, ExplorerCanvasTrace,
  ExplorerViewShortcuts, NodeDetailPanel, ExplorerCanvas) → 263 passed,
  0 failed, exit 0.
- Full `npm test -- --no-file-parallelism` in web/ (acceptance criterion):
  69 files, 1716 passed | 41 skipped, exit 0 — including the previously
  failing ExplorerViewShortcuts Escape-cascade regression test.
- Kill probes (mutations reverted immediately; tree clean after each):
  BFS→DFS → 5 failures; `dim_unmatched` 0.2→0.5 → 1 failure; Escape handler
  disabled → 1 failure; `hasCausalEdges` forced true (warning suppressed) →
  2 failures. The tests pin the specced behaviors, not structure.
- Minor observation (not a finding): removing the end-of-effect
  `untrack(() => scheduleRedraw())` does not fail any test — the badge tests
  render with `activeQuery` at mount where the `zoom: 'fit'` camera
  animation already drives redraws. That call is a robustness fix for the
  settled-camera case, not an acceptance criterion, and no specced behavior
  regresses without it on the tested paths.

### web/dist disposition (durable finding 48a7907a)

Committed web/dist at the candidate equals the assignment-base dist and has
no §6 bundle markers. Verified directly against the executor's
protected-publication stage (`pipeline-artifacts.py`), which itself restores
web/dist from the assignment base, rebuilds remotely, and commits
`build(web): regenerate verified assets (task)`; the review source-guard
likewise excludes web/dist from review scope. The finding's underlying
concern (a permanently trace-less served UI under `SKIP_WEB_BUILD=1`) is
resolved at that stage; committing a branch-local rebuild would be restored
to base by the executor regardless. Not a defect of this candidate.

### Notes for remote verification

E2E visual toolbar tests were not re-run locally (live-server Playwright
runs; the toolbar baseline delta is the single regenerated
`toolbar-default-linux.png` with prior-round pixel-identical CI evidence on
record, and the local WebSocket upgrade rejection is a documented sandbox
transport restriction). The verifier should run the web e2e visual suite in
the unrestricted environment.
