# Review — task-086 (HSI §3 Explorer Liveness Indicators, Two Layers)

Spec: `specs/system/human-system-interface.md` §3 "Design Principle: The
Built Software Is Always Alive" (lines 353-367). Candidate: `65c568960`
(assigned base `637be7187`; product surface `c17b6c95`, review repairs
`a3ad5d02` repo scoping and `7da87564` canvas-load timeline query,
verification repair `c4d00719` rustfmt-only). Verdict: **approved**.

## Round 1

### Diff scope

`git diff 637be7187..65c568960` touches 9 files, all task-owned:
`web/src/lib/liveness.svelte.js` (new, 307 lines), `ExplorerCanvas.svelte`
(+192), `ExplorerView.svelte` (+67), `App.svelte` (+13), three new test
files (+857), `crates/gyre-server/src/api/merge_requests.rs` (+59:
`per_page` on the MR list), and `specs/tasks/task-086.md`. No unrelated
production code, scripts, or specs touched. `check-task-commit-attribution.sh`
passes (all task-labeled product-surface commits recorded).

### Wire-contract verification (code reading, server side)

- **Active-agent glow data source is real.** `gyre_record_activity` MCP tool
  (`mcp.rs` handle_record_activity) builds `MessageKind::RunStarted/RunFinished`
  with payload `{agent_id, task_id}` and sends it on `message_broadcast_tx`;
  `ws.rs` forwards the full serialized `Message` envelope (`kind` snake_case
  per `MessageKind::as_str`, `payload`) to connections subscribed to the
  target workspace. The frontend tracker (`createLivenessTracker`, wired in
  `App.svelte` to the shared `createWsStore`) listens for exactly
  `kind === 'run_started' | 'run_finished'` with `payload.agent_id` —
  shape-verified against `mcp.rs:1297-1303` and the `Message` serde layout.
  Task → spec resolution uses the real `GET /api/v1/tasks/:id`
  (`TaskResponse.spec_path` exists, `api/tasks.rs:79`), with a mid-fetch
  finish guard.
- **Recent-change ring data source is real.** `GET /repos/:id/graph/timeline`
  returns `DeltaResponse { timestamp: u64 (epoch secs), delta_json }`
  (`graph.rs:178-186`); `ExplorerView.loadLivenessTimeline` queries it on
  canvas load with `since = now − 1h` exactly as the implementation plan
  specifies (repair `7da87564` — the insights-panel-only source was indeed
  dead in the default view). `computeRecentChanges` re-applies the window,
  matches `nodes_added`/`nodes_modified` qualified names to graph nodes, and
  `recentChangeAlpha` fades linearly full→zero over exactly 60 minutes,
  computed at draw time.
- **Gate edges data source is real and correctly scoped.** `GET
  /merge-requests/:id/gates` returns `GateResultResponse` with status strings
  `passed/failed/pending/running` (`gates.rs:130-133`) — matching
  `computeNodeGateStatus`'s terminal-state filter. The MR list is scoped via
  `repository_id` (repair `a3ad5d02` — `ListMrsQuery` indeed has no `repo_id`
  key) and bounded by the new server-side `per_page` (sort by `created_at`
  desc, truncate before per-MR enrichment; tested by
  `list_mrs_per_page_keeps_newest`). Gate colors are double-gated on the
  Evaluative lens (derivation and draw).
- **Layer 2 scaffolding** — play badge on nodes present in `nodeSpanStats`
  (spans from the real trace endpoint), hit-test shares `playIconRect` with
  the drawer, click switches to Evaluative lens and starts the existing
  span-tree particle playback. Matches the task's scaffolding scope.
- **Performance** — the render loop stays alive only while a glowing node is
  on-screen (`layoutNodeMap` bound); the fade needs only a 60 s redraw nudge;
  gate status is computed only in the Evaluative lens. No per-frame timers.

### Probes (evidence under /tmp/stage/review-evidence, ev1-ev9)

- `npx vitest run` on the three liveness test files: **46/46 passed, exit 0**
  (ev1).
- `cargo test -p gyre-server --lib api::merge_requests`: **32/32 passed,
  exit 0**, including `list_mrs_per_page_keeps_newest` (ev2).
- Full `npm test` in `web/` (after `npm ci`): **76 files, 1805 passed /
  41 skipped, exit 0** (ev7) — acceptance criterion met.
- Mutation checks (each reverted; tree verified clean afterwards):
  - **A** — remove the Evaluative-lens gate from edge coloring (both the
    derivation gate and the draw gate): `structural lens (default) shows NO
    gate colors` fails (ev3). The lens-hiding acceptance criterion has a
    killing test.
  - **B** — disable the fade (`recentChangeAlpha` returns constant 1):
    3 tests fail including "fades linearly from 1 to 0 over the hour —
    never an instant off" (ev4).
  - **C** — ignore `run_finished` (glow never removed): 2 tests fail
    (ev5).
  - **D** — revert MR-query scoping to `repo_id`: 2 wiring tests fail,
    including the contract-faithful mock that only returns MRs when
    `repository_id` matches (ev6).
- `check-arch.sh` OK, `check-abac-route-registry.sh` OK,
  `cargo fmt --check -p gyre-server` clean, attribution check OK.

### Notes (non-blocking)

- The implementation plan says "pulsing CSS animation"; the glow is drawn on
  the `<canvas>` (sine-modulated stroke + shadow blur), which is the only
  way to animate canvas-rendered nodes — the specced behavior (subtle pulse,
  start/stop on RunStarted/RunFinished) is delivered; not a divergence worth
  a spec amendment.
- `npm ci` triggers the web postinstall dist rebuild; the resulting
  `web/dist` churn was restored via `git checkout` — the working tree is
  clean at the candidate SHA.
- Coverage matrix row 18 still reads `task-assigned`; that ledger is the
  spec-fidelity auditor's surface, not this candidate's code.

**Approved.**
