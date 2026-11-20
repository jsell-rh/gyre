# Review — task-206 (meta-spec preview mode, §5)

Candidate: `47615b1d30100a9637336e7bbf420cbb657a5a55` (base `7a80e459`)
Verdict: **not approved** — one blocking functional defect, one
defense-in-depth gap. Evidence: `/tmp/stage/review-evidence/task-206-review-round3.md`.

## What was verified as real and correct

The §5 machinery is a genuine agent-run path, not a facade. Independently
confirmed on the candidate tree:

- Routes: global `POST/GET/DELETE /api/v1/meta-specs/preview[/:id]` plus
  resolver-enforced workspace-scoped twins (mod.rs:941-967), all mapped in the
  ABAC resolver; the two base exemption entries were retired (removal-only
  diff to the frozen exemption file). Hollow blast-radius preview is gone.
- Spawn: real Active agent rows (workspace-scoped from the repo record),
  real `preview/{preview_id}/{slug}` branches pinned to the default-branch
  tip, real worktrees, short-lived JWTs (separate TTL), draft injected into
  the actual process env (`GYRE_META_SPEC_DRAFT_KIND/CONTENT`,
  `GYRE_TARGET_SPEC_PATH`, `GYRE_PREVIEW_ID`) and into the LLM prompt
  (agent-runner.mjs previewPrompt); k8s pod manifest carries the draft.
  Rollback on provision/persist failure is complete and tested.
- Skip-ceremony: no task/gates/MR/merge-queue/provenance/refs writes from
  the preview path, and an unusually thorough set of refusal guards
  (REST + MCP, caller- and target-keyed) with fail-closed kv handling.
- Completion: exit monitor routes preview agents to `finish_preview_agent`
  (never Idle), worktree/token/slot cleaned, branch retained for the diff;
  exactly-once budget-slot release across all terminal paths; probe failure
  ≠ death in the monitor; DELETE and the `meta_spec_preview_gc` TTL job
  (default 24h) tear everything down; GC TTL boundary genuinely tested.
- Tests: 31 server + 2 MCP tests, overwhelmingly hard (production router,
  real git repos, real spawned processes, real env dumps, real side-effect
  assertions). Frontend: 184 vitest tests pass incl. mid-run diff rendering
  and composite `repo_id:spec_path` tab keying.
- Probes run this round (all pass): agent-runner node tests (12/12),
  MetaSpecs+EditorSplit vitest (184/184), check-arch, check-mcp-write-tools,
  check-abac-route-registry, check-abac-exempt-handlers,
  check-inert-enforcement.

## Blocking finding — preview agents cannot stay alive past 60s (status surface and UI then lie)

`launch_taskless_agent_process` (spawn.rs:2891) never issues a session SVID
(`issue_session_svid` exists only in the task-driven launch paths, spawn.rs:563
and :2143). Every heartbeat surface is fail-closed on SVID attestation
(agents.rs:405, mcp.rs:2214 → spiffe.rs:504 "session was never attested"), so
`last_heartbeat` never advances; the runner heartbeats every 15s with
`svid: ''`. `is_alive` falls back to `spawned_at`, the stale detector runs
every 30s with a 60s timeout, and preview provisioning leaves the default
`Pause` behavior — so ~60s into every real preview run the still-running agent
is flipped to Stopped with a spurious `agent.failed` event. The status surface
then reports per-agent "stopped" and run state "complete" while work is still
in flight, and the UI stops polling on "complete" (MetaSpecs.svelte:570) —
the diff the agent eventually pushes is never fetched or displayed. This
violates "GET status reports real per-agent state and the produced diff after
completion" and §5 iteration-cycle step 4 for any run >60s (i.e. essentially
all real runs; the tests' agent scripts all exit in <60s). The task's own
Shipped note that preview self-heartbeat "keeps working" is contradicted by
the candidate's test comment at mcp.rs:11288 ("it then fails SVID
verification in the handler"). Fix: issue a session SVID in the taskless
launch path (mirroring `launch_agent_workload`) or provide an equivalent
preview liveness path, with an integration test using a >60s preview agent.

## Second finding — MCP denial list gap (defense-in-depth)

`gyre_spawn_worker`, `gyre_spawn_repo_orchestrator`, `gyre_cross_repo_task`
and `gyre_decompose_spec` (task-creating, needs_write) are absent from the
preview-agent denial match (mcp.rs:5084-5101); they are currently refused
only by the orchestrator-claim token-shape check, not the preview registry.
Not exploitable today (preview JWTs carry no orchestrator claims), but the
containment should not rest on token shape.

## Non-blocking observations (recorded, not gating)

- EditorSplit's meta-spec surface aggregates multi-target diffs into one flat
  Code Diff tab and has no clean-up wiring; the primary §5 surface
  (MetaSpecs.svelte) implements tabs and clean-up correctly.
- The MCP preview tests hand-build the `preview_agents` kv record, so the MCP
  dispatch gate has no end-to-end test with a POST-spawned preview agent.
- `PUT /tasks/{id}` accepts a preview agent id as `assigned_to` (no
  target-keyed assignee check); human-initiated only, nothing auto-executes.

## Verifier must run

`cargo test -p gyre-server --lib -- preview_`, `cargo test -p gyre-server
--lib mcp_preview_agent`, `cargo test -p gyre-server --lib monitor_iteration`,
`cargo test -p gyre-adapters --lib compute::ssh`, `cd docker/gyre-agent &&
node --test agent-runner.test.mjs`, plus `cargo test --all`,
`cargo clippy --all-targets`, `cd web && npm ci && npm test`. After the
blocking fix: an integration test with a preview agent whose command sleeps
past `HEARTBEAT_TIMEOUT_SECS`, asserting the row stays Active and GET status
stays "running".
