# Review: TASK-086 — HSI Explorer Liveness Indicators (Two Layers)

**Reviewer:** Independent review (Gyre pipeline)
**Date:** 2026-10-09
**Comparison base:** a4ebdf6f58423a0b7941faaab708630cc7936e8f
**HEAD (candidate):** c3818b435493d0ac855688676676130370549f7f
**Verdict:** approved

---

## Summary

Both HSI §3 liveness layers are implemented in production frontend code with a
small, well-scoped server addition (`per_page` on `GET /merge-requests`). The
WS/HTTP contracts the frontend depends on were independently verified against
server source; all acceptance criteria are backed by real draw-path tests that
were mutation-checked (5 probes, all detected). No fake implementations, no
scope leaks, no test inflation found.

## Behavior verification

- [x] **Active-agent glow (Layer 1).** `web/src/lib/liveness.svelte.js` exports
  a reactive `activeRuns` map and `createLivenessTracker(wsStore, { fetchTask })`,
  wired in `App.svelte` inside the WS-store `$effect` (same lifecycle as the
  presence heartbeat; destroyed on teardown). Contract verified against server
  source: `gyre_record_activity` (mcp.rs `handle_record_activity`) builds
  payload `{ agent_id, task_id }` for `RUN_STARTED`/`RUN_FINISHED`, sends it on
  `message_broadcast_tx` with `Destination::Workspace`, and ws.rs forwards the
  serialized Message to subscribed workspace connections; `MessageKind::RunStarted`
  serializes as `"run_started"`. Task → spec resolution via `GET /tasks/:id`
  (`TaskResponse.spec_path` confirmed in api/tasks.rs), best-effort with a
  mid-fetch finish guard. `ExplorerCanvas` maps spec → nodes (spec_path/
  spec_paths basename + `governed_by` edges) and pulses a sky-400 glow with a
  2s sine cycle; the render loop stays alive only while a glowing node is
  on-screen (`glowAnimating` bounded by `layoutNodeMap`), so no idle CPU burn.
- [x] **Recent-change fading ring (Layer 1).** `computeRecentChanges` folds
  `ArchitecturalDelta` records (verified: `graph_extraction.rs` writes
  `timestamp` in epoch **seconds**; `nodes_modified` is a bare count without
  agent context and an array of `{ qualified_name }` otherwise — both handled).
  `recentChangeAlpha` fades linearly to zero over exactly 60 minutes (draw-time
  computation, 60s redraw nudge — no per-frame timer). The canvas-load query
  `api.repoGraphTimeline(repoId, since)` with `since = now − 1h` (epoch s, no
  `until`) runs on graph load, so the ring does not depend on the
  collapsed-by-default Architecture-Insights panel; the insights panel's full
  timeline supersedes the slice once loaded; stale responses dropped on repo
  switch.
- [x] **Gate status edges (Layer 1, Evaluative only).** `loadGateData` sends
  `repository_id` + `per_page: 10` (the parameter the server's `ListMrsQuery`
  actually recognizes — the original `repo_id` form silently fell through to
  the tenant-wide list; fixed in review-repair commit a3ad5d02), then fetches
  gates per spec_ref-carrying MR. `GateStatus` serde values confirmed
  ("pending"/"running"/"passed"/"failed"); only terminal states count, failed
  wins. Edge coloring is gated on `lens === 'evaluative'` for both the
  derivation and the draw; Structural (default) never computes or renders gate
  colors, and the legend gains gate entries only while data is live.
- [x] **Server `per_page`.** `list_mrs` sorts by `created_at` descending and
  truncates before per-MR enrichment, keeping the cap deterministic across
  adapters (whose list ordering is not contractual).
  `list_mrs_per_page_keeps_newest` pins newest-first with distinct timestamps.
- [x] **Layer 2 scaffolding.** ▶ play badge on nodes present in `nodeSpanStats`
  (pre-existing trace index over `GET /merge-requests/:id/trace`); click
  hit-test shares `playIconRect` with the drawer and starts the existing
  span-tree particle playback (Evaluative lens + `evalPlaying`), which is the
  data wiring this task owes per the assignment.
- [x] **Performance.** Full web suite (including `ExplorerCanvas-performance`)
  passes; glow animation loop is bounded to on-screen glowing nodes; fade uses
  a 60s nudge, not a timer per frame.
- [x] **`npm test` in web/.** 74 files, 1783 passed / 41 skipped, exit 0 —
  matches the shipped claim exactly (locked deps via `npm ci`).

## Mutation evidence (all detected, all restored; tree verified clean)

1. `repository_id` → `repo_id` in `loadGateData` → 2 wiring tests fail.
2. Canvas-load `loadLivenessTimeline(repoId)` call removed → 2 wiring tests
   fail (including the end-to-end ring draw through real ExplorerView →
   ExplorerCanvas with the insights panel collapsed).
3. Gate edge-color branch disabled → 2 canvas tests fail.
4. Glow stroke disabled → 2 canvas tests fail.
5. Fade math broken (constant full opacity) → fade test fails.

Evidence (commands, outputs, exit codes): `/tmp/stage/review-evidence/`
(task-086-review-probes.txt, task-086-frontend-focused-tests.txt).

## Commit attribution

`check-task-commit-attribution.sh` passes. The three frontmatter commits are
the product-surface commits; upstream rebase resolution (merge 2bd17bd30) and
docs/process commits are correctly scoped.

## Non-blocking observations

- `nodesForSpecRef` matches specs by **basename** (`auth.md`), so two specs
  sharing a filename in different directories would cross-match. Task
  spec_refs in this system carry only the filename, and spec basenames are
  unique in `specs/`, so this is a safe heuristic today; worth tightening if
  duplicate basenames ever appear.
- `activeRuns` is keyed by agent_id; a second `run_started` from the same
  agent overwrites the first run's entry. Agents run one task at a time, so
  this is correct for the current model.
