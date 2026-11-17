# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Review agent (pi runtime)
**Round:** R1 (merge-round head `7cc0ca33` → recorded candidate `e3f727e1aeb990bc74113d7748e4bfa80283b0d0`)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** 1f86b0064bdd4e3d14cb1e48bdb61270804fcf42
**Status:** complete — approved (no blocking findings)

---

## Verdict

**approved.** The hollow structural-impact preview is fully cut over to a real
agent run path, the skip-ceremony invariants are enforced (not just skipped) on
both the MCP and REST surfaces, containment is derived from repo/workspace
records rather than the request body, and the test suite is genuinely hard
(real git, real processes, positive controls, handler-text-pinned denials).
All probes runnable in this sandbox pass; the gyre-server test suite is
compile-verified here and deferred to the independent verifier (required
probes recorded in `/tmp/stage/review-evidence/summary.md`).

## Independent evidence (this sandbox)

- `cargo check -p gyre-server --lib --tests` — clean (full test body type-checks).
- `cargo test -p gyre-adapters --lib` — 576 passed (incl. the new
  `SshTarget::from_handle` round-trip/refusal tests and git2
  `force_remove_worktree`/`delete_branch` coverage).
- `cd web && npm ci && npm test` — 124 files, 2493 passed, 0 failed
  (MetaSpecs §5 preview suite + EditorSplit 63/63 post-merge-resolution).
- All 18 mechanical invariant scripts pass, including
  `check-abac-route-registry.sh` (exemption file shrank by exactly the two
  removed workspace-scoped preview routes — clean cutover, no grandfathering)
  and `check-task-commit-attribution.sh` (diff file set == listed commits).
- Deferred to verifier (build wrapper blocks server test codegen in review
  stage): `cargo test -p gyre-server --lib -- preview_`, `-- merge_queue`,
  `-- uninstrumented_role_guards`, full lib suite, clippy, `cargo build --all`.

## Acceptance-criterion verification

- **Real spawns** — `post_meta_spec_preview` (meta_specs.rs:832) validates every
  target up front (repo exists → workspace exists → `check_repo_abac` →
  explicit tenant containment → spec_path readable on the default branch),
  then per target provisions a real `Active` agent row, a
  `preview/{preview_id}/{slug}` branch pinned to the default-branch tip
  (base SHA pinned for the diff), a real git worktree, and a short-lived
  JWT; all-or-nothing with rollback on provision/persist failure.
  `preview_spawns_real_agents_with_branches_worktrees_and_draft` drives this
  through the production `api_router` against a real bare repo and a real
  spawned process, asserting rows, on-disk worktrees, branches, and the
  **actual process environment dump** (`GYRE_META_SPEC_DRAFT_KIND/CONTENT`,
  `GYRE_PREVIEW_ID`, `GYRE_TARGET_SPEC_PATH`, and the negative
  `!GYRE_TASK_ID=`).
- **202 shape** — `PreviewResponse{preview_id, agents:[{agent_id, repo_id,
  spec_path, branch}]}` exactly; hollow `StructuralImpact`/`PreviewBlastRadius`
  response fields removed (§4 blast-radius endpoint untouched).
- **Skip-ceremony enforced** — MCP dispatch refuses
  create/update-task/create-MR/agent-complete/conversation-upload for preview
  agents (mcp.rs:4924); the REST surface refuses the same for preview-agent
  callers on `POST /agents/{id}/complete` (spawn.rs:2897),
  `POST /merge-requests`, `POST /merge-queue/enqueue`, `POST/PUT /tasks`.
  Proven with the preview agent's **live minted JWT through `build_router`
  (ABAC middleware + seeded builtin policies)** with admin positive controls.
  No refs/agents/*, refs/tasks/*, MR, task, or provenance writes (asserted on
  the real repo + stores).
- **Budget** — workspace budget by default with an up-front N-slot reservation
  (`check_spawn_budget_for`); `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` caps preview
  spawns independently when set (429). Both tested (overflow 429; multi-target
  vs. workspace `max_concurrent_agents`).
- **Completion semantics** — every exit routes through
  `on_agent_process_exit` → `finish_preview_agent`: token revoked, worktree
  removed, budget slot released, row `Stopped` — never `Idle`. The branch
  survives for diffing. `preview_agent_finish_teardown_and_diff` proves the
  full chain with a real committing script agent and the production monitor.
- **Status + diff** — `GET /api/v1/meta-specs/preview/{id}` derives per-agent
  state from the real agent rows (`released` flag distinguishes
  complete/stopped) and the diff via the same git plumbing as MR diffs,
  against the pinned base SHA.
- **DELETE + GC** — `delete_meta_spec_preview` kills through the recorded
  spawn backend (not handle shape — SshDocker/K8s kills were the pre-fix
  orphan bug, fixed and regression-tested), removes worktrees/branches/records,
  404 on unknown id. `meta_spec_preview_gc` job registered (jobs.rs:705) and
  spawned in main.rs; TTL boundary tested (at-TTL deleted, fresh retained,
  orphan pass reclaims index entries whose run is gone).
- **Clean cutover** — workspace-scoped preview routes and their ABAC
  exemption entries removed; new global routes registered in mod.rs and the
  ABAC resolver; docs (`api-reference.md` incl. a full preview-mode section,
  `server-config.md` four new knobs, `docker/gyre-agent/README.md`) updated.

## Non-blocking observations (recorded for future hardening; no action required)

1. **Budget-release TOCTOU (inherited class).** `release_preview_slot`'s
   read-modify-write of the `released` flag (mutate_preview) can interleave
   when a natural exit races a concurrent DELETE/GC teardown of the same run,
   double-decrementing `active_agents`. This is the same exposure class the
   platform's pre-existing paths already carry (budget-enforcement kill and
   the exit monitor both decrement without a guard at all); the `released`
   flag makes the sequential cases (exit→DELETE, exit→GC) idempotent, which
   is an improvement over baseline. No recompute of `active_agents` from
   agent rows exists platform-wide.
2. **EditorSplit `context="meta-spec"` path is production-dead code.** The only
   production mount is `DetailPanel.svelte:2017` with `context="spec"`; the §5
   UI loop lives in `MetaSpecs.svelte`. The ported EditorSplit §5 branch is
   tested but unreachable, its header comment ("Used by … MetaSpecs") is
   inaccurate, and its poll loop aborts on the first transient error while
   MetaSpecs tolerates three. ui-layout row 13's note is honest about where
   the loop lives. Maintenance hazard only — the live surface is correct.
3. **Spec §5 iteration-cycle step 4 timing.** Per-agent diffs render when the
   whole run completes (tabs per target), while the running state shows live
   per-agent status. The spec sentence "the diff viewer updates as each
   preview agent completes" is realized for status/diff-stats but not for
   line-level diffs mid-run. The task's acceptance checklist does not include
   this item; recorded as a nuance, not a contract break.
4. **Test-tightening opportunities** (gaps, not self-confirmation): no
   agent-runner.mjs test for `previewPrompt` construction (env delivery is
   proven hard; prompt assembly is not), GC boundary one-sided (fresh vs
   at-TTL, not just-under-TTL), MCP positive control uses a non-ceremony
   tool, JWT TTL assertion one-sided, stem-collision slug disambiguation
   untested.
