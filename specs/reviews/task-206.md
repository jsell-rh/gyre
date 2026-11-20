# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Review agent (independent)
**Round:** final (candidate `7085496b5c7929ab2f1d027c87d4ddca9ae72788`, base `0a70b31effd9e6e85392c840fec915b1d5dc8567`)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Status:** complete — approved

---

## Verdict

Approved. The §5 preview loop is a real agent run path, not an estimate. Every
acceptance criterion is backed by production code and hard tests; no fakery,
no missing enforcement, no unsafe scope handling found. Probes run in this
sandbox (evidence: `/tmp/stage/review-evidence/task-206-final/`):

| Probe | Result |
|---|---|
| `cargo check -p gyre-server --tests` (via `/tmp/stage/bin/cargo`) | exit 0 (6m47s) |
| `cargo test -p gyre-adapters compute::ssh` | 19 passed, 0 failed (incl. both new `ssh_path` tests) |
| `cd docker/gyre-agent && node --test agent-runner.test.mjs` | 12 passed, 0 failed (incl. "preview mode: draft reaches the agent, ceremony skipped") |
| `cd web && npm ci && npm test` | 127 files, 2586 passed, 41 skipped, 0 failed |

gyre-server test *execution* is reserved for the independent verifier (4 GiB
build budget); required commands listed at the end.

## Acceptance-criteria evidence

**Spawn is real.** `post_meta_spec_preview` (meta_specs.rs:832) →
`resolve_preview_targets` (repo exists → 404, `check_repo_abac` + tenant
containment → 403, spec_path readable on the default branch → 400) →
`spawn_preview_run` → `provision_preview_agent`: real `Agent` row
(`Active`, `workspace_id` from the repo record, `current_task_id: None`),
branch `preview/{preview_id}/{slug}` pinned to the resolved default-branch
tip, real git worktree (`git_ops.create_worktree` + `worktrees.create`),
short-lived JWT minted through `agent_signing_key.mint` with
`preview_jwt_ttl_secs` (own TTL, default 1800s). Launch routes through
`spawn::launch_taskless_agent_process` on the workspace/tenant compute
target; a dispatch whose `launched` flag is false marks the agent Failed,
releases the slot and kills any registered process.
`preview_spawns_real_agents_with_branches_worktrees_and_draft` asserts all
of this against the stores and the filesystem, and reads the *spawned
process's own env dump* to prove the draft text reached
`GYRE_META_SPEC_DRAFT_KIND/CONTENT` — not a field assignment.

**Draft reaches the agent as governing context.** `agent-runner.mjs` builds
`previewPrompt` from the draft env vars: the draft meta-spec is embedded in
full as the governing "how to build" context over `GYRE_TARGET_SPEC_PATH`,
with "where the draft and your usual conventions disagree, the draft wins".
Verified by the passing node test.

**202 shape matches the spec exactly**: `{preview_id, agents:[{agent_id,
repo_id, spec_path, branch}]}` — no structural-impact fields; blast radius
stays on §4's endpoint. Tests hit `/api/v1/meta-specs/preview` (global route,
mod.rs:942-947).

**Skip-ceremony enforced.** No task row, no MR, no merge queue, no
provenance, no `refs/agents/*`/`refs/tasks/*` writes (all asserted in the
spawn test, including git-ref counts under the repo). Beyond passivity, the
escape routes are *refused*: MCP dispatch denies task/MR/provenance tools
and cross-agent liveness for preview-agent callers (mcp.rs:5118-5177);
REST ceremony routes (complete, MR creation, merge-queue, task/agent status,
orchestrator spawn, message injection) refuse `caller_is_preview_agent`.
`is_preview_agent` fails closed on a store outage (refusal, not pass-through).

**Budget.** Default: per-workspace `check_spawn_budget_for` with all N
targets of one POST reserved up front (a multi-target POST cannot over-admit
against a smaller workspace cap; tested). Separate configurable cap
`GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` returns 429 and *fails closed* when the
running-agent enumeration errors (tested via store outage). Slot release is
exactly-once across process-exit/DELETE/GC/generic terminal paths via the
durable `released` flag in the per-agent index.

**Status + diff.** `get_meta_spec_preview_status` derives per-agent state
from the real agent rows (`preview_agent_status_label`), and `preview_diff`
diffs the preview branch against the *pinned base sha* — a moved default
branch can never show up as the agent's deletions. `preview_agent_finish_teardown_and_diff`
covers the completion transition and diff exposure.

**No idle lingering.** Every compute-target monitor exit routes through
`on_agent_process_exit` → `finish_preview_agent`, which revokes the token,
force-removes the worktree, releases the slot, and lands the row in
`Stopped` — never `Idle`. The branch stays for the diff until DELETE/GC,
per spec. Inboxes are bulk-acked on both terminal paths.

**DELETE + GC.** `delete_meta_spec_preview` kills through the spawn-time
recorded backend (SshDocker/Kubernetes pid-less handles included — the
handle-shape regression is covered by dedicated ssh-docker and kubernetes
launch tests), removes worktrees, closes any dangling MR on the branch,
deletes branches, revokes tokens, drops the record; GET afterwards 404s
(tested). `meta_spec_preview_gc` job registered in jobs.rs:705-714 with
configurable interval; `preview_gc_ttl_boundary` proves at/past-TTL deleted,
just-under retained, plus the orphan pass reclaiming index entries whose run
record is gone.

**Clean cutover.** `compute_preview_blast_radius`/`StructuralImpact` and the
hollow handlers are gone (repo-wide grep: zero references). Docs updated:
api-reference.md §Meta-Spec Preview Mode (routes, shapes, error codes),
server-config.md (four new knobs), docker/gyre-agent/README.md.

**AuthZ tests.** Read-only role → 403, unknown repo → 404, no-repo-access →
403, cross-tenant target/status/delete → 403, scope surviving target-repo
deletion (recorded workspace tenant anchors the check; unresolvable
workspace denied) — all present as JWT-based tests that bypass the admin
token's containment bypass.

## Deviations from the plan (non-blocking, documented)

1. **Workspace-scoped preview routes retained, not removed.** The plan said
   to remove them; the frozen upstream ABAC exemption data (restored by the
   verifier's script overlay from the base tree) makes removal fail
   `check-abac-exempt-handlers.sh` — removal can never pass that gate
   (root-caused and documented in commits 5f5accd2/ff9d4e63). The sanctioned
   lifecycle was used instead: routes stay registered, now resolver-enforced
   with `RouteResourceMapping` entries (the base tree ran them with *no*
   policy evaluation), backed by the same real §5 machinery with strictly
   narrower containment (every target repo in the path's workspace + tenant
   check). The spec contract (§5 global routes) is fully met; this is an
   additive, more-contained surface. api-reference.md documents the true
   state. The coverage-row phrase "workspace-scoped routes removed" is
   slightly imprecise on this point, but the §5 status itself is accurate.
2. **`GYRE_TARGET_SPEC_PATH`** instead of the plan's `GYRE_SPEC_PATH` name —
   the contract (target spec reaches the agent) is met; naming is an
   implementation detail, consistently documented.

## Last-round repairs (commit 6fc510f8) — verified by source + focused probes

The 2026-11-20 verifier `cargo test --all` run failed 3 tests. Repairs:

- **Registry DELETE 404-vs-403**: `delete_meta_spec_registry` now matches
  base `0a70b31e` byte-for-byte in logic (leading `require_meta_spec_access`
  removed; 403-precedes-404 for non-Admin on missing rows, truthful 404 for
  Admin). Diff against base confirmed identical.
- **ssh-docker preview launch test**: the old test PATH-shadowed `ssh`,
  which `Command::new("ssh")` can never resolve. New `ssh_path` compute-target
  config knob (mirroring `kubectl_path`) drives every ssh invocation;
  wiring verified at spawn.rs (both dispatch sites), orchestrator.rs,
  compute.rs, and `ssh_docker_target_from_config`; the adapters tests
  `ssh_docker_spawn_uses_configured_ssh_path` /
  `..._defaults_to_path_ssh` pass in this sandbox, and the repaired
  server-side test now wires the fake via the config (no PATH mutation).

## Required verifier tests (gyre-server execution reserved for the verifier)

- `cargo test --all --quiet` (the finding's exact command)
- Focused: `cargo test -p gyre-server --lib preview_`,
  `cargo test -p gyre-server --lib registry_tests`,
  `cargo test -p gyre-server --lib registry_endpoints_reject_non_admin`,
  `cargo test -p gyre-server --lib registry_mutations_require_admin`,
  `cargo test -p gyre-server --lib preview_launch_on_ssh_docker_target_is_not_failed`,
  `cargo test -p gyre-server --lib preview_launch_on_kubernetes_target_is_not_failed`,
  `cargo test -p gyre-adapters compute::ssh`,
  `cd docker/gyre-agent && node --test agent-runner.test.mjs`,
  `cd web && npm ci && npm test`
- Static gates incl. `bash scripts/check-arch.sh`,
  `scripts/check-abac-route-registry.sh`,
  `scripts/check-abac-exempt-handlers.sh` (the exemption-file overlay
  interaction above), `scripts/check-mcp-write-tools.sh`
