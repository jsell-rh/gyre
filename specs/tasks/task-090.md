---
title: "HSI Explorer Three Lenses (Structural, Evaluative, Observable)"
spec_ref: "human-system-interface.md §3 Three Lenses"
depends_on:
  - task-065
progress: complete
coverage_sections:
  - "human-system-interface.md §3 Three Lenses"
commits: ["00d6497aa6e3258482053be801849ed2e5dcdc3e", "47fca19981be89efbc6dc8ddf687f6dcf9a7f86f", "f51574568c6c3ebdd025d42892ddb27a423129eb", "7bb23bca676396d8434491cef270f6094a92a36f", "0766a16799c0cd414080d98b0cad5f56e561e7ee", "54c0b32ab22c74e7a86d535779c1f83e139ab6ce"]
---

## Spec Excerpt

Every Explorer view can be overlaid with one of three lenses:

| Lens | What It Shows | Data Source |
|---|---|---|
| **Structural** (default) | Interfaces, boundaries, data shapes, dependencies | Knowledge graph nodes + edges |
| **Evaluative** | Test results, gate outcomes, spec assertion status | Gate results, spec approval ledger |
| **Observable** (future) | SLIs, error rates, latency per endpoint | Production telemetry (not yet implemented) |

The lenses compose — you can view the domain model (structural) with test coverage overlay (evaluative) to see "which types have tests?"

## Implementation Plan

1. **Lens selector in Explorer toolbar:**
   - Add a lens toggle/dropdown to the ExplorerCanvas toolbar
   - Three options: Structural (default), Evaluative, Observable (disabled/greyed out with "Coming soon")
   - Lens selection persists in URL query params (`?lens=evaluative`)

2. **Structural lens (default):**
   - This is the baseline: nodes show types, endpoints, modules with their relationships
   - Data from knowledge graph (existing `GET /api/v1/repos/:id/graph`)
   - Node styling: categorical colors by node_type
   - Edge styling: dependency arrows, interface implementation edges

3. **Evaluative lens overlay:**
   - When activated, overlay gate results and spec status onto existing nodes
   - **Test coverage:** nodes show coverage percentage (green/yellow/red heatmap)
   - **Gate status:** edges colored green (pass) / red (fail) based on most recent gate results
   - **Spec assertion status:** nodes with spec assertions show pass/fail badges
   - **Spec approval:** spec-linked nodes show approval status icon (✓ Approved, ⏳ Pending, ✗ Rejected)
   - Data sources:
     - Gate results from gate evaluation records
     - Spec approval from spec approval ledger
     - Test coverage from `GraphNode.test_coverage` field

4. **Lens composition:**
   - Evaluative overlays compose on top of Structural base
   - The Structural lens provides the graph layout; Evaluative adds visual encodings (colors, badges)
   - A node can show its type (structural) AND its test coverage (evaluative) simultaneously

5. **Observable lens (placeholder):**
   - Render as disabled in the lens selector
   - Tooltip: "Observable lens requires production telemetry integration (future)"
   - When §10 Design Constraints are met, this lens will show SLIs, error rates, latency

6. **Lens-specific node rendering:**
   - Create a lens rendering pipeline that accepts a base node and applies lens-specific decorations
   - Structural: type icon, label, edge arrows
   - Evaluative: coverage badge, gate status ring, spec assertion icon
   - Observable (future): latency badge, error rate indicator

## Acceptance Criteria

- [ ] Lens selector visible in Explorer toolbar (Structural, Evaluative, Observable)
- [ ] Structural lens is the default and shows knowledge graph nodes/edges
- [ ] Evaluative lens overlays test coverage, gate status, and spec approval on nodes
- [ ] Test coverage shown as color-coded badge (green/yellow/red)
- [ ] Gate status shown as edge colors (green=pass, red=fail)
- [ ] Spec approval status shown as icon on spec-linked nodes
- [ ] Lenses compose: Evaluative adds decorations on top of Structural layout
- [ ] Observable lens is disabled with "Coming soon" tooltip
- [ ] Lens selection persisted in URL query params (`?lens=evaluative`)
- [ ] Switching lenses does not re-layout the graph (only changes visual encodings)
- [ ] `npm test` passes in `web/`

## Agent Instructions

Read `specs/system/human-system-interface.md` §3 "Three Lenses" for the spec. Also read `specs/system/explorer-canvas.md` §3 for lens definitions. The ExplorerCanvas component is at `web/src/lib/ExplorerCanvas.svelte`. The knowledge graph data comes from `GET /api/v1/repos/:id/graph`. Gate results may be available from existing gate endpoints. Spec approval data is in the spec approval ledger. The `GraphNode` struct should have a `test_coverage` field — check if it's populated. For the Evaluative lens, you need gate result data per graph node — this may require joining gate results with the graph node's spec_ref. The Observable lens is explicitly future work — just add the disabled option. Check `specs/system/explorer-canvas.md` §4 "Lens Definitions" for any additional lens specification details.

## Shipped

The three-lens architecture (HSI §3) is complete in the Explorer: Structural
(default), Evaluative (real overlays from gate results, the spec approval
ledger, and `GraphNode.test_coverage`), and Observable (disabled placeholder
per spec "future"). The lens substrate (toolbar toggle, structural base,
trace-particle evaluative, observable banner) already existed from task-065;
this task closed the remaining HSI §3 gaps: the evaluative overlays for
test coverage / gate outcomes / spec approval, and URL persistence of the
lens selection. The candidate is merged with the task-086 liveness base
(both feature sets coexist in ExplorerCanvas/ExplorerView).

### Shipped behavior

- **Lens decoration pipeline** (`web/src/lib/lens-rendering.js`): pure
  functions that compose decorations on a base node/edge. Coverage badge
  classification (green >= 70%, yellow >= 30%, red below, from
  `GraphNode.test_coverage`), the node → governing-spec join (`spec_path`,
  N:M `spec_paths`, GovernedBy edge targets), gate status resolution (a
  failing gate on any governing spec fails the node), approval status
  resolution from ledger records (active → approved; rejected → rejected;
  revoked → pending again; spec-linked with no record → pending), and the
  URL helpers (`?lens=evaluative`, structural default drops the param).
- **Evaluative node decorations** (ExplorerCanvas `drawLeafNode`): coverage
  badge (green/yellow/red, % label), gate status ring (green/red outline),
  and approval icon (✓/⏳/✗) — all drawn on top of the unchanged
  structural fill, spec border, and label (lenses compose; the overlay
  never replaces the structural rendering).
- **Gate status edge colors** (ExplorerCanvas `drawEdges`): two composing
  sources, both evaluative-only. The task-086 record-aggregate coloring
  (`edgeGateStatus` over `gateRecords`, both endpoints, failed wins)
  applies first; the task-090 per-spec recency join
  (`gateStatusByNodeId` from `gateStatusBySpec`: most recent MR with
  concluded gates per spec path, latest run per gate decides) overrides
  it for edges leaving a governed node — "most recent gate results" per
  HSI §3. Precomputed per node so the render loop stays O(1) per edge;
  the legend shows the aggregate gate entries only while the per-spec
  join has no data (no duplicate entries).
- **Data loading** (ExplorerView `loadEvaluativeData`): gate outcomes
  joined from this repo's recent MRs (queried repo-scoped with
  `repository_id` plus a server-side `per_page: 10` cap — the endpoint
  returns the newest MRs first — so the per-MR `mrGates` fan-out is
  bounded by the same ≤10 bound the task-086 liveness loaders uphold;
  `spec_ref` "path@sha" → per-spec most recent MR with concluded gates)
  and the spec approval ledger (`GET /api/v1/specs/approvals`), both
  best-effort, non-blocking. It runs alongside the task-086 loaders
  (`loadGateData`, `loadLivenessTimeline`) and the trace loader, all
  repo-scoped.
- **URL persistence**: lens selection is restored from `?lens=evaluative`
  deep links on mount and synced via `history.replaceState` on switch
  (Back is not polluted with lens toggles; other params and the `#drill=`
  hash are preserved).
- **Ledger API**: `SpecApprovalResponse` serializes `rejected_at` /
  `rejected_reason` so a rejected approval (✗) is distinguishable from a
  revoked one (back to ⏳ pending) — server round-trip test included.
- **Observable lens**: disabled (`aria-disabled`, tooltip "requires
  production telemetry integration") per spec — future work.

### Test evidence

- `web/src/__tests__/lens-rendering.test.js` — 27 unit tests: coverage
  thresholds and badge colors, node→spec join (spec_path, spec_paths,
  GovernedBy targets, soft-deleted edges ignored), gate pass/fail
  resolution (fail wins), approval status from ledger history, composed
  decorations, URL param read/write.
- `web/src/__tests__/ExplorerCanvas.test.js` — lens component tests
  within the suite: evaluative legend entries, coverage badge drawn green
  at 85% (and NOT in structural lens), gate edge/ring colors, a
  differential EDGE-stroke test isolating the per-spec gate edge coloring
  by path geometry (arrows vs roundRect rings — see the repair note below),
  approval icons ✓/⏳/✗ (and absent in structural lens), and lens switching
  preserves node label positions exactly (no re-layout).
- `web/src/__tests__/ExplorerCanvasLiveness.test.js` +
  `ExplorerViewLivenessWiring.test.js` (task-086) still pass unchanged:
  the record-aggregate gate edges, legend entries, and the ≤10
  per_page/repository_id wiring contract hold on the merged candidate.
- `web/src/__tests__/ExplorerViewLensUrl.test.js` — 6 tests: structural
  default has no param, switching to evaluative persists
  `?lens=evaluative`, switching back drops it, deep-link restore, the
  observable lens disabled state + tooltip, and the evaluative gate-data
  load being repo-scoped: all three loaders fire on graph load and every
  `mergeRequests` call carries `repository_id` with no `repo_id` param
  (fails if the evaluative loader regresses to `repo_id` or is dropped).
- `cargo test -p gyre-server --lib api::gates` — 7 passed (incl.
  `list_spec_approvals_includes_rejection_fields`).
- Full `npm test` in `web/` on the repaired head (after the base-72ad2a128
  merge and review repair 2): 80 files, 1865 passed / 41 skipped (exit 0);
  focused 6-suite run (ExplorerCanvas,
  ExplorerCanvasLiveness, ExplorerViewLensUrl, ExplorerViewLivenessWiring,
  lens-rendering, liveness): 237/237 passed (exit 0, incl. the new
  differential edge-stroke test). Logs under /tmp/stage/review-evidence.

### Review repair: unresolved merge conflicts with the task-086 base

The review round (repair id `ff43281d44144e52974cfe348ba1ed5e`) found
that pipeline checkpoint 8adb4b914 merged base 2100127c4 (task-086
liveness indicators) with literal conflict markers committed in
ExplorerCanvas.svelte and ExplorerView.svelte — both files failed to
compile and every Explorer suite was dead. Fixed in product-surface
commit `47fca19981be89efbc6dc8ddf687f6dcf9a7f86f` (originally
818eb1c9; the SHA changed in the review-repair-2 subject rewrite below,
same tree) by resolving all seven blocks so both feature sets coexist (details under "Shipped
behavior" above). Two reconciliations were forced by genuinely
overlapping contracts:

- **Edge gate coloring**: both tasks color evaluative edges by gate
  status. The per-spec recency join (this task) takes precedence over
  the record aggregate (task-086) when both data sources are present;
  each side's tests pass their own props and remain green.
- **MR list params**: this task's earlier round-16 fix queried
  `{ repository_id }` with a 50-MR client-side window on the premise
  that the endpoint had no `per_page` support. The task-086 base added
  server-side `per_page` (newest first) and
  ExplorerViewLivenessWiring pins a ≤10 fan-out bound on every
  `mergeRequests` call. `loadEvaluativeData` now sends
  `{ repository_id, per_page: 10 }`, and the LensUrl repo-scoping test
  asserts the underlying contract (every call `repository_id`-scoped,
  never `repo_id`, all three loaders firing) instead of the old
  exact-object match. The out-of-scope `loadTraceData` `repo_id` misuse
  flagged in round 16 was already fixed by the base (task-086 commit
  a3ad5d028).

Acceptance criteria left unchecked for independent review.

### Review repair 2: commit-subject task label + missing edge-coloring differential test

The review round (repair id `0fbe07ef9f9c4af9801fd82f3ef2a665`) found
 two findings on the candidate:

- **CI invariant (process)**: `scripts/check-task-commit-attribution.sh`
  exited 1 because product-surface commit 818eb1c9's subject ('fix(task-090):
  resolve rebase conflicts with task-086 liveness code') referenced bare
  `task-086`, and 818eb1c9 is recorded only in task-090's frontmatter.
  Fixed by rewriting the commit subject to 'fix(task-090): resolve rebase
  conflicts with the liveness-indicators base' (commit body, which explains
  the task-086 reconciliation in detail, is unchanged — the check scans
  subjects only). The history rewrite preserved topology exactly: the
  amended commit is `47fca19981be89efbc6dc8ddf687f6dcf9a7f86f`, the two
  process commits were cherry-picked, and the merge with base 72ad2a128
  was recreated via `git commit-tree` with the identical tree
  (d94b2e9666bf82b40845590a92991de1c044b46b). The frontmatter now records
  the rewritten SHA.
- **Test (mutation-surviving coverage)**: the per-spec gate edge coloring
  had no test that fails when it is removed — the reviewer's mutation
  probe (`if (false && lens === 'evaluative')` on the drawEdges
  gateStatusByNodeId block) left all 198 tests green because the
  nominally covering test was satisfied by the node ring's stroke colors
  alone (rings and edges share the gate greens/reds, and the assertions
  were mere stroke-color membership). Root cause of the invisibility: the
  LENS_NODES fixture never renders its governed_by edge at all — the
  spec node's tree-group is below the 450px leaf-visibility threshold at
  fit zoom, so the edge is opacity-culled in drawEdges. Fixed in commit
  `00d6497aa6e3258482053be801849ed2e5dcdc3e`: a new test captures EDGE
  strokes only, discriminated by path geometry (drawArrow shafts are
  single-lineTo no-curve paths; rings/borders go through roundRect with
  quadraticCurveTo segments), on a fixture shaped like the liveness
  gate-edge tests (single qualified-name tree so leaves and edges render
  at fit zoom). It asserts the edge leaving the governed node turns
  red on fail / green on pass while the edge leaving an ungoverned node
  keeps its structural color. Verified: passes on real code (158/158 in
  ExplorerCanvas.test.js) and FAILS under the reviewer's exact mutation
  (1 failed/157 passed). Evidence:
  /tmp/stage/review-evidence/r17-edge-coloring-test.txt,
  r17-mutation-edge-coloring-disabled.txt, r17-focused-suites.txt.
