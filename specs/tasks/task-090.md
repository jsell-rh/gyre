---
title: "HSI Explorer Three Lenses (Structural, Evaluative, Observable)"
spec_ref: "human-system-interface.md §3 Three Lenses"
depends_on:
  - task-065
progress: not-started
coverage_sections:
  - "human-system-interface.md §3 Three Lenses"
commits: []
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
