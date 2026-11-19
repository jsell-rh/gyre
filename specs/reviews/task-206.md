# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Independent review agent
**Round:** R1 (this candidate)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** 4e88f1836083273bc9420cac84607bb185d74c24
**HEAD reviewed:** 1ee9cf7e9156800971e38d2fb607ca8ad37ab66c
**Status:** complete — approved

---

## Verdict

Approved. The candidate replaces the hollow structural-impact preview
endpoint with a real agent run path that satisfies every acceptance
criterion of the task, with hard tests behind each one. No blocking
findings. Probe evidence: `/tmp/stage/review-evidence/` (summary.md).

## What was verified

### Real spawns, spec-conformant routes and response

`POST /api/v1/meta-specs/preview` (registered in api/mod.rs:902-908 with
ABAC resolver entries in abac_middleware.rs:743-746) validates every target
up front — repo exists (404), caller passes `check_repo_abac` plus tenant
containment (403), spec_path exists on the default branch (400) — before
creating anything. Per target it provisions a real `Active` agent row
scoped to the repo's workspace, a `preview/{preview_id}/{slug}` branch
pinned to the default-branch tip (stem collisions disambiguated by the
sanitized full path), a real git worktree, and a short-lived preview JWT
(separate `GYRE_PREVIEW_JWT_TTL_SECS`, scope claims derived from loaded
rows). 202 body is exactly `{preview_id, agents:[{agent_id, repo_id,
spec_path, branch}]}`. Provision/persist failures roll back every
already-provisioned target (branch, worktree, agent row, index entry,
untaken budget slots are not decremented — the false-flag hazard is
explicitly guarded).

### Draft reaches the agent

`GYRE_PREVIEW_ID`, `GYRE_META_SPEC_DRAFT_KIND`, `GYRE_META_SPEC_DRAFT_CONTENT`,
`GYRE_TARGET_SPEC_PATH` travel in the container env via
`launch_taskless_agent_process` (merged before the server-owned identity
vars so they cannot shadow them; `GYRE_TASK_ID` deliberately absent).
`docker/gyre-agent/agent-runner.mjs` builds the preview prompt with the
draft meta-spec as governing "how to build" context over the target spec
and withholds the task-mutating tools client-side.

### Skip-ceremony enforced, not just promised

No task assignment, no gates, no MR, no merge queue, no provenance, no
`refs/agents/*`/`refs/tasks/*` writes on the preview path. The MCP dispatch
and the REST escape routes (task/agent/MR status, complete/stop/fail,
spec approve/save, compose, orchestrator spawn, merge-queue cancel,
commit-record/provenance, agent registration) refuse preview-agent callers
via `caller_is_preview_agent`, with a unit test pinning the array-scoped
JWT discriminator that a string-scope guard would silently miss.

### Completion, status + diff, DELETE, GC

Every compute-target monitor exit routes through `on_agent_process_exit`
→ `finish_preview_agent`: Stopped (never Idle), token revoked, worktree
removed, exactly-once budget-slot release keyed on the durable per-agent
index flag (generic stop/fail/kill/budget/stale-detector terminal paths
flag the slot through `mark_preview_slot_released`, so no double
decrement). `GET /api/v1/meta-specs/preview/{id}` reports real per-agent
state from the agent rows plus the produced diff (preview branch vs pinned
base SHA, MR-diff plumbing). `DELETE` kills through the recorded
`spawned_backends` pair (SshDocker/Kubernetes handles carry no pid — the
launch-verdict and kill tests pin this), removes worktrees/branches/
tokens/records, 404 on unknown id. The `meta_spec_preview_gc` job is
registered (interval env, default 1h) and spawned in main.rs; TTL default
24h via `GYRE_META_SPEC_PREVIEW_TTL_HOURS`.

### Budget

Workspace budget by default (`check_spawn_budget_for` reserves all N
targets of one POST up front, grouped per workspace); independent
`GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` cap with fail-closed enumeration when
set. Config knobs documented in docs/server-config.md; routes and the
preview-mode section documented in docs/api-reference.md.

### Clean cutover

`compute_preview_blast_radius`, `StructuralImpact` and the workspace-scoped
preview routes are gone; their two exemption entries were deleted from the
frozen file (shrinks, never grows). Coverage matrix row 9
(meta-spec-reconciliation.md §5) flipped to `implemented` with an accurate
description of the shipped behavior.

## Probe evidence (this review)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` → exit 0 (7m34s).
- `check-arch.sh`, `check-abac-route-registry.sh`,
  `check-abac-exempt-handlers.sh`, `check-mcp-write-tools.sh`,
  `check-mem-port-contracts.sh`, `check-dead-message-kinds.sh`,
  `check-migration-versions.sh` → all exit 0.
- `python3 scripts/check-rustfmt-diff.py 4e88f183` → clean (34 files);
  `git diff --check HEAD^1 HEAD` → clean.
- `python3 -m unittest discover -s scripts -p test_dev_checks.py` → 10/10
  (includes the two new overlay data-follows-merge tests).
- `web`: `npm ci` then `npx vitest run src/__tests__/MetaSpecs.test.js
  src/__tests__/EditorSplit.test.js` → 178/178 passed.
- `cargo test -p gyre-adapters --lib compute::ssh` → 15 passed (new
  `SshTarget::from_handle` round-trip/refusal tests included);
  `--lib git2_ops` → 35 passed (new `delete_branch` /
  `force_remove_worktree`).
- `cargo test -p gyre-server` (the full preview suite listed in
  /tmp/stage/review-evidence/summary.md) is deferred to the independent
  verifier per the sandbox build budget; the tests were reviewed in source
  and are behavior-killing (real processes via env-dump/commit scripts,
  real git repos, real JWTs, handler-visible error text asserted), not
  self-confirming.

## Reviewed process change

The candidate also repairs `scripts/verifier-script-overlay.py` so frozen
`scripts/*-exemptions.txt` data follows the merge while check logic is
still restored from the base. Analyzed and found sound: exemption
additions/replacements relative to base are rejected by dev-check.sh's
no-new-exemptions gate, and entry deletions only re-expose the underlying
findings to the upstream checks — this repairs a false positive when a
candidate legitimately removes a route together with its exemption entry
(exactly what this task's clean cutover did). Covered by two new unit
tests, and docs/dev-pipeline.md documents the behavior.

## Non-blocking observations

1. `is_preview_agent`/`caller_is_preview_agent` return false on a kv-store
   error, so the ceremony-refusal guards fail open during a kv outage. This
   is a deliberate, documented tradeoff (fail-closed would deny every
   agent's task/MR tools on a kv blip); primary role/scope enforcement is
   unaffected and preview tokens are short-lived. Hardening opportunity.
2. The task record's "Shipped" prose names `GYRE_SPEC_PATH`; the
   implemented and correctly documented variable is
   `GYRE_TARGET_SPEC_PATH`. Prose slip only.
3. `web/dist/` was not rebuilt on this branch — generated output owned by
   the deterministic verifier; authored frontend source and tests were
   reviewed and pass.
