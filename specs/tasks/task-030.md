---
title: "Structural Lens — Spec Coverage Coloring & Complexity Sizing"
spec_ref: "explorer-canvas.md §Structural (default)"
depends_on: 
  - task-029
progress: complete
coverage_sections: []
commits: []
---

## Spec Excerpt

From `explorer-canvas.md` §Structural:

> **What it shows**:
> - Static topology: boundaries, interfaces, data shapes, dependencies
> - Spec linkage: nodes colored by spec coverage (green = governed, amber = suggested, red = no spec)
> - Complexity/churn indicators: node size reflects complexity, border thickness reflects churn

From `system-explorer.md` §1:

> Colors indicate spec coverage (green = governed by spec, amber = suggested link, red = no spec). Size indicates complexity or churn.

## Current State

**Implemented.** All acceptance criteria met:
- `ExplorerCanvas.svelte` lines 92-107: `specBorderColor()` maps spec_confidence to green/amber/orange/red
- Lines 1366-1368: `nodeWeight(n)` uses `complexity` for treemap sizing
- Line 3197: `churnThickness = Math.min(4, 1 + (n?.churn_count_30d || 0) * 0.3)` scales border thickness
- Lines 4944-4953, 5072-5076: legend with swatches for structural/evaluative modes

## Implementation Plan

1. In `ExplorerCanvas.svelte`, add spec-coverage coloring when `lens === 'structural'`:
   - `spec_confidence: High` → green border/fill (`#22c55e`)
   - `spec_confidence: Medium` → amber border/fill (`#eab308`)
   - `spec_confidence: Low` → orange border/fill (`#f97316`)
   - `spec_confidence: None` → red border/fill (`#ef4444`)
   - Apply as a secondary color layer (e.g., border or subtle background) so the node_type categorical color is still visible

2. Add complexity-based node sizing:
   - Scale node width/height proportionally to `complexity` field
   - Use a linear scale with min/max bounds (e.g., 24px min, 64px max) to prevent extreme sizes
   - Nodes with no complexity data use default size

3. Add churn-based border thickness:
   - Scale border-width proportionally to `churn_count_30d`
   - Range: 1px (zero churn) to 4px (high churn)

4. Add a legend showing the color/size encoding (small, collapsible, bottom-left of canvas).

5. Add tests for the visual encoding logic (unit test the scale functions, not the SVG rendering).

## Acceptance Criteria

- [ ] Nodes show spec coverage coloring (green/amber/orange/red) when structural lens is active
- [ ] Node size scales with complexity metric
- [ ] Border thickness scales with churn_count_30d metric
- [ ] Legend shows encoding explanation
- [ ] Encoding only applies when lens is structural (evaluative lens has its own encoding)
- [ ] Tests for scale/encoding functions

## Agent Instructions

When working on this task:
1. Update the progress field above to `in-progress`
2. Read `web/src/lib/ExplorerCanvas.svelte` for the current node rendering logic
3. Read `crates/gyre-common/src/graph.rs` for `GraphNode` field definitions (spec_confidence, complexity, churn_count_30d)
4. Implement the visual encoding in the SVG rendering path
5. Use CSS custom properties or inline styles for dynamic coloring
6. On completion, update progress to `ready-for-review` and list git commits below

## Git Commits

Implemented as part of the explorer-canvas branch prior to task decomposition.
