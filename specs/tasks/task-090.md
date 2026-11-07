---
title: "HSI Explorer Three Lenses (Structural, Evaluative, Observable)"
spec_ref: "human-system-interface.md §3 Three Lenses"
depends_on:
  - task-065
progress: ready-for-review
coverage_sections:
  - "human-system-interface.md §3 Three Lenses"
commits:
  - 0766a167
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

- [x] Lens selector visible in Explorer toolbar (Structural, Evaluative, Observable)
- [x] Structural lens is the default and shows knowledge graph nodes/edges
- [x] Evaluative lens overlays test coverage, gate status, and spec approval on nodes
- [x] Test coverage shown as color-coded badge (green/yellow/red)
- [x] Gate status shown as edge colors (green=pass, red=fail)
- [x] Spec approval status shown as icon on spec-linked nodes
- [x] Lenses compose: Evaluative adds decorations on top of Structural layout
- [x] Observable lens is disabled with "Coming soon" tooltip
- [x] Lens selection persisted in URL query params (`?lens=evaluative`)
- [x] Switching lenses does not re-layout the graph (only changes visual encodings)
- [x] `npm test` passes in `web/`

## Agent Instructions

Read `specs/system/human-system-interface.md` §3 "Three Lenses" for the spec. Also read `specs/system/explorer-canvas.md` §3 for lens definitions. The ExplorerCanvas component is at `web/src/lib/ExplorerCanvas.svelte`. The knowledge graph data comes from `GET /api/v1/repos/:id/graph`. Gate results may be available from existing gate endpoints. Spec approval data is in the spec approval ledger. The `GraphNode` struct should have a `test_coverage` field — check if it's populated. For the Evaluative lens, you need gate result data per graph node — this may require joining gate results with the graph node's spec_ref. The Observable lens is explicitly future work — just add the disabled option. Check `specs/system/explorer-canvas.md` §4 "Lens Definitions" for any additional lens specification details.

## Shipped

The three-lens architecture (HSI §3) is complete in the Explorer: Structural
(default), Evaluative (real overlays from gate results, the spec approval
ledger, and `GraphNode.test_coverage`), and Observable (disabled placeholder
per spec "future"). The lens substrate (toolbar toggle, structural base,
trace-particle evaluative, observable banner) already existed from task-065;
this task closes the HSI §3 gaps that remained: the evaluative overlays for
test coverage / gate outcomes / spec approval, and URL persistence of the
lens selection.

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
- **Gate status edge colors** (ExplorerCanvas `drawEdges`): edges leaving a
  node governed by a spec with a concluded gate run are colored green
  (pass) / red (fail), with the latest result per gate deciding the
  outcome. Precomputed per node so the render loop stays O(1) per edge.
- **Data loading** (ExplorerView `loadEvaluativeData`): gate outcomes
  joined from recent MRs (`spec_ref` "path@sha" → per-spec most recent MR
  with concluded gates) and the spec approval ledger
  (`GET /api/v1/specs/approvals`), both best-effort, non-blocking.
- **URL persistence**: lens selection is restored from `?lens=evaluative`
  deep links on mount and synced via `history.replaceState` on switch
  (Back is not polluted with lens toggles; other params and the `#drill=`
  hash are preserved).
- **Ledger API**: `SpecApprovalResponse` now serializes `rejected_at` /
  `rejected_reason` so a rejected approval (✗) is distinguishable from a
  revoked one (back to ⏳ pending) — server round-trip test added.
- **Observable lens**: remains disabled (`aria-disabled`, tooltip
  "requires production telemetry integration") per spec — future work.

### Test evidence

- `web/src/__tests__/lens-rendering.test.js` — 27 unit tests: coverage
  thresholds and badge colors, node→spec join (spec_path, spec_paths,
  GovernedBy targets, soft-deleted edges ignored), gate pass/fail
  resolution (fail wins), approval status from ledger history, composed
  decorations, URL param read/write.
- `web/src/__tests__/ExplorerCanvas.test.js` — 8 new component tests:
  evaluative legend entries, coverage badge drawn green at 85% (and NOT in
  structural lens), gate edge/ring colors (differential red/green stroke
  assertions), approval icons ✓/⏳/✗ (and absent in structural lens), and
  lens switching preserves node label positions exactly (no re-layout).
- `web/src/__tests__/ExplorerViewLensUrl.test.js` — 5 tests: structural
  default has no param, switching to evaluative persists
  `?lens=evaluative`, switching back drops it, deep-link restore, and the
  observable lens disabled state + tooltip.
- `cargo test -p gyre-server --lib api::gates` — 7 passed (incl. the new
  `list_spec_approvals_includes_rejection_fields`).
- `npm test` in `web/` — 66 files, 1665 passed, 41 skipped (pre-existing
  skips). Production compile verified with `vite build` to a temp outDir.

### Notes

- The evaluative lens's trace-particle metrics (span duration/count/error
  rate, from task-065/087 lineage) are unchanged; the coverage/gate/
  approval overlays compose alongside them.
- Test-environment note: the shared synchronous rAF mock in
  ExplorerCanvas.test.js re-enters `animLoop` and permanently blocks
  redraws once the evaluative-lens early `scheduleRedraw` effect fires;
  the new tests install a queued (browser-ordered) rAF mock to draw frames
  deterministically. No production behavior change.
