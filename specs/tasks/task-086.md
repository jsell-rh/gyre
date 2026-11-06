---
title: "HSI Explorer Liveness Indicators (Two Layers)"
spec_ref: "human-system-interface.md §3 Design Principle"
depends_on:
  - task-065
progress: not-started
coverage_sections:
  - "human-system-interface.md §3 Design Principle: The Built Software Is Always Alive"
commits: []
---

## Spec Excerpt

**Two layers of liveness on the canvas:**

**Layer 1 — SDLC activity (Gyre's work):** Ambient indicators of what agents are building:
- **Active agents:** nodes where agents are working pulse with a subtle glow (sourced from `RunStarted`/`RunFinished` Telemetry-tier messages).
- **Recent changes:** nodes modified in the last hour have a fading highlight ring (sourced from `ArchitecturalDelta` records).
- **Gate status:** edges show gate pass/fail as color (green/red) when the Evaluative lens is active.

**Layer 2 — Test-time traces (the built software's behavior):** Animated data flow showing real requests flowing through the architecture, captured during gate execution.

## Implementation Plan

1. **Layer 1: Active agent indicators:**
   - Subscribe to WebSocket messages for `RunStarted`/`RunFinished` Telemetry-tier messages
   - Track which graph nodes are associated with currently running agents (agent → task → spec → graph nodes)
   - Apply a pulsing CSS animation (subtle glow) to affected nodes on the canvas
   - Remove the glow when `RunFinished` is received

2. **Layer 1: Recent change highlights:**
   - On canvas load, query recent `ArchitecturalDelta` records for the current repo/workspace
   - Identify graph nodes that were added or modified in the last hour
   - Apply a fading highlight ring animation (CSS: ring appears at full opacity, fades over 60 minutes)
   - Use `GET /api/v1/repos/:id/graph/timeline?since=<epoch-1h>` or similar endpoint

3. **Layer 1: Gate status on edges:**
   - When the Evaluative lens is active, overlay gate results on edges
   - Green edges: all gates passed for the connected nodes
   - Red edges: at least one gate failed
   - Data sourced from gate evaluation records for the current scope
   - Only visible when Evaluative lens is active (default Structural lens hides gate colors)

4. **Layer 2: Test-time trace data flow (initial scaffolding):**
   - Check if trace data exists for the current context (`GET /api/v1/merge-requests/:id/trace`)
   - If trace data exists, show a ▶ play icon on nodes with trace data
   - Clicking ▶ starts animated particles flowing through edges following the span tree
   - Full animation implementation (particles, time scrubbing, speed control) is a follow-on task — this task provides the scaffolding and data wiring

## Acceptance Criteria

- [ ] Nodes with active agents display a pulsing glow animation
- [ ] Glow starts on `RunStarted` and stops on `RunFinished` for relevant nodes
- [ ] Recently modified nodes (last hour) show a fading highlight ring
- [ ] Highlight ring fades over time (not instant on/off)
- [ ] When Evaluative lens is active, edges show green/red gate status colors
- [ ] Gate status colors are NOT shown in Structural lens (default)
- [ ] Layer 2 scaffolding: play icon appears on nodes with trace data
- [ ] Canvas performance is not degraded by liveness indicators
- [ ] `npm test` passes in `web/`

## Agent Instructions

Read `specs/system/human-system-interface.md` §3 "Design Principle: The Built Software Is Always Alive" for the two-layer design. The ExplorerCanvas component is at `web/src/lib/ExplorerCanvas.svelte`. For Layer 1, you need WebSocket message subscriptions — check how the existing WebSocket connection is set up in the frontend. For `ArchitecturalDelta` records, check `specs/system/realized-model.md` §3. For gate results, check `specs/system/agent-gates.md`. The trace endpoint `GET /api/v1/merge-requests/:id/trace` exists (registered at line 327 in api/mod.rs). Focus on Layer 1 completion and Layer 2 scaffolding — the full animated trace visualization (particles, time scrubbing) is a complex follow-on task.
