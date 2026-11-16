# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Independent review agent
**Round:** R2 (repair-round head `c33a86e2` was reviewed as R1 `13e0dc8b`; this round reviews the final candidate)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** 6082be2ffab31f715321c08e4ccb2426b23c097f
**HEAD reviewed:** 6bfdaac97844a1772e1084d652e5f2aa8bd9fc1c
**Status:** findings — not approved (one code defect; see below)

---

## What was verified

The implementation is real, not hollow — the prior structural-estimate preview
is gone and a genuine agent-run path is in its place:

- **Routes / clean cutover.** `POST /api/v1/meta-specs/preview`,
  `GET|DELETE /api/v1/meta-specs/preview/:preview_id` are registered
  (api/mod.rs:864-879) with ABAC resolver entries; the workspace-scoped
  routes and their exemption-file entries are removed (the frozen exemption
  list shrank, which the registry check accepts). `compute_preview_blast_radius`
  / `StructuralImpact` no longer exist.
- **Real spawn.** Per target: repo + workspace resolution, `check_repo_abac`
  plus explicit tenant containment, spec-path existence on the default
  branch, `preview/{preview_id}/{slug}` branch pinned to the default-branch
  tip, real `git worktree`, Active agent row (no task), short-lived preview
  JWT (`GYRE_PREVIEW_JWT_TTL_SECS`, default 1800 s, separate from
  `GYRE_AGENT_JWT_TTL`), budget slot increment. Provisioning is
  all-or-nothing with real rollback (proven by
  `preview_persist_failure_rolls_back_every_provisioned_agent`, which injects
  a kv failure at the latest persist step and asserts rows/tokens/index/
  record/branches/worktrees/budget slots are all cleaned).
- **Draft delivery.** `GYRE_PREVIEW_ID` / `GYRE_META_SPEC_DRAFT_KIND` /
  `GYRE_META_SPEC_DRAFT_CONTENT` / `GYRE_TARGET_SPEC_PATH` injected via
  `extra_env` merged *before* the server-owned vars (cannot shadow
  `GYRE_AUTH_TOKEN` etc.); `GYRE_TASK_ID` deliberately absent.
  `agent-runner.mjs` builds the preview prompt from those vars (draft as
  governing context over the target spec), withholds task-mutating MCP tools,
  skips conversation upload and `gyre_agent_complete`. Injection is proven
  against a real spawned process env dump
  (`preview_spawns_real_agents_with_branches_worktrees_and_draft`).
- **Skip-ceremony enforced, not just skipped.** MCP dispatch refuses
  task/MR/provenance tools for preview agents; the REST surface refuses
  preview-agent callers on `POST /agents/{id}/complete`,
  `POST /merge-requests`, `POST|PUT /tasks`, `POST /merge-queue/enqueue`
  (guards at spawn.rs:2892, merge_requests.rs:267, tasks.rs:190/308,
  merge_queue.rs:64, mcp.rs:4897). Tests drive the production
  `build_router` with the preview agent's live JWT and include admin
  positive controls.
- **Completion semantics.** Every compute-target monitor routes exits through
  `spawn::on_agent_process_exit` → `finish_preview_agent`: token revoked,
  worktree removed, budget slot released (idempotent via the `released`
  flag), agent row `Stopped` — never `Idle`. The branch survives for
  diffing; the status endpoint derives per-agent state from the real agent
  rows and surfaces `git_ops.diff(base_sha, branch)` — the diff base is
  pinned at provisioning so a moving default branch cannot pollute the diff.
- **Cleanup.** `DELETE` kills through the backend recorded at spawn time
  (`spawned_backends` — the F1 repair, regression-tested with a real process
  behind a mislabeled ssh-shaped handle), stops rows, force-removes
  worktrees, closes open MRs on preview branches, deletes branches, revokes
  tokens, drops kv records; 404 on unknown id; second DELETE 404. GC job
  `meta_spec_preview_gc` (interval `GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS`,
  TTL `GYRE_META_SPEC_PREVIEW_TTL_HOURS`, default 24 h) tears down expired
  runs and reclaims orphaned `preview_agents` index entries; the TTL boundary
  (at/past deleted, just-under retained) is tested.
- **Budget.** Workspace budget by default with all N slots of one request
  reserved up front (`check_spawn_budget_for`, multi-target 429 test);
  independent `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` cap when set (429 test).
- **UI.** MetaSpecs workspace view and EditorSplit's meta-spec context are
  both cut over to the §5 API (draft+targets POST, per-agent status/diff
  polling, target tabs, Clean Up → DELETE); stale-poll generation counter,
  bounded polling with honest "stalled" state, honest failure paths.
- **Docs.** api-reference.md (routes + a full Preview Mode section with the
  spec's skip-table), server-config.md (four new knobs),
  docker/gyre-agent/README.md (preview env vars).

### Probes run here (logs in /tmp/stage/review-evidence)

- `cargo check -p gyre-server` and `cargo check -p gyre-server --lib --tests`
  — exit 0 (test code type-checks; execution deferred to the verifier per
  the stage build wrapper).
- `cargo test -p gyre-adapters` — 574 passed, 0 failed (new
  `SshTarget::from_handle` round-trip/refusal tests, git2_ops worktree and
  branch deletion).
- `npm ci` + targeted vitest (MetaSpecs + EditorSplit: 164 passed) and the
  full `npm test` (122 files, 2441 passed / 41 skipped) — exit 0.
- All mechanical invariant scripts pass (arch, abac route registry —
  exemption file shrank, exempt handlers, mcp write tools, mem port
  contracts, in-memory state stores, byte-slice, relative paths, fail-open
  refs, scope literals, fabricated scopes, inert enforcement, lossy secrets,
  forwarded headers, unbounded http, forged scope fields, migration
  versions/portability, dead message kinds).

### Verifier must run (gyre-server test codegen is blocked in this stage)

`cargo test -p gyre-server --lib -- preview_`, `-- monitor_iteration
register_spawned_agent admin_kill_terminates kill_process_handle`, `--
mcp_preview_agent_denied_ceremony_tools`, then the full workspace suite and
fmt/clippy gates. The web suite passed here already.

---

## Findings

### F1 (code — blocking): preview launch misjudges ssh-docker and kubernetes starts, breaking §5 on those backends and orphaning the running process

`launch_preview_agent` (crates/gyre-server/src/api/meta_specs.rs, end of the
function) decides "started" as:

```rust
let started = if target_type == "container" {
    outcome.container_id.is_some()
} else {
    outcome.pid.is_some()
};
```

But `SshDockerTarget::spawn_process` returns `ProcessHandle { pid: None, id:
<remote container id> }` (ssh.rs:495-512; `container_mode` defaults to true
in the dispatch arm) and `KubernetesTarget::spawn_process` returns
`ProcessHandle { pid: None, id: <pod name> }` (kubernetes.rs:314-320). For a
workspace/tenant compute target of type Ssh (docker mode, the agent-runtime
§3 default) or Kubernetes, `outcome.pid` is therefore always `None`: a remote
container/Pod that **did** start is reported as "`ssh`/`kubernetes` process
never started". The agent is transitioned Failed with its token revoked and
worktree removed (`mark_preview_agent_failed` performs no process kill), so
the actually-started remote container/Pod keeps running unmanaged. No diff is
ever produced — preview mode is broken on two of the five compute backends.
The normal spawn path treats the same dispatch as successful
(`AgentLaunchResult.launched` is true); `dispatch_agent_launch` drops the
`launched` flag when returning to `launch_taskless_agent_process`, which is
why the preview path had to guess — and the guess is wrong. The codebase's
own test comment (spawn.rs:6406-6408) records the fact this guess ignores:
"container and Pod handles (their `pid` is None)".

Reproduce: configure an Ssh or Kubernetes compute target on the target
workspace (or tenant default); POST /api/v1/meta-specs/preview with one valid
target; the 202 lists the agent, but GET status shows "failed" from the first
poll and the server log carries "ssh process never started" (or kubernetes),
while the remote container/Pod is alive.

Fix direction: propagate `launched` out of `dispatch_agent_launch` (or accept
`container_id.is_some() || pid.is_some()`), and have
`mark_preview_agent_failed` kill through the recorded handle/backend pair
when one is registered. No existing test covers a preview launch on an
ssh/k8s compute target — add one (a config-level ssh-docker/k8s target with
the dispatch asserted `launched`, or at minimum a unit test pinning the
started predicate against SshDocker/Kubernetes handle shapes).

### F2 (docs — minor): duplicated paragraph in api-reference.md

The "Meta-Spec Preview Mode" section of docs/api-reference.md ends with the
sentence "Preview agents are killed on completion (no idle state), and their
`preview/{preview_id}/*` branches are auto-deleted after the TTL
(`GYRE_META_SPEC_PREVIEW_TTL_HOURS`)." twice in a row. Cosmetic duplication;
collapse to one.

---

## Non-blocking observations

- `specs/coverage/system/meta-spec-reconciliation.md` §5 row still reads
  `task-assigned … Hollow (code-verified 2026-09-29)`. It is stale in the
  conservative direction (it does not overclaim), and the verifier regenerates
  coverage artifacts from the merged matrices — but the row should be flipped
  to reflect the shipped §5 implementation at integration so the matrix keeps
  matching reality.
- Run-level `state` in GET status is `"complete"` whenever no agent is
  `running`, including all-failed runs (per-agent `status` still reports
  `failed`). The spec does not define a run-level failure state; noting for
  the record.
- Prior-round shipped notes and repair history in specs/tasks/task-206.md are
  consistent with the code as reviewed (F1 kill-routing, F2 registry-pair
  purge, F3 persist rollback all verified in source and covered by tests).

## Verdict

Not approved: F1 is a real production defect on supported compute backends.
Everything else in the task contract is genuinely implemented and tested; the
repair is small (started predicate + failed-launch kill + one regression
test).
