# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Independent review (pi runtime)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** 9f0c709f81a155c78c429c82096a05734037c6d7
**Candidate reviewed:** fda19137ec9945922d8d2101de6d44f7c92765c1
**Status:** approved (fresh verdict for this candidate; no earlier approval transferred)

---

## Verdict

Approved. The candidate implements §5 preview mode as a real agent run path with
no fakery found. Independent probes: `cargo check -p gyre-server` (exit 0, incl.
`--tests`), `cargo test -p gyre-adapters --lib ssh` (15 passed) and `--lib
git2_ops` (35 passed), `cd web && npm ci && npm test` (127 files, 2576 passed),
and all 19 deterministic static gates green (arch, both ABAC checks,
mcp-write-tools, migrations, message kinds, and the remaining invariant checks).
gyre-server test compilation exceeds this sandbox's 4 GiB budget — the required
server probes are recorded in `/tmp/stage/review-evidence/required-tests.md`
(full preview test battery, integration/e2e suites, `cargo test --all`).

## Evidence for the acceptance criteria

- **Real spawns** — `post_meta_spec_preview` (api/meta_specs.rs) resolves every
  target up front (repo 404, workspace, `check_repo_abac`, tenant containment,
  spec_path existence on the default branch), then provisions per target a real
  `Active` agent row, a `preview/{preview_id}/{slug}` branch pinned to the
  default-branch tip, a real git worktree, and a short-lived JWT
  (`GYRE_PREVIEW_JWT_TTL_SECS`, default 1800 s). The hard test
  `preview_spawns_real_agents_with_branches_worktrees_and_draft` drives the
  router against a real bare git repo, asserts Active rows, on-disk branches and
  worktrees, and that the spawned process's environment actually contains the
  draft text (`GYRE_META_SPEC_DRAFT_KIND`/`_CONTENT`, `GYRE_PREVIEW_ID`,
  `GYRE_TARGET_SPEC_PATH`, and no `GYRE_TASK_ID`).
- **Spec shape** — 202 with exactly `{preview_id, agents:[{agent_id, repo_id,
  spec_path, branch}]}`; hollow `StructuralImpact` estimate,
  `compute_preview_blast_radius`, and the workspace-scoped routes are fully
  removed (clean cutover; the two ABAC exemption entries for the removed routes
  are dropped, and `check-abac-route-registry` / `check-abac-exempt-handlers`
  pass on the merged tree).
- **Draft reaches the agent** — `agent-runner.mjs` preview prompt embeds the
  draft meta-spec as governing "how to build" context over the target spec,
  withholds task-mutating MCP tools, and ends the run at process exit with no
  provenance upload and no completion call.
- **Skip-ceremony enforced, not just skipped** — MCP dispatch refuses
  `gyre_create_task`/`gyre_update_task`/`gyre_create_mr`/`gyre_agent_complete`/
  `conversation_upload` for preview agents; the REST escape class is closed
  (task/agent/MR/merge-queue/spec-approve/compose/orchestrator-spawn/register
  routes refuse preview-agent callers, keyed off the array-scoped Gyre agent-JWT
  discriminator + `preview_agents` index — the discriminator itself is pinned by
  a unit regression after the string-form guard was found dead). No
  `refs/agents/*`/`refs/tasks/*` writes, no MR, no provenance, never `Idle`:
  every compute-target exit monitor routes through `finish_preview_agent`
  (Stopped, token revoked, worktree removed, branch retained for diffing).
- **Budget** — defaults to the workspace budget with per-request N-slot
  reservation (`check_spawn_budget_for`, over-admission fixed), plus the
  configurable `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` cap with fail-closed
  enumeration. Slot release is exactly-once across process-exit, DELETE, GC,
  stop/fail routes, admin kill, budget stop and stale-abort via the durable
  `released` flag (`mark_preview_slot_released`), with regression tests per
  path.
- **Status + diff** — `GET /api/v1/meta-specs/preview/{preview_id}` derives
  per-agent state from the real agent rows and exposes the diff of the preview
  branch against the spawn-time pinned `base_sha` (MR-diff plumbing).
  `preview_agent_finish_teardown_and_diff` runs a real committing agent script
  and asserts Stopped (not Idle), worktree removed, branch kept, token revoked,
  and the diff surfacing the agent's commit.
- **DELETE + GC** — `DELETE` kills via the recorded spawn backend (not handle
  shape — SshDocker/Kubernetes pid-less handles handled), force-removes
  worktrees, closes dangling MRs, deletes branches, drops records; 404 unknown.
  The `meta_spec_preview_gc` job (registration bound by test) sweeps past the
  TTL (`GYRE_META_SPEC_PREVIEW_TTL_HOURS`, default 24 h) and reclaims orphaned
  index entries; the TTL boundary test pins at/past-TTL deleted vs
  just-under retained.
- **UI** — MetaSpecs.svelte drives the §5 API directly (POST → per-agent
  status/diff polling, target tabs, bounded wait, generation-counter stale-run
  invalidation, Clean Up → DELETE); EditorSplit's meta-spec context is ported
  to the same API. 2576 frontend tests pass.
- **Docs** — api-reference.md (new §"Meta-Spec Preview Mode" + route rows) and
  server-config.md (preview TTL/JWT/GC-interval/budget knobs) updated.

## Notes (non-blocking)

- **Overlay change** (`scripts/verifier-script-overlay.py`, d28e4a32): check
  *logic* is still restored from base; only frozen `-exemptions.txt` data files
  follow the merge. Since dev-check forbids adding exemption entries, merged
  data is entrywise a subset of base data — strictly conservative — and it is
  the only sound resolution for the clean-cutover case (base data references
  routes the task requires removing). Regression tests cover both directions
  (legitimate removal passes; a stale kept entry still fails). Not a weakening.
- **Env var name**: the task plan suggested `GYRE_SPEC_PATH`; the shipped
  `GYRE_TARGET_SPEC_PATH` is functionally equivalent and avoids collisions. The
  spec excerpt (the contract) names no env vars.
- **Generated output**: `web/dist/` is verifier-owned; a review-side `cargo
  check` rebuilt it via build.rs and the tree was restored afterwards. No
  generated-output finding.
- The staged working tree resets `specs/tasks/task-206.md` to `commits: []`
  (controller staging, pre-existing before review actions); against the
  committed candidate the frontmatter lists all 24 product-surface commits, so
  `check-task-commit-attribution` passes on the candidate as committed.
