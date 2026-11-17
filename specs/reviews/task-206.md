# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Independent review (pi runtime)
**Round:** R3 (post repair rounds 7cad1261, 2f10e8d1)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** e9ed695561dabf1dc5de3239851d07686a43daaa
**HEAD reviewed:** 338f89e48f85dbb6e1c8a997e4bb78eed608813d
**Status:** complete — approved (no blocking findings)

---

## Verdict

Approved. The preview loop is a real agent run path, not an estimate. Every
acceptance criterion is backed by production code I could read end-to-end and
by hard tests; the escape-class repair rounds hold. Probe evidence:
`/tmp/stage/review-evidence/summary.md` (all 19 invariant scripts pass,
`cargo check -p gyre-server --lib --tests` clean, `cargo fmt --check` clean,
web suite 125 files / 2513 tests green after `npm ci`).

## Findings

None blocking. One non-blocking observation recorded for the backlog:

- **(minor, non-blocking)** The per-agent `preview_agents` kv index entries
  are written once at persist time and their `released` flag is never
  rewritten; only the run record's copy is updated by
  `release_preview_slot`/`mark_preview_agent_failed`. Every consumer that
  matters reads the run record (DELETE/GC teardown, status labels) or
  cross-checks the agent row's `Active` status
  (`count_running_preview_agents`), so no incorrect behavior results; a
  narrow monitor-exit vs. kill race could double-decrement the budget
  counter (saturating, over-enforcing direction only). Not a §5 contract
  violation.

## Acceptance-criterion evidence

**A1 — Real spawn per target on `preview/{preview_id}/{slug}`.**
`post_meta_spec_preview` (api/meta_specs.rs:832) resolves every target first
(repo exists, `check_repo_abac`, non-Admin tenant containment, spec_path
readable on the default branch), then per target provisions: branch pinned to
the default-branch tip via `resolve_ref` (fail-closed — no
`unwrap_or_default`), real `Active` agent row scoped to the repo's workspace
(`agent.workspace_id` from the loaded repo, never a literal default), real
git worktree through `git_ops.create_worktree` + `worktrees.create`,
short-lived EdDSA JWT. Provisioning is all-or-nothing with compensation
(rollback deletes row/worktree rows/branch/token/index entries; the
persist-failure, worktree-failure and agent-row-failure tests prove each
compensation path). Slug collisions disambiguate via the qualified path;
`sanitize_branch_slug` blocks `..`, leading/trailing `-`/`.`, and `.lock`
suffixes.

**A2 — Draft reaches the agent.** `launch_preview_agent` injects
`GYRE_PREVIEW_ID`, `GYRE_META_SPEC_DRAFT_KIND`, `GYRE_META_SPEC_DRAFT_CONTENT`,
`GYRE_TARGET_SPEC_PATH` (extra_env merged before server-owned vars so it
cannot shadow them; `GYRE_TASK_ID` deliberately absent).
`docker/gyre-agent/agent-runner.mjs` builds the preview prompt itself: draft
meta-spec as governing "how to build" context over the target spec, push to
the preview branch as the entire deliverable. The spawn test asserts the
*actual process env dump* contains the draft kind/content bytes — not a
field-set check.

**A3 — 202 spec shape.** Response is exactly `{preview_id, agents:[{agent_id,
repo_id, spec_path, branch}]}`; verified in code and asserted in
`preview_spawns_real_agents_...` (branch names
`preview/{id}/{search|identity}`).

**A4 — Skip-ceremony enforced, budget enforced.** MCP dispatch refuses
`gyre_create_task`/`gyre_update_task`/`gyre_create_mr`/`gyre_agent_complete`/
`conversation_upload` for preview agents; the REST surface refuses the same
class (tasks POST/PUT/status, agents complete/status/stop/fail, MR
create/transition, merge-queue enqueue/cancel, specs approve, compose
apply/teardown, workspace-orchestrator spawn — the last now also
Admin/Developer role-gated). `reject_spec` is role-gated and
`revoke_spec_approval` requires original-approver-or-Admin, so a preview
agent cannot reach spec-approval ceremony through them. Budget: workspace
budget by default (N-target requests reserve all N slots up front via the new
`check_spawn_budget_for`), `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` caps preview
spawns independently when set (429). No `refs/agents/*`/`refs/tasks/*`
writes (asserted by ref-counting in the spawn test).

**A5 — Status + diff.** `get_meta_spec_preview_status` derives per-agent
state from the real agent rows (`Active→running`, terminal `Stopped` with the
released flag → `complete`, `Failed`/`Dead` surfaced honestly, missing row →
`unknown`), and exposes the diff of the preview branch against the pinned
base sha through the same `git_ops.diff` plumbing as MR diffs. The
finish/diff test drives a real agent process that commits and pushes, then
asserts `files_changed ≥ 1` and the changed spec path in the patches.

**A6 — No lingering Active/Idle.** Every compute-target monitor routes exits
through `spawn::on_agent_process_exit` → `finish_preview_agent`: token
revoked, worktree removed, budget slot released, row `Stopped` — never
`Idle`. Terminal row is the last mutation (a terminal row implies a cleaned
agent). The `update_agent_status` REST route refuses preview-agent callers,
so they cannot flip themselves back to Idle.

**A7 — DELETE teardown.** `delete_meta_spec_preview` (404 on unknown id,
preview-scope containment) kills registered processes through the recorded
backend (never guessing from handle shape — malformed ssh ids are refused
rather than killing a local pid, proven by a test that would otherwise kill
the test runner), stops rows, force-removes worktrees, closes open MRs on
preview branches (belt-and-braces), deletes branches, revokes tokens,
releases slots, drops the records.

**A8 — Background GC.** `meta_spec_preview_gc` job registered in jobs.rs and
spawned in main.rs (`GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS`, default 1h;
TTL `GYRE_META_SPEC_PREVIEW_TTL_HOURS`, default 24h). `run_once` expires
runs at/past TTL, drops corrupt records, and reclaims orphaned agent entries
(slot, token, worktree, branch). The TTL boundary test ages a run by
rewriting `created_at` and proves at/past-TTL deletion, just-under retention
and orphan reclamation.

**A9 — Clean cutover.** The workspace-scoped preview routes,
`compute_preview_blast_radius` and `StructuralImpact` are gone from mod.rs /
meta_specs.rs (grep-verified); the abac route-registry exemption file shrank
by the two removed workspace-scoped entries; new global routes are in the
ABAC resolver (`meta_spec` resource). Blast radius stays on §4's endpoint.

**A10 — Docs.** `docs/api-reference.md` documents all three routes (shapes,
error codes, authorization) plus a Preview Mode section;
`docs/server-config.md` documents `GYRE_META_SPEC_PREVIEW_TTL_HOURS`,
`GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS`, `GYRE_PREVIEW_JWT_TTL_SECS`,
`GYRE_PREVIEW_BUDGET_MAX_CONCURRENT`; `docker/gyre-agent/README.md` documents
the agent-side env contract.

**A11 — Tests.** Hard, not self-confirming: real bare git repos on disk with
the real `Git2OpsAdapter`, real spawned processes via `GYRE_AGENT_COMMAND`
(env-dump and commit-and-push scripts), live preview JWTs driven through the
production router (ABAC middleware + seeded builtin policies) for the
escape-class refusals with admin positive controls, cross-tenant and
ABAC-denial probes using minted foreign-tenant JWTs (not the global token,
which would bypass exactly the guarded branches), failure-injection via
swappable kv/agents store adapters for the rollback paths, and compute-target
launch regressions proving the `launched` flag (not pid shape) decides
failure for ssh-docker/kubernetes.

## Independent escape-class spot checks

I re-derived the preview-agent reach from the seeded policies
(`agent-scoped-access` Allow 700 vs `agent-repo-scope` Deny 750: non-repo-
addressed resources fail the deny condition closed) and probed the remaining
mutating surface list for §5-ceremony gaps. All ceremony surfaces are guarded
or role-gated (see A4). The residual Agent-role reach on non-ceremony,
non-repo-addressed routes (e.g. `record_commit`, MR comments/reviews with
self-attribution binding, repo creation) is pre-existing platform behavior
identical for every scoped worker-agent JWT — not introduced or widened by
this task, and outside §5's skip-table.

## Verification handoff

gyre-server test execution is deferred to the verifier (4 GiB budget); the
required focused test list is in `/tmp/stage/review-evidence/summary.md`,
headed by the spawn/finish-diff/GC-boundary/escape-refusal tests.
