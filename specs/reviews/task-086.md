# Review — task-086 (HSI Explorer Liveness Indicators, Two Layers)

Spec: `specs/system/human-system-interface.md` §3 "Design Principle: The Built Software Is Always Alive" (two liveness layers on the Explorer canvas).
Candidate under review: `6c8fcf2a` (base `66a5fc6`; product surface `c17b6c95`, review repairs `a3ad5d02` repo scoping and `7da87564` canvas-load timeline query — all three recorded in the task's `commits:` frontmatter and ancestors of the candidate).
Verdict: **approved** (third round; both prior findings repaired and independently mutation-verified).

## Evidence

All commands, outputs, and mutation results retained at `/tmp/stage/review-evidence/task-086-review-round3.txt`. Summary:

- Full suite: `npm test` in `web/` (after `npm ci`) — 67 files, 1672 passed / 41 skipped, exit 0.
- Focused: the three new suites — `liveness.test.js` (28), `ExplorerCanvasLiveness.test.js` (14), `ExplorerViewLivenessWiring.test.js` (4) — 46/46 pass.
- Server: `cargo test -p gyre-server --lib api::merge_requests` — 31/31 pass, including the new `list_mrs_per_page_keeps_newest` (per_page caps to most recently created MRs, newest first, with pinned distinct timestamps).

## Independent verification of the production wiring

- **Active agent glow (Layer 1):** verified the full envelope chain against server code — `handle_record_activity` (mcp.rs) maps `RUN_STARTED`/`RUN_FINISHED` to `MessageKind::RunStarted/RunFinished` with payload `{agent_id, task_id}`; `MessageKind` serializes as `"run_started"`/`"run_finished"`; ws.rs delivers the serialized `Message` to workspace subscribers; `ws.js` forwards parsed messages to `onMessage` listeners; the tracker in `liveness.svelte.js` matches exactly. Task→spec resolution uses the real `GET /tasks/:id` response (`spec_path`, tasks.rs:79/105), guarded against a run finishing mid-fetch. Node mapping covers `spec_path`/`spec_paths` basename matches plus `governed_by` edges to a matching spec node.
- **Recent-change fading ring (Layer 1):** verified against the real `ArchitecturalDelta` producer — `graph_extraction.rs` stamps `timestamp` in epoch **seconds** (frontend multiplies by 1000), `nodes_added`/`nodes_modified` shapes match, and the count-only no-agent-context `nodes_modified` variant is correctly skipped. The canvas-load liveness query (`api.repoGraphTimeline(repoId, since)` with `since = now−1h` in epoch seconds, no `until`) is wired in `loadGraph`, independent of the collapsed-by-default Architecture-Insights panel; the insights panel's full timeline supersedes it once loaded; stale responses are dropped on repo switch. Fade is linear full→zero over exactly 60 minutes, computed at draw time with a 60-second redraw nudge (no per-frame timer).
- **Gate status edges (Layer 1, Evaluative lens only):** MR list queries now send `repository_id` (the parameter `ListMrsQuery` actually recognizes) with a server-side `per_page` cap implemented in this diff (sort by `created_at` desc, truncate before per-MR enrichment — bounded fan-out). Lens hiding is enforced redundantly at both the `nodeGateStatus` derivation and the edge-draw site.
- **Layer 2 scaffolding:** ▶ play badge driven by `nodeSpanStats` over real `GET /merge-requests/:id/trace` span data (`graph_node_id` linkage); click hit-test shares `playIconRect` with the drawn region; clicking switches to the Evaluative lens and starts the existing span-tree particle playback. Particles/scrubbing polish is a follow-on task per the assignment.
- **Performance criterion:** the glow render loop stays alive only while a glowing node is actually on-screen (bounded by `layoutNodeMap`); the ring fade needs no per-frame work; gate data is static per load; the per-MR gates fan-out is capped at `per_page=10`.

## Mutation checks (each edit restored; tree verified clean at the candidate afterwards)

1. `repository_id` → `repo_id` in `loadGateData`: 2 wiring tests fail (repo scoping enforced).
2. Remove the `loadLivenessTimeline` call: both canvas-load timeline tests fail.
3. Remove **both** lens gates on gate edge colors: the "structural lens shows NO gate colors" test fails (removing either gate alone passes — the two are redundant defenses, each independently sufficient).
4. Disable the glow draw: both glow tests fail.
5. Instant ring fade (`const fade = 1`): both fading-ring tests fail.

The tests are not self-confirming: every acceptance criterion has at least one test that fails when the corresponding production behavior is disabled. Both prior durable findings (tenant-wide MR fall-through on the unrecognized `repo_id` key; no data source for the fading ring in the default view) are repaired with mutation-verified enforcement.
