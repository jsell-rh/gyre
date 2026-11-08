---
title: "HSI Explorer Three Lenses (Structural, Evaluative, Observable)"
spec_ref: "human-system-interface.md §3 Three Lenses"
depends_on:
  - task-065
progress: complete
coverage_sections:
  - "human-system-interface.md §3 Three Lenses"
commits: ["86b6f3006ebe07a38fa3f890efacb00cdfbfb320", "9ebea6a428733496e57b32d2e3b18a9cb6f24732", "54c0b32ab22c74e7a86d535779c1f83e139ab6ce", "00d6497aa6e3258482053be801849ed2e5dcdc3e", "47fca19981be89efbc6dc8ddf687f6dcf9a7f86f", "f51574568c6c3ebdd025d42892ddb27a423129eb", "7bb23bca676396d8434491cef270f6094a92a36f", "0766a16799c0cd414080d98b0cad5f56e561e7ee", "3543192816a9df2a63720ba43fe9bddbcbe4b692"]
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
merged with the task-086 liveness base (both feature sets coexist in
ExplorerCanvas/ExplorerView).

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
  governed node. Precomputed per node; the legend shows the aggregate gate
  entries only while the per-spec join has no data.
- **Data loading** (ExplorerView `loadEvaluativeData`): gate outcomes joined
  from this repo's recent MRs (repo-scoped `repository_id` + server-side
  `per_page: 10`, newest first) and the spec approval ledger
  (`GET /api/v1/specs/approvals`), best-effort and non-blocking, alongside
  the task-086 loaders and the trace loader.
- **URL persistence**: lens restored from `?lens=evaluative` deep links on
  mount, synced via `history.replaceState` on switch (other params and the
  `#drill=` hash preserved).
- **Ledger API**: `SpecApprovalResponse` serializes `rejected_at` /
  `rejected_reason` so a rejected approval (✗) is distinguishable from a
  revoked one (back to ⏳ pending) — server round-trip test included.
- **Observable lens**: disabled (`aria-disabled`, tooltip "requires
  production telemetry integration") per spec — future work.

### Test evidence

- `web/src/__tests__/lens-rendering.test.js` — 27 unit tests: coverage
  thresholds/badge colors, node→spec join (spec_path, spec_paths,
  GovernedBy targets, soft-deleted edges ignored), gate pass/fail resolution
  (fail wins), approval status from ledger history, composed decorations,
  URL param read/write.
- `web/src/__tests__/ExplorerCanvas.test.js` — lens component tests:
  evaluative legend entries, coverage badge drawn green at 85% (and NOT in
  structural lens), gate edge/ring colors, a differential EDGE-stroke test
  isolating the per-spec gate edge coloring by path geometry, approval icons
  ✓/⏳/✗ (and absent in structural lens), lens switching preserves node
  label positions exactly (no re-layout).
- `web/src/__tests__/ExplorerViewLensUrl.test.js` — 6 tests: structural
  default has no param, evaluative persists `?lens=evaluative`, switching
  back drops it, deep-link restore, observable disabled state + tooltip, and
  the evaluative gate-data load being repo-scoped (all three loaders fire,
  every `mergeRequests` call carries `repository_id`, never `repo_id`).
- `web/src/__tests__/ExplorerCanvasLiveness.test.js` +
  `ExplorerViewLivenessWiring.test.js` (task-086) still pass unchanged.
- `cargo test -p gyre-server --lib api::gates` — 7 passed (incl.
  `list_spec_approvals_includes_rejection_fields`).
- Full `npm test` in `web/`: 80 files, 1865 passed / 41 skipped (exit 0).
- E2E (`npx playwright test`): 65 passed — see the CI repair below.

### CI repair: evaluative toolbar visual baseline

PR head 55064c8dc failed the E2E gate (run 42503691204):
`evaluative_lens_toggle_changes_toolbar` expected a 919x103 toolbar but the
candidate renders 919x133. Root cause: the `toolbar-evaluative-linux.png`
baseline dated from task-065; the task-090 evaluative legend (Coverage
ok/partial/low, Gate pass/fail, Approved/Pending/Rejected — asserted by the
ExplorerCanvas unit tests and required to document the HSI §3 overlays)
wraps onto an extra `flex-wrap` row, so the toolbar legitimately grew. The
rendered UI is the specced one; the baseline was stale.

Fixed in commit `9ebea6a428` by regenerating the baseline against HEAD's
committed dist with the locked playwright/chromium (no assertion weakened:
same locator, same `maxDiffPixelRatio: 0.02`, same timeout). Verified with
byte-exact local reproduction of the CI failure (identical 919x103 vs
919x133, 31764 diff pixels), the regenerated baseline is byte-identical to
the actual PNG captured by the failing CI run, the full
`explorer-visual.spec.js` suite passes (16/16, `toolbar-default.png`
unchanged), and the full e2e suite passes (65/65). Evidence retained at
/tmp/stage/review-evidence (controller-side).

### Generated web/dist disposition (repair round)

The review finding that branch HEAD's committed `web/dist` bundle equals the
pre-task-090 base bundle is confirmed and is a generated-asset issue only —
no source defect: `web/src` and `gates.rs` are byte-identical to the
independently reviewed candidate 64598aca. Per controller policy the executor
restores web/dist from the assignment base and final integration rebuilds and
publishes those assets remotely from source. Verified in this round
(evidence: /tmp/stage/review-evidence/task-090-dist): a fresh `npm ci` +
`vite build` of HEAD source (temp outDir, committed dist untouched) emits
`index-QXRGsaeH.js` byte-identical (sha256 4c80cc80…) to the verified task-090
feature bundle from 54c0b32ab, containing gateStatusBySpec, specApprovals,
lens URL persistence, and the evaluative legend labels. Focused re-run:
lens-rendering + ExplorerCanvas + ExplorerViewLensUrl — 191/191 passed.
No dist files were committed in this assignment.

Acceptance criteria left unchecked for independent review.
