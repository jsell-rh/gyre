# Review — task-206 (Implement meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup)

Spec: `specs/system/meta-spec-reconciliation.md` §5 "Preview Mode: The Fast Iteration Loop".
Candidate: `4d8ce46` (merge head `169313da` + attribution commit), base `e8f73c06`.
Verdict: **approved** — no blocking findings. Evidence: `/tmp/stage/review-evidence/`
(summary.md plus raw logs). gyre-server test codegen is deferred to the independent verifier
by the review-stage build wrapper; the required server probes are listed there and below.

## What was reviewed

The full diff `e8f73c06..4d8ce46` (39 files): `api/meta_specs.rs` (preview run path, status,
DELETE, GC), `api/spawn.rs` (`launch_taskless_agent_process`, `AgentLaunchParams`,
`on_agent_process_exit` → `finish_preview_agent`, kill routing through `spawned_backends`),
`api/mod.rs` + `abac_middleware.rs` (global routes registered and resolver-mapped; the two
stale exemption lines for the removed workspace-scoped routes dropped by merge round
`169313da`), `api/budget.rs` (`check_spawn_budget_for` N-slot reservation), ceremony guards
(`api/tasks.rs`, `api/merge_requests.rs`, `api/merge_queue.rs`, `mcp.rs`,
`spawn.rs::complete_agent`), `jobs.rs`/`main.rs` (`meta_spec_preview_gc`), ports/adapters
(`force_remove_worktree`, `delete_branch`, `ProcessHandle.options`, `SshTarget::from_handle`),
`docker/gyre-agent/agent-runner.mjs` (preview prompt from the draft env, task tools withheld,
no provenance upload, no complete call), web UI (`MetaSpecs.svelte` real preview loop,
`EditorSplit.svelte`, `api.js`, locales), docs, coverage row 9.

## Probes run (independent evidence)

- All 19 mechanical invariant scripts pass (incl. `check-abac-route-registry.sh` and
  `check-abac-exempt-handlers.sh`, which failed on the pre-merge candidate — fixed by the
  final merge round dropping the two stale exemption lines; verified gone at the candidate).
- `cargo check -p gyre-server` — clean (7m39s, includes the build.rs web build).
- `cargo test -p gyre-adapters` — 576 passed, 0 failed (includes the new
  `SshTarget::from_handle` round-trip/refusal tests).
- `cd web && npm ci && npm test` — 125 files, 2510 passed, 41 skipped, 0 failed
  (preview suites alone: 174/174).
- Git push semantics probe: pushing to a branch checked out in a worktree of the bare repo
  is rejected by default git config, but `Git2OpsAdapter::init_bare` (pre-existing,
  git2_ops.rs:75) sets `receive.denyCurrentBranch=ignore` on platform repos — probe confirms
  the push is then accepted and lands on the branch, so the remote-agent preview push flow
  (deliverable = the pushed branch) is sound.

## Verified against the acceptance criteria

- **Real spawn path.** POST validates every target up front (repo 404, repo ABAC + explicit
  tenant containment 403, spec_path existence on the default branch 400, kind shape 400),
  then provisions per target: an `Active` agent row with real workspace scope, a
  `preview/{preview_id}/{slug}` branch pinned at the default-branch tip (`base_sha` recorded
  for the diff), a real git worktree via the git adapter, and a short-lived EdDSA JWT with
  scope claims derived from the loaded workspace/repo rows (never caller-supplied). Branch
  slugs are sanitized and collision-disambiguated. 202 response is exactly the spec shape.
  Provisioning is all-or-nothing with rollback of partial state (including the
  persist-failure path, which also drops a partially-written run record).
- **Draft reaches the agent.** `GYRE_PREVIEW_ID`, `GYRE_META_SPEC_DRAFT_KIND/CONTENT`,
  `GYRE_TARGET_SPEC_PATH` ride the standard env-injection block (`GYRE_TASK_ID`
  deliberately absent); `agent-runner.mjs` builds the preview prompt with the draft as the
  governing "how to build" context over the target spec, withholds task-mutating MCP tools,
  skips conversation upload and the complete call. The happy-path test asserts the draft text
  lands in the spawned process's real environment (env-dump entrypoint script), not a field.
- **Skip-ceremony is enforced, not merely skipped.** MCP dispatch refuses
  create/update-task, create-mr, agent-complete and conversation-upload for preview agents;
  the REST surface refuses `POST /agents/{id}/complete`, `POST /merge-requests`,
  merge-queue enqueue and task create/update for preview-agent callers (guards keyed off
  the `preview_agents` kv discriminator + agent-JWT scope, not the advisory client-side
  allowedTools). Git push gates skip preview agents naturally (no task_id → constraint gate
  skipped; non-default branch → manifest/cycle gates skipped). No MR/provenance/task/
  refs writes asserted in tests. Teardown also closes any open MR on a preview branch.
- **Budget.** Workspace budget by default, with a new `check_spawn_budget_for` that reserves
  all N slots of a multi-target request up front (fixes an over-admit window);
  `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` caps preview spawns independently when set (429).
  Slot release is idempotent across process-exit, DELETE and GC via the `released` flag
  synced through both kv records (`mutate_preview` rewrites run record and per-agent index).
- **Completion semantics.** Every compute-target monitor routes exits through
  `on_agent_process_exit` → `finish_preview_agent`: token revoked, worktree removed, slot
  released, row → `Stopped` (never `Idle`); branch survives for diffing. The
  `launched`-flag check correctly treats pid-less SshDocker/Kubernetes handles as starts
  (regression tests pin this with fake ssh/kubectl binaries).
- **Status + diff.** GET reports per-agent state derived from the real agent rows and the
  diff of the preview branch against the pinned base sha (MR diff plumbing), 404 unknown.
- **DELETE + GC.** DELETE kills via the backend recorded at spawn time (not handle-shape
  reconstruction — pinned by a regression test with an ssh-shaped handle behind a Local
  backend), removes worktrees/branches/records; 404 unknown. `meta_spec_preview_gc`
  registered in the job registry and spawned in main.rs; TTL default 24h
  (`GYRE_META_SPEC_PREVIEW_TTL_HOURS`), interval 1h; boundary test (at/past TTL deleted,
  fresh retained) plus an orphan-reclamation pass.
- **Clean cutover.** Hollow `compute_preview_blast_radius`/`StructuralImpact` and the
  workspace-scoped preview routes are gone (grep-verified, registry checks green); blast
  radius stays on §4's endpoint. Docs (`api-reference.md`, `server-config.md`,
  `docker/gyre-agent/README.md`) updated; coverage row 9 flipped to implemented with
  accurate evidence.
- **UI.** `MetaSpecs.svelte` implements the §5 iteration loop against the real endpoints
  (target selector, per-agent tabs, live diff, Clean Up → DELETE, bounded polling with
  generation-counter invalidation); `EditorSplit` keeps backward-compatible props (all new
  props defaulted; `DetailPanel` usage unchanged); full vitest suite green.

## Non-blocking observations (recorded, not findings)

- `scripts/e2e-flow.sh` (pre-existing, ~line 1783) still probes the removed
  workspace-scoped preview routes with an empty body; it degrades to an "not available"
  info line (no CI failure) but no longer exercises preview at all. The task plan did not
  include the e2e script; follow-up hygiene only.
- Env-mutating tests (`GYRE_AGENT_COMMAND`, PATH shadowing) use the repo's established
  ENV_LOCK serialization pattern (pre-existing in email.rs/lib.rs/message_dispatcher.rs) and
  CI runs nextest (per-test process isolation) — safe as shipped.

## Deferred verifier probes

`cargo nextest run --all` (CI parity), in particular the gyre-server suites:
`-- preview_`, `-- merge_queue`, `-- uninstrumented_role_guards`,
`-- mcp_preview_agent_denied_ceremony_tools`, `-- prompt`; plus `cd web && npm ci && npm test`
(already green in this review).
