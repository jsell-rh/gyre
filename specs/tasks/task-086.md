---
title: "HSI Explorer Liveness Indicators (Two Layers)"
spec_ref: "human-system-interface.md §3 Design Principle"
depends_on:
  - task-065
progress: complete
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

Both liveness layers of HSI §3 are implemented in production frontend code
(product surface in commit c17b6c95; review repairs in commits a3ad5d02 —
repo scoping — and 7da87564 — the canvas-load timeline query below; upstream
rebase resolution in merge commit 2bd17bd3 — both liveness and upstream's
causal-trace/focus features coexist in `ExplorerView`/`ExplorerCanvas`;
verification repair in commit c4d00719 — a rustfmt-only fix to the
`per_page` test's pinned-timestamp loop, described under Test evidence).

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
  `ArchitecturalDelta` records into a per-node map of change times for
  deltas within the last hour (delta timestamps are epoch seconds;
  `nodes_added`/`nodes_modified` entries matched by qualified_name;
  count-only and unparseable deltas skipped). Draw-time `recentChangeAlpha`
  fades the amber ring linearly from full opacity to zero over exactly 60
  minutes — never an instant off; a 60-second interval nudges a redraw
  while unfaded rings exist (no per-frame timer).
- **Gate status edges (Evaluative lens only)** — `ExplorerView` loads the
  repo's recent MRs and their gate results
  (`GET /merge-requests?repository_id=<repo>&per_page=10`, then
  `GET /merge-requests/:id/gates` per spec_ref-carrying MR) best-effort
  into `gateRecords` (`{ mrId, spec_ref, gates }`), passed to the canvas.
  `computeNodeGateStatus` maps each MR's `spec_ref` to the nodes its spec
  governs — a failed gate wins over passing ones; pending/running-only sets
  prove nothing and are ignored. In the Evaluative lens, edge drawing
  colors an edge green when both endpoints have all gates passed and red
  when either endpoint has a failed gate; the `nodeGateStatus` derivation
  is gated on `lens === 'evaluative'`, so the default Structural lens never
  computes or renders gate colors. The legend gains green/red gate entries
  only while gate data is live.
- **Repo scoping of liveness MR queries (review repair, a3ad5d02)** — the
  loaders originally sent `{ repo_id, per_page }`, but the server's
  `ListMrsQuery` only recognizes `repository_id` (unknown keys are
  silently dropped), so the MR list fell through to the tenant-wide set:
  another repo's failed gate on a same-named spec could color this repo's
  edges red, and the per-MR gates fan-out was unbounded. Both
  `loadGateData` and `loadTraceData` now send `repository_id`, and
  `per_page` is implemented server-side on `GET /merge-requests`: the
  handler sorts by `created_at` descending (deterministic across adapters,
  whose list ordering is not contractual) and truncates before the per-MR
  enrichment, so dropped MRs cost no extra queries. As a side effect the
  trace loader now genuinely sees the most recent MRs first.
- **Canvas-load timeline query (review repair, 7da87564)** — the fading
  ring was originally driven only by the `timeline` prop fed from
  `graphTimeline`, which loads inside the lazy Architecture-Insights
  effect — a panel collapsed by default — so in the default view the
  canvas received `timeline=[]` and no ring was ever drawn. ExplorerView
  now runs a dedicated liveness loader on canvas load:
  `api.repoGraphTimeline(repoId, since)` with
  `since = Math.floor((Date.now() − RECENT_CHANGE_WINDOW_MS)/1000)`,
  exactly the `?since=<epoch-1h>` bound the implementation plan specifies,
  keeping the payload small (no `until` — the window runs to the present).
  The canvas `timeline` prop prefers the insights panel's full timeline
  (a superset) once that panel loads and falls back to the liveness slice
  otherwise; stale responses are dropped when the user switches repos
  mid-fetch. Non-blocking and best-effort, like the trace/gate loaders.

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
- `web/src/__tests__/ExplorerViewLivenessWiring.test.js` (4 tests) — the
  liveness loaders' HTTP wiring: every MR list call carries
  `repository_id` for the current repo (never `repo_id`) with a bounded
  `per_page`, and gates are fetched for the current repo's spec_ref-carrying
  MRs against a contract-faithful mock (returns MRs only when
  `repository_id` matches); the canvas-load timeline query fires exactly
  once with a `since` bound of one hour (epoch seconds) and no `until`;
  and end-to-end through the real ExplorerView → ExplorerCanvas mount
  with the insights panel collapsed (the default), the loaded delta draws
  the amber fading ring. Mutation-checked: reverting the MR-query fix to
  `repo_id`, removing the canvas-load `loadLivenessTimeline` call, or
  reverting the canvas `timeline` prop wiring each fails the corresponding
  tests. (The suite's rAF stub is queue-based — a synchronous stub leaves
  a stale `animFrame` id that suppresses data-driven redraws — and the
  beforeEach re-establishes default api mock implementations so per-test
  overrides cannot leak.)
- Server: `api::merge_requests::tests::list_mrs_per_page_keeps_newest` —
  `per_page` caps the list to the most recently created MRs, newest
  first, with distinct pinned timestamps; full module
  `api::merge_requests::` 32/32 passed (`cargo test -p gyre-server --lib
  api::merge_requests`, exit 0, after the rustfmt repair below).
- Full suite: `npm test` in `web/` — after the upstream rebase merge
  (2bd17bd3, resolving additive conflicts in ExplorerView/ExplorerCanvas so
  the liveness imports/props and upstream's `focusRequest`/`windowEscape`/
  causal-trace wiring all survive together): 74 files, 1783 passed /
  41 skipped, exit 0 (locked deps via `npm ci`). The four conflict regions
  were all additive (both sides' features kept); no liveness behavior was
  weakened by the merge.
- Verification repair (c4d00719) — the verification gate's rustfmt check
  failed on changed line 1407 of `crates/gyre-server/src/api/merge_requests.rs`
  (the pinned-timestamp array literal in the `per_page` test exceeded the
  line-width budget). `cargo fmt -p gyre-server` reformats that one loop;
  no behavior change, and `cargo fmt --check` is clean afterwards. All
  other gate failures in that run: none — every other check in the log
  passed and the web build itself succeeded (the Svelte a11y/unused-CSS
  lines in the log tail are pre-existing non-fatal warnings).
- Evidence (commands, SHAs, exit codes, output) saved under
  `/tmp/stage/review-evidence/task-086-*.txt` for executor retention.
