---
task: task-086
title: "HSI Explorer Liveness Indicators (Two Layers)"
candidate: ea493fba5142da1c2a616bdf0c254c23a2b70842
base: 124a852bedb4e5759bf143b6c2683df6eeb03849
reviewed_at: 2026-11-08
verdict: approved
---

# Independent Review — task-086 HSI Explorer Liveness Indicators

Reviewed the exact candidate `ea493fba5` against base `124a852be`
(full diff: `web/src/lib/liveness.svelte.js` (new, 307 lines),
`ExplorerCanvas.svelte`, `ExplorerView.svelte`, `App.svelte`, three new web
test files, `crates/gyre-server/src/api/merge_requests.rs` (`per_page`),
`specs/tasks/task-086.md` docs only).

## Contract verification (read from production server/frontend source)

- **WS envelope (agent glow):** `handle_record_activity` (mcp.rs) builds
  `{kind: "run_started"/"run_finished", payload: {agent_id, task_id}}`
  (MessageKind::RunStarted serializes to the snake_case wire string,
  message.rs:223/:715), sends it to `message_broadcast_tx`; ws.rs delivers
  the full envelope to workspace subscribers with no tier filter; ws.js
  passes the parsed raw object to `onMessage` listeners. The tracker's
  `msg?.kind`/`msg?.payload.agent_id` access matches the real envelope.
- **Task → spec:** `api.task(id)` → TaskResponse.spec_path; domain contract
  (task.rs) is the SYSTEM spec path (e.g. "specs/system/agent-gates.md"),
  which is what `nodesForSpecRef` matches against node spec_path/spec_paths
  basenames and `governed_by` edges.
- **Timeline:** DeltaResponse.timestamp is epoch SECONDS (graph_extraction.rs
  `as_secs()`); frontend multiplies by 1000 and queries
  `?since=<epoch−1h>` (no `until`) on canvas load — exactly the plan's bound.
- **Gates:** `/merge-requests/:id/gates` returns status strings
  pending/running/passed/failed — matching the terminal-state filter.
  MR list queries use `repository_id` (the key `ListMrsQuery` actually
  recognizes) with server-side `per_page` (sort by created_at desc, truncate
  before enrichment fan-out).
- **Task file:** diff vs base is additive (progress, commits, Shipped notes);
  assigned requirements unchanged.

## Probes run (evidence: /tmp/stage/review-evidence/task-086-*.txt)

- Focused new tests: 46/46 pass (liveness, ExplorerCanvasLiveness,
  ExplorerViewLivenessWiring).
- Full `npm test` in web/ (after `npm ci`): 74 files, 1783 passed /
  41 skipped, exit 0 — the acceptance criterion.
- Rust: `cargo test -p gyre-server --lib api::merge_requests` — 32/32 pass,
  including the new `list_mrs_per_page_keeps_newest`.
- Mutation checks (each reverted; tree restored clean at the candidate SHA):
  - gate colors leaked into Structural lens (both redundant lens gates
    removed) → structural-lens test FAILS ✓
  - constant fade (no fade over the hour) → 3 tests FAIL ✓
  - 1-hour window bound removed → test FAILS ✓
  - canvas-load `loadLivenessTimeline` call removed → 2 wiring tests FAIL ✓
  - `repository_id` → `repo_id` regression → 2 wiring tests FAIL ✓
  - `run_finished` ignored by tracker → 2 tracker tests FAIL ✓
- Mechanical invariant scripts (arch, task-commit attribution, ABAC route
  registry, dead-message-kinds, mem-port-contracts, and 15 others): all pass.

## Acceptance criteria assessment

All nine criteria are met with real production code: pulsing glow driven by
real WS telemetry (start on run_started, stop on run_finished, verified by
mutation); fading amber ring from real ArchitecturalDelta records with
linear 60-minute fade (draw-time alpha, no instant off); green/red gate
edges only in the Evaluative lens (double-gated: derivation AND draw-time,
mutation-verified hidden in default Structural lens); ▶ play badge on
span-carrying nodes with working click-through to playback (Layer 2
scaffolding as assigned); performance bounded (glow rAF loop only while a
glowing node is on screen, 60s redraw interval only while unfaded rings
exist, gate computation only in evaluative lens); `npm test` passes.

## Non-blocking observations (not defects)

1. The gate-color lens gating is doubly redundant (derivation + draw path);
   removing either alone changes nothing — defensive, not a bug.
2. If the Architecture-Insights panel's full-timeline fetch fails after the
   canvas-load liveness slice succeeded, `graphTimeline = []` (catch
   fallback) shadows `livenessTimeline` via `??` and rings vanish until the
   next repo switch. Both loaders are documented best-effort; edge case in
   a failure path only.
3. Reconciliation-created tasks carry a meta-spec name (no `.md`) as
   spec_path, so those runs glow no nodes — consistent with the documented
   best-effort spec resolution (`specRefFilename` returns null).
