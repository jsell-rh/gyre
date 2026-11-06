---
title: "HSI Explorer Liveness Indicators (Two Layers)"
spec_ref: "human-system-interface.md §3 Design Principle"
depends_on:
  - task-065
progress: ready-for-review
coverage_sections:
  - "human-system-interface.md §3 Design Principle: The Built Software Is Always Alive"
commits: ["c17b6c95e66824fd3b72b21f95b9b95f2a284ec6"]
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

Both liveness layers of HSI §3 are implemented in production frontend code
(product surface in commit c17b6c95, frontend-only — no Rust changes; this
round preserved that fix unchanged, restored the assigned task contract
text, and re-verified the full chain against the real server surfaces; see
Test evidence).

### Layer 1 — SDLC activity

- **Active agent glow** — `web/src/lib/liveness.svelte.js` exports a
  reactive `activeRuns` map plus `createLivenessTracker(wsStore,
  { fetchTask })`, wired in `App.svelte` to the shared workspace WebSocket
  store. The tracker listens for Telemetry-tier bus messages
  (`kind: "run_started"` / `"run_finished"`, payload `{ agent_id, task_id }`
  — the exact envelope the server builds in the `record_activity` MCP tool
  for `MessageKind::RunStarted/RunFinished` and forwards to workspace WS
  subscribers as `{ kind, payload }`) and resolves the run's task → spec
  via `GET /api/v1/tasks/:id` (its response carries `spec_path`; best-effort,
  guarded against a run finishing mid-fetch). `ExplorerCanvas` maps each
  active run's spec to graph nodes (spec_path/spec_paths basename match
  plus `governed_by` edges to a matching spec node) and pulses those nodes
  with a sky-400 glow (2s sine cycle). The render loop stays alive only
  while a glowing node is actually on screen; the redraw is deferred to a
  macrotask so the mount flush is never drawn into synchronously.
- **Recent-change fading ring** — `computeRecentChanges` folds the repo's
  `ArchitecturalDelta` records (already loaded from
  `GET /repos/:id/graph/timeline`) into a per-node map of change times for
  deltas within the last hour (delta timestamps are epoch seconds;
  `nodes_added`/`nodes_modified` entries matched by qualified_name;
  count-only and unparseable deltas skipped). Draw-time `recentChangeAlpha`
  fades the amber ring linearly from full opacity to zero over exactly 60
  minutes — never an instant off; a 60-second interval nudges a redraw
  while unfaded rings exist (no per-frame timer).
- **Gate status edges (Evaluative lens only)** — `ExplorerView` loads the
  repo's recent MRs and their gate results
  (`GET /merge-requests/:id/gates`) best-effort into `gateRecords`
  (`{ mrId, spec_ref, gates }`), passed to the canvas.
  `computeNodeGateStatus` maps each MR's `spec_ref` to the nodes its spec
  governs — a failed gate wins over passing ones; pending/running-only sets
  prove nothing and are ignored. In the Evaluative lens, edge drawing
  colors an edge green when both endpoints have all gates passed and red
  when either endpoint has a failed gate; the `nodeGateStatus` derivation
  is gated on `lens === 'evaluative'`, so the default Structural lens never
  computes or renders gate colors. The legend gains green/red gate entries
  only while gate data is live.

### Layer 2 — trace play icon (scaffolding)

- Nodes with linked trace spans (the `nodeSpanStats` index over
  `GET /merge-requests/:id/trace` data, already loaded for the Evaluative
  lens) render a ▶ play badge at the node's bottom-right corner. Clicking
  it (hit-test shares `playIconRect` with the drawer, so the clickable
  region is exactly the drawn region) switches to the Evaluative lens and
  starts the existing span-tree particle playback from the beginning —
  the data wiring this task owes; particles/scrubbing polish remains a
  follow-on task per the assignment.

### Test evidence

- `web/src/__tests__/liveness.test.js` (28 tests) — tracker lifecycle
  (run_started records + async spec resolution, run_finished removal,
  mid-fetch finish guard), spec-ref → node mapping, recent-change window
  and fade math, gate status precedence, and edge status composition.
- `web/src/__tests__/ExplorerCanvasLiveness.test.js` (14 tests) — the
  real component: glow stroke rendered only for governed nodes while a
  run is active and removed after `run_finished`; fading ring alpha
  decreases with age and never snaps; gate edge colors appear only in the
  Evaluative lens (Structural stays structural-colored); ▶ play badge
  drawn on span-carrying nodes and its click starts playback.
- Full suite: `npm test` in `web/` — 65 files, 1665 passed / 41 skipped,
  exit 0 (this round, locked deps via `npm ci`). Focused liveness run:
  42/42 passed.
- Evidence (commands, SHAs, exit codes, output) saved under
  `/tmp/stage/review-evidence/task-086-*.txt` for executor retention.
