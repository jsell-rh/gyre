# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Independent review agent (gateway/rits/zai-org/glm-5-3)
**Round:** R1 (candidate after verification-repair rounds through ac8c3061)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** 9f998fe80f3485e8d7f544a11269fa5bd86b1766
**Candidate reviewed:** ac8c30619492975a682844d77413c36f8409a1cd
**Status:** complete — approved

---

## Verdict

Approved. The preview mode is a real agent run path, not an estimate: every
acceptance criterion is backed by production code and hard tests, the two
documented deviations from the task plan are mechanically forced and
strictly improve security, and all static gates plus type-checking pass on
both the candidate and the exact verify-merge construction. Evidence and
probe logs: `/tmp/stage/review-evidence/` (see README.md there).

## What was independently verified

### Real spawn path (AC1–AC3)

- `POST /api/v1/meta-specs/preview` (mod.rs:941-947, resolver-mapped in
  abac_middleware.rs:784-788) validates every target up front (repo exists,
  `check_repo_abac`, tenant containment, spec_path readable on the default
  branch), then provisions per target a real `Active` agent row scoped to
  the repo's workspace, a `preview/{preview_id}/{slug}` branch pinned to the
  default-branch tip (`git_refs::resolve_ref`), a real git worktree, and a
  short-lived JWT (`GYRE_PREVIEW_JWT_TTL_SECS`, default 1800 s), and launches
  a real process through the platform's single dispatch
  (`launch_taskless_agent_process` → `dispatch_agent_launch`). Response is
  the spec shape 202 `{preview_id, agents:[{agent_id, repo_id, spec_path,
  branch}]}`; stem collisions are disambiguated by the qualified path
  (`preview_branch_name`). The hollow structural-impact implementation
  (`compute_preview_blast_radius`/`StructuralImpact`) is fully gone.
- The draft reaches the agent for real: `GYRE_PREVIEW_ID`,
  `GYRE_META_SPEC_DRAFT_KIND`, `GYRE_META_SPEC_DRAFT_CONTENT`,
  `GYRE_TARGET_SPEC_PATH` travel in the container env (merged before the
  server-owned identity vars so they cannot shadow credentials), and
  `docker/gyre-agent/agent-runner.mjs` builds the preview prompt with the
  draft meta-spec as the governing "how to build" context over the target
  spec, withholds the task-mutating MCP tools, and never uploads a
  conversation or calls complete. The spawn test dumps the actual process
  environment and asserts the draft text is present in it — not a field-set
  assertion.

### Skip-ceremony enforced, not just skipped (AC4)

- No task assignment (`task_id: None` everywhere; test asserts
  `current_task_id.is_none()` and an empty task ledger), no gates, no MR
  (test asserts empty MR list; teardown additionally closes any MR that
  somehow points at a preview branch), no merge queue, no provenance (test
  asserts empty `agent_provenance` kv and zero `refs/agents/*` /
  `refs/tasks/*` writes).
- Escape-route closure: the MCP dispatch refuses
  create/update-task, create-MR, complete and conversation-upload for
  preview agents, and the REST twin handlers (task create/update/transition,
  agents complete/stop/fail/status, MR creation, merge-queue cancel, commit
  provenance recording, compose, orchestrator/workspace spawn) refuse
  preview-agent callers via `caller_is_preview_agent` — including the
  array-scoped JWT claim shape real minted tokens carry (pinned by a direct
  unit test, task-206 round 9ee3491d's dead-guard class).
- Budget: defaults to the target workspaces' own budget, reserved per
  workspace up front for multi-target requests
  (`check_spawn_budget_for`), with a separate configurable cap
  (`GYRE_PREVIEW_BUDGET_MAX_CONCURRENT`, 429 on overflow, fail-closed when
  the running-agent enumeration errors).

### Real state, diffs, completion semantics (AC5–AC6)

- `GET /api/v1/meta-specs/preview/{preview_id}` derives per-agent state from
  the real agent rows (Active→running, terminal+released→complete, etc.) and
  exposes the produced diff of the preview branch against the pinned
  base SHA through the same git plumbing the MR diff uses. The completion
  test drives a real agent script that commits and pushes to the preview
  branch, then asserts the diff surfaces the changed spec file.
- Every compute-target monitor exit routes through
  `on_agent_process_exit` → `finish_preview_agent`: token revoked, worktree
  force-removed, budget slot released (exactly-once across the generic
  terminal paths — stop/fail routes, admin kill, budget stop, stale
  detector — via the durable `released` flag), row `Stopped`, never `Idle`.
  Inbox bulk-ack on both terminal paths.

### DELETE and GC (AC7–AC8)

- `DELETE /api/v1/meta-specs/preview/{preview_id}` kills runtimes through
  the recorded spawn backend (not handle-shape guessing — pinned by the
  kill-routing test), stops rows, acks inboxes, removes worktrees, closes
  dangling MRs, deletes every `preview/{preview_id}/*` branch, revokes
  tokens, releases slots, drops the kv records; 404 on unknown ids; the
  teardown test observes the real process PID die.
- `meta_spec_preview_gc` registered in jobs.rs (interval
  `GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS`, default 1 h; registration bound
  by test) with TTL `GYRE_META_SPEC_PREVIEW_TTL_HOURS` default 24 h; the GC
  test ages one run past the TTL and keeps a fresh one (boundary), and the
  orphan pass reclaims agent index entries whose run record is gone.

### Authz and rollback

- Role gate (Admin/Developer, RBAC-audited denials), per-repo ABAC, tenant
  containment, and — for the workspace-scoped surface — workspace
  containment (target repos must live in the path's workspace, cross-tenant
  workspace 403, wrong-workhouse status/teardown 404). Tests cover
  read-only 403, unknown repo 404, missing spec path 400, ABAC/tenant 403.
- Provision/persist failures roll back everything (agent row, worktree,
  branch, token, index entry, budget slot — with the no-slot-taken guard so
  rollback never decrements a counter that was never incremented), each
  path covered by a dedicated test.

### Docs, UI, coverage (AC9–AC11)

- `docs/api-reference.md` documents all six routes with shapes and error
  codes; `docs/server-config.md` documents all four knobs;
  `docker/gyre-agent/README.md` documents the preview env contract.
- The web UI implements the §5 iteration loop: inline editor with
  pending-changes diff, target spec selector, Preview/Cancel/Clear actions
  (Clear → DELETE), per-target diff tabs with keyboard navigation, and
  status polling with run-generation guards against stale ticks. Full web
  suite re-run independently: 127 files, 2576 passed, 0 failures.
- Coverage row 9 flipped task-assigned → implemented with accurate
  evidence (one stale closing clause — see observations).

## Deviations from the task plan (both verified justified)

1. **Workspace-scoped routes re-registered, not removed.** Reproduced the
   forcing condition (`route-removal-phase1-probe.log`): the verifier's
   phase-one overlay restores the BASE tree's frozen
   `abac-route-registry-exemptions.txt`, which still lists both
   workspace-scoped preview paths, and `check-abac-exempt-handlers.sh`
   hard-fails when an exempt pattern matches no route registered in
   api/mod.rs — removal is mechanically unshippable. The candidate followed
   the check's sanctioned lifecycle instead: routes stay registered, gained
   resolver `RouteResourceMapping` entries (the base ran them with NO policy
   evaluation), per-handler tenant/workspace containment, the exemption
   entries were deleted (file shrank 46→44 entries), and the hollow
   implementation those routes used to serve stayed removed. Phase one and
   phase two both pass on the exact candidate. Strict security improvement;
   documented in specs/tasks/task-206.md and docs/api-reference.md.
2. **`scripts/verifier-script-overlay.py` changed** so frozen
   `*-exemptions.txt` data follows the merge in phase one (check logic still
   restored from base). Safety verified: entries only suppress findings,
   `dev-check.sh` rejects any added exemption entry between HEAD^1 and HEAD
   at entry level (blocking count-neutral replacement attacks), and the
   final candidate passes phase one with the base's own frozen data, so it
   does not depend on the change. Semantics pinned by new tests in
   `scripts/test_dev_checks.py` (10/10 pass).

## Probe evidence

- Phase one (candidate + base scripts incl. frozen exemptions): all gates
  exit 0. Phase two (candidate scripts): all 15 invariant checks + registry
  + exempt handlers + arch + attribution exit 0.
- `cargo check -p gyre-server --lib --tests`: Finished on the candidate
  (8m32s) and on the verify-merge tree (8m19s).
- The candidate branched from 00669baa, not the assigned base; the
  verify-merge merge(9f998fe8, ac8c3061) resolves with no conflicts,
  preserves task-269's terminal worktree cleanup and combines it with the
  preview slot-release bookkeeping, and passes the same gates.
- gyre-server test execution reserved for the verifier (4 GiB budget):
  `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib -- preview_`, the full
  lib suite, `cargo test --all`, and `cd web && npm ci && npm test` (web
  suite already re-run green here).

## Non-blocking observations

1. `specs/coverage/system/meta-spec-reconciliation.md` row 9's closing
   clause "workspace-scoped routes removed" predates the final repair
   (ff9d4e63) that re-registered those paths resolver-enforced; the
   authoritative task record and api-reference describe the final state
   accurately. Stale clause in an audit note; the row status is accurate.
2. Neither observation affects the production code contract.
