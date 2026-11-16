---
title: "Ghost Overlays — structural prediction visualization on canvas"
spec_ref: "system-explorer.md §3"
depends_on: [task-178, task-185]
progress: not-started
coverage_sections:
  - "system-explorer.md §3. Inline Spec Editing with Progressive Preview"
commits: []
---

## Spec Excerpt

system-explorer.md §3 defines Inline Spec Editing with Progressive Preview. The core feature is the **ghost overlay** — a fast, probabilistic feedback loop (2-5 seconds) that shows predicted structural impact on the architecture canvas as the user edits a spec.

**The Edit → Preview Flow (three tiers):**

1. **Instant (milliseconds)** — Link graph impact:
   - Which specs are connected, which repos implement this spec, how many graph nodes are governed
   - Shown as a subtle info bar above the editor

2. **Fast (2-5 seconds)** — Structural prediction via ghost overlays:
   - LLM analyzes spec diff against knowledge graph via `POST /api/v1/repos/{id}/graph/predict`
   - Predicts: new types/traits to add, existing types to modify, dependencies to change
   - Shown as **ghost nodes** on the canvas — dotted outlines, yellow highlights, strikethroughs
   - Canvas updates live as user edits (debounced)

3. **Thorough (minutes, on-demand)** — Full preview:
   - Click [Preview] to spawn agent on throwaway branch
   - Real code produced, architecture canvas updates with actual extracted nodes
   - Diff viewer: existing vs. preview code

**Ghost Overlay Visual Language:**

| Visual | Meaning |
|---|---|
| Dotted outline, green fill | New element predicted to be added |
| Yellow highlight on existing node | Existing element predicted to change |
| Red strikethrough on existing node | Existing element predicted to be removed |
| Dotted edge, green | New relationship predicted |
| Pulsing border | Currently being previewed (agent running) |
| Solid border (after preview) | Confirmed by actual implementation |

**Predict endpoint** (already exists): `POST /api/v1/repos/{id}/graph/predict` — request: `{spec_path, draft_content}`, response: `{predictions: [{action, node_type, name, confidence, reason, ...}], affected_specs, affected_repos, estimated_agent_cost}`. Predictions include confidence scores (higher confidence = more opaque ghost).

## Implementation Plan

1. **Ghost node rendering** (`GhostOverlay.svelte`):
   - SVG layer rendered on top of (or alongside) the regular canvas nodes
   - Ghost nodes: dotted border, semi-transparent fill, color by action (green=add, yellow=modify, red=remove)
   - Ghost edges: dotted stroke, green for new relationships
   - Opacity scales with confidence score (0.3 at 50% confidence, 1.0 at 100%)
   - Each ghost node shows a tooltip with the prediction reason

2. **Prediction trigger logic**:
   - When user edits a spec (in Editor Split or Spec tab), debounce changes (1-2 seconds)
   - On debounce fire, call `POST /repos/{id}/graph/predict` with current draft content
   - While prediction is loading, show a subtle "Predicting..." indicator
   - On response, render ghost nodes on the canvas

3. **Instant tier — link graph impact bar**:
   - When spec editing begins, immediately compute:
     - Connected specs (from spec links API: `GET /specs/:path/links`)
     - Repos implementing this spec
     - Number of graph nodes governed by this spec
   - Show as a compact info bar above the editor

4. **Thorough tier — full preview integration**:
   - [Preview] button spawns an agent on a throwaway branch
   - While agent runs, ghost nodes show pulsing border
   - When preview completes, ghost nodes become solid (confirmed by real implementation)
   - Architecture tab in Editor Split shows actual graph delta (from task-172)
   - Code Diff tab shows line-level changes

5. **Canvas integration**:
   - Ghost nodes must be positioned relative to existing nodes
   - New nodes: placed near their predicted parent/related node
   - Modified nodes: overlay on existing node position
   - Removed nodes: overlay with strikethrough on existing position
   - Ghost nodes are interactive (hoverable for tooltip) but don't trigger navigation

6. **Tests**:
   - Ghost nodes render with correct visual styles per action type
   - Confidence score maps to opacity correctly
   - Debounced prediction calls fire after edit pause
   - Ghost overlay clears when editing stops or spec is saved
   - Preview mode transitions ghosts from pulsing to solid

## Acceptance Criteria

- [ ] Ghost nodes render on canvas with correct visual language (green=add, yellow=modify, red=remove)
- [ ] Ghost edges render as dotted green lines for new relationships
- [ ] Confidence scores control ghost opacity
- [ ] Ghost tooltips show prediction reason
- [ ] Debounced prediction calls to `POST /repos/:id/graph/predict`
- [ ] Instant tier: link graph impact bar shows connected specs, repos, governed nodes
- [ ] Thorough tier: [Preview] spawns agent, ghosts pulse during execution, become solid on completion
- [ ] Ghost overlay clears when editing ends
- [ ] Tests pass

## Agent Instructions

Read `system-explorer.md` §3 "Inline Spec Editing with Progressive Preview" for the full specification. The predict endpoint is already implemented — check `crates/gyre-server/src/api/graph.rs` for the `predict_graph` handler and its request/response types. The route is at `POST /api/v1/repos/:id/graph/predict` (verified in `mod.rs`). The ghost overlay visual language must match the table in the spec exactly. The Editor Split (task-172) and LLM-Assisted Spec Editing (task-185) handle the left-side editing; this task focuses on the right-side canvas ghost visualization. For the preview agent spawn, check the agent spawn API in `api/spawn.rs`. Ghost node positioning should use a simple heuristic: place new nodes near their predicted parent or in available space near related existing nodes.
