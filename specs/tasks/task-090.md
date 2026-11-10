---
title: "HSI Explorer Three Lenses (Structural, Evaluative, Observable)"
spec_ref: "human-system-interface.md §3 Three Lenses"
depends_on:
  - task-065
progress: ready-for-review
coverage_sections:
  - "human-system-interface.md §3 Three Lenses"
commits: ["3543192816a9df2a63720ba43fe9bddbcbe4b692", "86b6f3006ebe07a38fa3f890efacb00cdfbfb320", "9ebea6a428733496e57b32d2e3b18a9cb6f24732", "54c0b32ab22c74e7a86d535779c1f83e139ab6ce", "00d6497aa6e3258482053be801849ed2e5dcdc3e", "47fca19981be89efbc6dc8ddf687f6dcf9a7f86f", "f51574568c6c3ebdd025d42892ddb27a423129eb", "7bb23bca676396d8434491cef270f6094a92a36f", "0766a16799c0cd414080d98b0cad5f56e561e7ee"]
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
trace-particle evaluative, observable banner) came from task-065; this task
closed the remaining HSI §3 gaps — the evaluative overlays for test coverage
/ gate outcomes / spec approval, and URL persistence of the lens selection —
composed with the task-086 liveness base.

### Shipped behavior

- **Lens decoration pipeline** (`web/src/lib/lens-rendering.js`): pure
  functions composing decorations on a base node/edge — coverage badge
  classification (green >= 70%, yellow >= 30%, red below, from
  `GraphNode.test_coverage`), node → governing-spec join (`spec_path`, N:M
  `spec_paths`, GovernedBy edge targets), gate status resolution (a failing
  gate on any governing spec fails the node), approval status from ledger
  records (active → approved; rejected → rejected; revoked → pending again;
  spec-linked with no record → pending), and the `?lens=evaluative` URL
  helpers (structural default drops the param).
- **Evaluative node decorations** (ExplorerCanvas `drawLeafNode`): coverage
  badge (green/yellow/red, % label), gate status ring, approval icon
  (✓/⏳/✗) — drawn on top of the unchanged structural fill, spec border, and
  label (lenses compose; the overlay never replaces structural rendering).
- **Gate status edge colors** (ExplorerCanvas `drawEdges`), evaluative-only:
  the task-086 record-aggregate coloring applies first; the per-spec recency
  join (`gateStatusByNodeId`: most recent MR with concluded gates per spec
  path, latest run per gate decides) overrides it for edges leaving a
  governed node.
- **Data loading** (ExplorerView `loadEvaluativeData`): gate outcomes joined
  from this repo's recent MRs (repo-scoped `repository_id` + server-side
  `per_page: 10`, newest first) and the spec approval ledger
  (`GET /api/v1/specs/approvals`), best-effort and non-blocking.
- **URL persistence**: lens restored from `?lens=evaluative` deep links on
  mount, synced via `history.replaceState` on switch (other params and the
  `#drill=` hash preserved).
- **Ledger API**: `SpecApprovalResponse` serializes `rejected_at` /
  `rejected_reason` so a rejected approval (✗) is distinguishable from a
  revoked one (back to ⏳ pending).
- **Observable lens**: disabled (`aria-disabled`, tooltip "requires production
  telemetry integration") per spec — future work.
- **Evaluative toolbar legend** (Coverage ok/partial/low, Gate pass/fail,
  Approved/Pending/Rejected) documents the HSI §3 overlays; the
  `toolbar-evaluative-linux.png` e2e baseline was regenerated once
  (`9ebea6a428`) when that legend legitimately wrapped the toolbar to a
  second row (same locator, same `maxDiffPixelRatio`, no assertion weakened).

### Reconciliation with the current base (this round)

The independently reviewed implementation (retained source checkpoint
`d00990bc`) was reconciled with the current contract base `078ec12a` (merge
`7d3313d51`, working tree clean, no unresolved conflicts):

- All task-090 source files are intact after the merge: `lens-rendering.js`,
  `ExplorerView.svelte`, and the three lens test files are byte-identical to
  the reviewed checkpoint; `ExplorerCanvas.svelte` and `gates.rs` carry the
  new-base evolution (notifications, canvas density policy, ViewEvents) on
  top of the lens features — both feature sets coexist.
- `ExplorerViewLensUrl.test.js` drills into the repo via the current
  workspace realized-architecture graph (double-click on the rendered repo
  node, task-174) instead of the retired workspace repo cards.
- Generated `web/dist` is restored from the assignment base per controller
  policy; final integration rebuilds and publishes those assets remotely
  from source. No dist files were committed in this round.
- Full e2e/GitHub-CI validation is owned by verification/publication; no
  visual baseline was changed in this round (no current failure artifacts to
  act on).

### Test evidence (this round, branch HEAD `7d3313d51`)

- `vitest run lens-rendering.test.js ExplorerCanvas.test.js
  ExplorerViewLensUrl.test.js` — 191/191 passed, exit 0.
- `vitest run ExplorerCanvasLiveness ExplorerViewLivenessWiring
  ExplorerViewWorkspaceGraph ExplorerViewViewEvents` — 35/35 passed, exit 0
  (task-086 liveness and new-base canvas features unaffected by the merge).
- `cargo test -p gyre-server --lib api::gates` — 12 passed (incl.
  `list_spec_approvals_includes_rejection_fields`), exit 0.
- Probe commands, exit codes, and output retained at
  /tmp/stage/review-evidence/task-090-reconcile (controller-side).

Acceptance criteria left unchecked for independent review.
