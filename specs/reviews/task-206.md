# Review: task-206 — Meta-spec preview mode: real agent preview runs

**Reviewer:** Independent review (pi runtime)
**Round:** R-current (candidate 14e471f4, base 00669baa)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** 00669baacfd5112b24e1466ad51f5a9bec43d8e9
**HEAD reviewed:** 14e471f4c91422c6b85589ec1fc178cc0280c2b8 (verified `git rev-parse HEAD`)

**Status:** complete — approved. No findings.

---

## Scope note

The diff base→candidate includes merge commits integrating four later bases
(task-253/258/261/263); the task-206 surface was reviewed via the candidate's
authored preview code plus the merge-conflict resolutions in the eight
colliding gyre-server files (both task-263's task-scope guards and task-206's
preview guards verified present on the shared handlers — spot-checked
`complete_agent`, `stop_agent`, `fail_agent`, `update_task`, orchestrator
spawns). The uncommitted `specs/tasks/task-206.md` modification is the stage
harness resetting frontmatter to the assignment state (matches
`review-before.json`), not candidate content.

## Independent evidence (saved under /tmp/stage/review-evidence)

| Probe | Result |
|---|---|
| `bash scripts/check-abac-route-registry.sh` | OK (new `/api/v1/meta-specs/preview` and `/preview/:preview_id` registered in the ABAC resolver, abac_middleware.rs:774-779; no exemptions added — `scripts/abac-route-registry-exemptions.txt` shrank by 2) |
| `bash scripts/check-abac-exempt-handlers.sh` | OK (84 handlers; registry-exempt stop/fail/usage routes now carry per-handler auth) |
| `bash scripts/check-arch.sh` | OK |
| `check-mcp-write-tools`, `check-migration-versions`, `check-dead-message-kinds`, `check-byte-slice-truncation`, `check-relative-path-defaults`, `check-fail-open-ref-resolution`, `check-mem-port-contracts`, `check-in-memory-state-stores`, `check-inert-enforcement` | all OK |
| `/tmp/stage/bin/cargo check -p gyre-server` | exit 0 (7m16s) |
| `/tmp/stage/bin/cargo check -p gyre-server --tests` | exit 0 (3m33s — the ~7k lines of new tests type-check) |
| `/tmp/stage/bin/cargo test -p gyre-adapters` | 597 passed, 0 failed (incl. new `SshTarget::from_handle` round-trip/malformed-id tests and git2_ops worktree/branch ops) |
| `npm ci` + `npx vitest run MetaSpecs.test.js EditorSplit.test.js` | 178 passed, 0 failed |

## Findings on each acceptance criterion

**A1 — real spawns on `preview/{preview_id}/{slug}` branches.** Verified in
source and test. `post_meta_spec_preview` (meta_specs.rs:832) resolves every
target up front (repo 404, workspace, `check_repo_abac`, non-Admin tenant
containment, spec_path existence on the default branch → 400/403/404), pins
`base_sha` via `resolve_ref` (no fail-open default — matches the repo
invariant), creates the branch, a real `Active` agent row scoped to the repo's
workspace, a real worktree (`Git2OpsAdapter::create_worktree`), and a
short-lived JWT minted with caller-derived tenant/workspace/repo claims.
`preview_spawns_real_agents_with_branches_worktrees_and_draft` runs a real
bare git repo through `Git2OpsAdapter` and asserts agent rows, branches, and
on-disk worktrees. Rollback compensation on provision/persist failure is
covered by dedicated failure-injection tests.

**A2 — draft reaches the agent as governing context.** `extra_env` carries
`GYRE_PREVIEW_ID`/`GYRE_META_SPEC_DRAFT_KIND`/`GYRE_META_SPEC_DRAFT_CONTENT`/
`GYRE_TARGET_SPEC_PATH`; merged before server-owned vars so it cannot shadow
identity/credentials. `docker/gyre-agent/agent-runner.mjs` builds a preview
prompt embedding the draft text as the governing "how to build" context over
the target spec, withholds task-mutating MCP tools, and never calls complete.
The env-dump test asserts the actual draft text appears in the spawned
process environment (not a field-set assertion). Env-var name deviates from
the plan's suggested `GYRE_SPEC_PATH` (impl: `GYRE_TARGET_SPEC_PATH`) — the
spec names no env vars; server, runner, and docs are consistent.

**A3 — 202 spec shape.** `PreviewResponse { preview_id, agents: [{agent_id,
repo_id, spec_path, branch}] }` exactly; asserted in tests.

**A4 — skip-ceremony enforced.** No task assignment (`task_id: None`), no
gates, no MR, no merge queue, no provenance, no `refs/agents/*` /
`refs/tasks/*` writes (asserted by ref-counting in the spawn test). Beyond
skipping, escape routes are closed server-side: MCP dispatch refuses
`gyre_create_task`/`gyre_update_task`/`gyre_create_mr`/`gyre_agent_complete`/
`conversation_upload` for preview agents; REST refuses preview-agent callers
on complete (both directions), MR create/status, merge-queue enqueue/cancel,
task create/update/transition, agents/status, agents register (identity
laundering), specs approve/save, commits/record, compose apply/teardown,
orchestrator spawns, stop/fail. Budget: workspace budget by default
(`check_spawn_budget_for` reserves N slots per workspace up front —
over-admission fix), `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` caps preview spawns
independently when set, fail-closed on enumeration errors; slot release is
exactly-once across every generic terminal path via the durable
`mark_preview_slot_released` flag (stop/fail/budget-stop/stale-abort all
flagged; covered by dedicated double-release tests).

**A5 — GET status with real state + diff.** Per-agent status derived from the
agent rows via `preview_agent_status`; diff computed by the git adapter
against the pinned base SHA (same `DiffResult` plumbing as the MR diff),
`null` until the branch exists. `preview_agent_finish_teardown_and_diff`
drives a real agent script that commits on the preview branch, waits for the
monitor to observe the exit, and asserts the surfaced diff contains the
agent's commit.

**A6 — no idle state.** Every monitor exit routes through
`on_agent_process_exit` → `finish_preview_agent` (token revoked, worktree
removed, slot released, row Stopped — never Idle; branch kept for diffing).
All other Idle transition sites are unreachable for preview agents
(complete/MCP-complete refused, signal-chain path is orchestrator-only).

**A7 — DELETE.** 404 unknown id; kills processes through the recorded
backend+handle pair (not handle-shape guessing — covered by
`preview_kill_routes_through_recorded_backend_not_handle_shape` and the
ssh/kubernetes launch tests), stops rows, acks orphaned inboxes, removes
worktrees, closes dangling MRs on preview branches, deletes branches,
revokes tokens, releases slots, drops kv records. GET status afterwards 404s
(asserted).

**A8 — GC.** `meta_spec_preview_gc` job registered in `start_job_registry`
(interval `GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS`, default 3600) with a
handler-binding test that triggers the real sweep; spawned from `main.rs`.
TTL `GYRE_META_SPEC_PREVIEW_TTL_HOURS` default 24h; `preview_gc_ttl_boundary`
proves at/past-TTL deletion, just-under retention, and the orphan pass.

**A9 — clean cutover.** `compute_preview_blast_radius` / `StructuralImpact`
removed (grep: no matches); workspace-scoped preview routes removed from
mod.rs; frontend `api.js` cut over to the global routes; §4 blast-radius
endpoint untouched.

**A10 — docs.** `docs/api-reference.md` (new routes + full "Meta-Spec Preview
Mode" section with skip-table and enforced-refusal list) and
`docs/server-config.md` (four new knobs) updated; `docker/gyre-agent/README.md`
documents the preview env contract.

**A11 — tests.** Hard tests against real git repos and real spawned processes
(env-dump and commit scripts), real state transitions, failure-injection
rollback, cross-tenant/ABAC denials, budget cap and fail-closed enumeration,
MCP refusal with a non-preview negative control. Frontend tests (178) assert
the real §5 wire shapes including running→complete transitions and diff
rendering. Coverage matrix row 9 flipped to implemented with an accurate
evidence summary.

## Generated-output observation (non-blocking, per review rules)

`web/dist/` in the merge commit is the 3-way merge result of previously
built assets; the deterministic verifier owns the post-integration rebuild.

## Verifier must run (not run here — large gyre-server test build)

- `cargo test -p gyre-server` (full suite; at minimum the preview block:
  `preview_spawns_real_agents_with_branches_worktrees_and_draft`,
  `preview_agent_finish_teardown_and_diff`, `preview_gc_ttl_boundary`,
  `preview_delete_tears_down_everything`, `preview_validation_and_role_gates`,
  `preview_post_cross_tenant_and_abac_denial_are_forbidden`,
  `preview_status_and_delete_cross_tenant_are_forbidden`,
  `preview_budget_cap_rejects_overflow`,
  `preview_budget_enumeration_failure_fails_closed`,
  `preview_agent_rest_ceremony_is_refused` and the other refusal tests,
  `preview_persist_failure_rolls_back_every_provisioned_agent`,
  `mark_preview_agent_failed_kills_registered_process`,
  `mcp_preview_agent_denied_ceremony_tools`, `meta_spec_preview_gc_job_registered`)
- `cargo test --all` and `cd web && npm test` per repo definition of done.
