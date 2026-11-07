# Review — task-086 (HSI Explorer Liveness Indicators, Two Layers)

Spec: `specs/system/human-system-interface.md` §3 "Design Principle: The Built Software Is Always Alive" (lines 353-366).
Candidate under review: `a2e838e4e12ed2a525de3e2f208e86e93a7cd8aa` (branch `pipeline/task-086/deb72b1cdf194967b2651ab7d421cc44-1`), diff from assigned base `66a5fc6791eeb259c247b9a96f2ea93668c999b1`. Product commits: `c17b6c95` (features), `a3ad5d02` (repo-scoping repair); remaining commits are docs/process only.
Verdict: **needs-revision** (1 blocking finding).

## Round 1

### Evidence (all under `/tmp/stage/review-evidence/`)

- `npm ci` in `web/` → exit 0 (locked deps).
- `npx vitest run` on the three new test files (`liveness.test.js`, `ExplorerCanvasLiveness.test.js`, `ExplorerViewLivenessWiring.test.js`) → 44/44 passed (`fe-liveness.txt`).
- Full `npm test` in `web/` → 67 files, 1670 passed / 41 skipped, exit 0 (`task-086-web-full-suite.txt`).
- `/tmp/stage/bin/cargo test -p gyre-server api::merge_requests` → 31/31 passed, including the new `list_mrs_per_page_keeps_newest` (`rust-mr.txt`; cold compile, first attempt hit the 1200s tool timeout mid-build, rerun to completion).
- Mutation 1 — reverted `repository_id` → `repo_id` in `loadGateData`: both wiring tests fail (`mut1-repo-id.txt`). Restored.
- Mutation 2 — removed BOTH evaluative-lens guards on gate colors (derivation + edge-draw): the structural-lens test fails, proving the "no gate colors in Structural lens" pin is real (`mut2-structural-gate.txt`). Removing either guard alone changes nothing (double-gated). Restored.
- Probe — ExplorerView mounted with repo scope: after `repoGraph` resolves, `api.repoGraphTimeline` is **never called** (`probe-timeline-onload.txt`, temporary test deleted after the run).

### Verified working (no findings)

- **Active agent glow (Layer 1)** — end-to-end envelope verified against the server: MCP `record_activity` maps `RUN_STARTED`/`RUN_FINISHED` to `MessageKind::RunStarted/RunFinished` with payload `{agent_id, task_id}` (`mcp.rs` handle_record_activity), publishes to `message_broadcast_tx`, and `ws.rs` delivers the serialized `Message` (kind serializes as `"run_started"` per `MessageKind::as_str`) to connections subscribed to the destination workspace. App.svelte creates the tracker on the shared WS store and subscribes the store to the current workspace, so the messages actually arrive. Task → `spec_path` resolution via `GET /tasks/:id` matches the server `TaskResponse.spec_path`. `governed_by` direction (src = code node, tgt = spec node) matches the canvas's pre-existing convention. Canvas tests exercise the real component: glow stroke on run-active, removal after run-finished, legend gating, render loop bounded to on-screen glowing nodes (no idle rAF burn).
- **Recent-change fade math (Layer 1)** — delta timestamps are epoch seconds (`graph_extraction.rs:213` `as_secs()`), correctly ×1000 client-side; 60-minute linear fade computed at draw time (never instant off); count-only/unparseable deltas skipped. The pure logic is correct — the blocking finding is that its data is never loaded on canvas load (below).
- **Gate status edges (Layer 1)** — green only when both endpoints all-passed, red when either failed; pending/running prove nothing and are ignored; failed wins over passed. Double-gated on `lens === 'evaluative'` (derivation AND edge draw), so the default Structural lens never computes or renders gate colors. Legend entries only in the Evaluative branch.
- **Repo scoping repair (`a3ad5d02`)** — both liveness MR loaders now send `repository_id` (the only key `ListMrsQuery` recognizes); server-side `per_page` sorts by `created_at` descending and truncates before per-MR enrichment, bounding the gates fan-out. New server test pins newest-first capping with distinct pinned timestamps. Mutation-verified: reverting to `repo_id` fails both wiring tests.
- **Layer 2 scaffolding** — ▶ play badge on nodes present in `nodeSpanStats` (real trace spans from `GET /merge-requests/:id/trace`, pre-existing); click hit-test shares `playIconRect` with the drawer and starts the existing span-tree particle playback (Evaluative lens + playing state), matching the "scaffolding and data wiring" scope; full particle polish remains a follow-on per the assignment.
- No new routes (ABAC registry unaffected), hexagonal boundaries untouched, working tree restored clean after all probes/mutations (a `web/dist` rebuild side effect of `cargo test`'s build.rs was reverted).

### Findings

- [ ] **F1 (major): the recent-change fading ring has no data source on canvas load.** `ExplorerCanvas` receives `timeline={graphTimeline ?? []}`, but `graphTimeline` is only fetched by the lazy Architecture-Insights effect (`ExplorerView.svelte:1168-1190`), which returns early while `insightsCollapsed` is true — and the panel defaults to collapsed (`ExplorerView.svelte:633`). Probe: with ExplorerView mounted on a repo scope and the graph loaded, `api.repoGraphTimeline` is never invoked, so the canvas gets `timeline=[]` and **no ring is drawn in the default view**. This contradicts the task's implementation plan item 2 ("On canvas load, query recent `ArchitecturalDelta` records for the current repo/workspace ... Use `GET /api/v1/repos/:id/graph/timeline?since=<epoch-1h>` or similar endpoint") and leaves the acceptance criterion "Recently modified nodes (last hour) show a fading highlight ring" unsatisfied unless the user first expands an unrelated, collapsed-by-default panel. The server endpoint supports `since`/`until` and the frontend wrapper `api.repoGraphTimeline(id, since, until)` accepts them — the canvas-load query was simply never wired. The shipped notes' "already loaded from `GET /repos/:id/graph/timeline`" is accurate only for the expanded-panel state and does not disclose the gating. Remedy: on canvas load (in/alongside `loadGraph`), fetch `api.repoGraphTimeline(repoId, Math.floor((Date.now() - RECENT_CHANGE_WINDOW_MS) / 1000))` and feed the canvas timeline prop (merging with the insights-panel data if both are live).

Minor (non-blocking, recorded for completeness):

- `nodesForSpecRef` matches by basename only, so `specs/system/auth.md` and `specs/other/auth.md` are conflated (glow/gate status could land on same-named specs from different areas). Pre-existing display logic (`specBorderColor`) normalizes full paths.
- `activeRuns` is keyed by agent id only; a second concurrent run started by the same agent overwrites the first's entry (and its resolved spec) until `run_finished` deletes both. The message payload carries `task_id`, so keying by `agent_id`+`task_id` would be more faithful; the spec does not demand multi-run handling.
