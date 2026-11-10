---
title: "HSI Explorer Liveness Indicators (Two Layers)"
spec_ref: "human-system-interface.md §3 Design Principle"
depends_on:
  - task-065
progress: ready-for-review
coverage_sections:
  - "human-system-interface.md §3 Design Principle: The Built Software Is Always Alive"
commits: ["c4d00719ceec949c6ccd455ff80bd58c428d1d3a", "7da875649ac751dc37afe5b9e496b67ca64393db", "a3ad5d0288d7404c5f7759f7103640ab495dfc5c", "c17b6c95e66824fd3b72b21f95b9b95f2a284ec6", "2aafb8f27418b2dbd1681f6d442c26dea2677931"]
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

## Shipped

Both liveness layers of HSI §3 are implemented in production frontend
code. Product surface in commit c17b6c95; review repairs in a3ad5d02
(repo-scoped MR queries via `repository_id` + server-side `per_page`),
7da87564 (canvas-load timeline query), c4d00719 (rustfmt repair);
verified asset build 2aafb8f2.

### Layer 1 — SDLC activity

- **Active agent glow** — `web/src/lib/liveness.svelte.js` provides a
  reactive `activeRuns` map and `createLivenessTracker(wsStore,
  { fetchTask })`, wired in `App.svelte` to the workspace WebSocket
  store. The tracker listens for Telemetry-tier `run_started` /
  `run_finished` bus messages (the exact `{ kind, payload: { agent_id,
  task_id } }` envelope the server forwards) and resolves each run's
  task → spec via `GET /api/v1/tasks/:id`. `ExplorerCanvas` maps active
  runs' specs to graph nodes (spec_path basename match plus
  `governed_by` edges) and pulses them with a sky-400 glow (2s sine
  cycle); the render loop runs only while a glowing node is on screen.
- **Recent-change fading ring** — a dedicated canvas-load liveness
  loader (`loadLivenessTimeline`) queries
  `GET /api/v1/repos/:id/graph/timeline?since=<now−1h>` (no `until`)
  so rings appear in the default view, not only when the collapsed
  Architecture-Insights panel loads; `computeRecentChanges` folds the
  deltas into a per-node change-time map and draw-time
  `recentChangeAlpha` fades the amber ring linearly to zero over
  exactly 60 minutes (60-second redraw nudge, no per-frame timer).
  The canvas `timeline` prop prefers the insights panel's full
  timeline once loaded and falls back to the liveness slice.
- **Gate status edges (Evaluative lens only)** — `loadGateData` fetches
  the repo's MRs (`repository_id`, `per_page` 10) and per-MR gate
  results into `gateRecords`; `computeNodeGateStatus` maps each MR's
  `spec_ref` to the nodes its spec governs (a failed gate wins). In
  the Evaluative lens, edges render green (all passed) / red (any
  failed); the `nodeGateStatus` derivation is gated on
  `lens === 'evaluative'`, so the default Structural lens never
  computes or renders gate colors. Where upstream's later
  per-spec-recency `gateStatusBySpec` data is also present, it
  overrides the record-aggregate coloring (documented precedence in
  the edge-draw code); both mechanisms compose without weakening
  either.

### Layer 2 — trace play icon (scaffolding)

- Nodes with linked trace spans (the `nodeSpanStats` index over
  `GET /merge-requests/:id/trace` data) render a ▶ play badge;
  clicking it (hit-test region == drawn region) switches to the
  Evaluative lens and starts the span-tree particle playback. Full
  particle/scrubbing polish remains a follow-on task per the plan.

### Requirements-changed repair (this round)

The retained source checkpoint (65c56896) was merged with current main
(078ec12a) at f7b1267e9; the merged tree is byte-identical to main
(tree `5f6ae702…`), i.e. the published implementation already contains
every content line of the checkpoint, and all five frontmatter commits
are ancestors of HEAD. The current contract was revalidated at the
merged HEAD with focused probes (below) — no surface code needed
re-implementation.

### Test evidence

- `web/src/__tests__/liveness.test.js` (28) — tracker lifecycle,
  spec-ref → node mapping, recent-change window/fade math, gate
  precedence, edge status composition.
- `web/src/__tests__/ExplorerCanvasLiveness.test.js` (14) — the real
  component: glow on governed nodes only, ring fade never snaps, gate
  edge colors only in the Evaluative lens, ▶ badge and click-to-play.
- `web/src/__tests__/ExplorerViewLivenessWiring.test.js` (4) — HTTP
  wiring: `repository_id` (never `repo_id`) with bounded `per_page`,
  canvas-load timeline query with a one-hour `since` and no `until`,
  end-to-end ring rendering with the insights panel collapsed.
- Revalidation at merged HEAD f7b1267e9 (evidence:
  `/tmp/stage/review-evidence/task-086-revalidation.txt`): the three
  liveness suites 46/46 passed; merged-surface suites
  (ExplorerCanvas, ExplorerCanvas-performance, lens-rendering, api)
  224/224 passed; `cargo test -p gyre-server --lib
  api::merge_requests` 33/33 passed (includes
  `list_mrs_per_page_keeps_newest`). Prior round's full suite: 74
  files, 1783 passed / 41 skipped; full gates are owned by
  verification, and the identical-to-main tree rules out merge
  regression by construction.
