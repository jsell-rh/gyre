# Review — task-086 (HSI Explorer Liveness Indicators, Two Layers)

Spec: `specs/system/human-system-interface.md` §3 "Design Principle: The Built Software Is Always Alive" — Layer 1 (active-agent glow from `RunStarted`/`RunFinished` telemetry, recent-change fading ring from `ArchitecturalDelta` records, gate pass/fail edge colors in the Evaluative lens) + Layer 2 scaffolding (▶ play icon on nodes with trace data).

Candidate under review: `fdb6589aab2a6cac51f041bb901182c4cde0239a` (base `7f8b5d9cbdff735f15f513f007fbb12c4d4fe545`; product surface commit `c17b6c95`, frontend-only). Diff: `web/src/lib/liveness.svelte.js` (+307), `ExplorerCanvas.svelte` (+192), `ExplorerView.svelte` (+33), `App.svelte` (+13), two test files (+616), task doc.

Verdict: **needs-revision** — one real wiring defect in the Layer 1 gate-status data path.

## What was independently verified as working

- **Server-surface contracts all check out.** The tracker's envelope assumption matches the actual wire: `record_activity` MCP tool maps `RUN_STARTED`/`RUN_FINISHED` to `MessageKind::RunStarted`/`RunFinished` with payload `{agent_id, task_id?}` (`mcp.rs` ~1256, payload schema in `message.rs` `RunStarted`/`RunFinished` arms), the WS handler forwards the serialized `Message` envelope (`kind`/`payload` top-level) to workspace subscribers (`ws.rs` bus branch). `GET /tasks/:id` responses carry `spec_path` (`api/tasks.rs:32,79,105`). Delta `timestamp` is epoch seconds (`graph_extraction.rs:213-216`), multiplied by 1000 client-side — correct. `GET /merge-requests/:id/gates` returns a bare array of `{status: 'pending'|'running'|'passed'|'failed'}` (`gates.rs::gate_status_str`) — matches the frontend's terminal-state filter.
- **Glow lifecycle is real.** `createLivenessTracker` records on `run_started`, deletes on `run_finished`, guards a mid-fetch finish; `App.svelte` wires it to the shared WS store with `api.task` spec resolution. Canvas draws a sine-modulated sky-400 glow only for nodes governed by the active run's spec (spec_path/spec_paths basename + `governed_by` edges); render loop stays alive only while a glowing node is on screen (`glowAnimating` bounded by `layoutNodeMap`).
- **Fading ring math is real.** `computeRecentChanges` folds `nodes_added`/`nodes_modified` qualified names within the last hour (count-only and unparseable deltas skipped — matching the no-agent-context `nodes_modified: <count>` shape in `graph_extraction.rs`); `recentChangeAlpha` fades linearly 1→0 over exactly 60 min, computed at draw time; a 60 s interval nudges redraw while unfaded rings exist.
- **Lens gating is real.** `nodeGateStatus` is derived only when `lens === 'evaluative'`; edge coloring is additionally guarded by `lens === 'evaluative' && hasGateEdges`. Structural default never computes or renders gate colors.
- **Layer 2 scaffolding is real.** ▶ badge drawn on nodes present in the pre-existing `nodeSpanStats` span index; click hit-test shares `playIconRect` with the drawer (draw and hit regions coincide — verified `sln.w * cam.zoom` matches the draw path's `sw`); `startTracePlayback` flips the `$bindable` lens (parent binds `bind:lens={explorerLens}`) and starts the existing span-tree particle playback.
- **Tests are hard, not self-confirming.** Both mutation probes killed: disabling the gate edge-color branch fails 2 canvas tests; disabling the glow stroke fails 2 canvas tests (evidence: `/tmp/stage/review-evidence/task-086-mutation-probes.txt`). The component tests render the real component with a captured canvas context and assert actual stroke colors/alphas — no mirrored logic.
- **Suites pass on the pristine candidate tree:** focused liveness 42/42; full `npm test` in `web/` — 65 files, 1665 passed / 41 skipped, exit 0 (matches the shipped claim; locked deps via `npm ci`).
- Working tree left clean after all probes (`git status --short` empty; HEAD `fdb6589a` unchanged).

## F1 (finding): `loadGateData` MR query params are silently ignored — gate records are not scoped to the current repo and the fetch is unbounded

`web/src/components/ExplorerView.svelte`, `loadGateData` (added by this diff):

```js
const mrs = await api.mergeRequests({ repo_id: repoId, per_page: 10 });
```

The server's `ListMrsQuery` (`crates/gyre-server/src/api/merge_requests.rs:38-42`) recognizes only `status` / `repository_id` / `workspace_id`. `repo_id` and `per_page` are unknown keys — serde_urlencoded ignores them (no `deny_unknown_fields`; `per_page` exists nowhere in this handler, only in `api/scim.rs`), so `list_mrs` falls through to `state.merge_requests.list()`, which is tenant-scoped but **not repo-scoped** (`sqlite/merge_request.rs:218-227`). The server's own test uses `?repository_id=repo-1` (`merge_requests.rs:1295`), and the correct pattern is used elsewhere in the same frontend (`DetailPanel.svelte:890`, `RepoSettings.svelte:191`, `RepoMode.svelte:210`, `ExplorerCodeTab.svelte:165`).

Consequences:

1. **Cross-repo gate contamination of the evaluative signal.** Every spec_ref-carrying MR in the *tenant* contributes gate records, and `computeNodeGateStatus` maps them onto the *current repo's* nodes via spec-**filename** (basename) matching. Two repos in a tenant that each have a spec named e.g. `auth.md` cross-contaminate: another repo's failed gate colors this repo's edges red. The implementation plan requires "Data sourced from gate evaluation records for the current scope", and the candidate's own docstring claims "MR gate evaluation records for the current repo" — neither holds in a multi-repo tenant.
2. **Unbounded sequential request fan-out.** The intended `per_page: 10` cap is absent, so every canvas load issues one awaited `GET /merge-requests/:id/gates` per spec_ref-carrying MR in the tenant, sequentially in a for-loop. This bears on the "Canvas performance is not degraded by liveness indicators" acceptance criterion: while best-effort/async, in a busy tenant this is an N+1 request storm per canvas load.

Reproduce (static): compare `ExplorerView.svelte` `loadGateData` with `ListMrsQuery` fields. Evidence chain with line references: `/tmp/stage/review-evidence/task-086-gate-scope-defect.txt`.

Note: the same wrong-param pattern pre-exists in `loadTraceData` (base `7f8b5d9cbdff`, `ExplorerView.svelte:922`) — the candidate replicated the pre-existing mistake in new code rather than following the codebase's correct `repository_id` pattern. The pre-existing instance is out of scope for this verdict; the new one is not, because gate-status scoping is exactly what this task's plan item 3 specifies.

Why the test suite missed it: the tests cover the pure helpers (`liveness.test.js`) and the canvas render path (`ExplorerCanvasLiveness.test.js`) — both correct — but nothing exercises `loadGateData`'s actual HTTP wiring, which is where the defect lives.

Fix is one word plus a dropped param: `api.mergeRequests({ repository_id: repoId })` (and drop `per_page` or add server-side support for it).

## Probe evidence

All under `/tmp/stage/review-evidence/`: `task-086-focused-tests.txt` (42/42), `task-086-full-suite.txt` (1665 passed/41 skipped, exit 0), `task-086-mutation-probes.txt` (both mutations killed, source restored, post-restore re-run 28/28), `task-086-gate-scope-defect.txt` (F1 evidence chain).

— Reviewer, 2026-10-08
