---
title: "Implement meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup"
spec_ref: "meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop"
commits: ["8212fa5d945070afac1e859a96e1caae27af950c", "05c06fde1c501cd1f9f5ab5b729c1ec2a1ba2860", "cc3080ff119b5084d507330e038aafb30ee5a0d5", "69488298e887e8c4c76ea5a48f3ce01c10d175c6", "c33a86e2ce9c93de50a97ff3e7644a861cef17d6", "1622b73cdf7e36d32299a4010462b9b1c046eab9", "c8f9fed9abef048df4abf83d4a86e1407a17b9db", "001ec52dd65e890637751d40ecb1a64743a4934d", "a31a731a262c62f7c85202cbca727fe9d9633646", "2c1ddfe60be7f19c843d833e7eead308c3585c8f"]
---

## Spec Excerpt

From `meta-spec-reconciliation.md` §5 — Preview Mode: The Fast Iteration Loop:

> **Preview mode strips all ceremony from the Ralph loop.** Same agent, same spec, same repo -- but no gates, no MR, no merge queue, no provenance recording. Just: spawn an agent with a draft meta-spec, point it at a real spec in a real repo, see what it produces. Throwaway branch, garbage-collected after review.

```
POST /api/v1/meta-specs/preview
{
  "draft": {
    "kind": "meta:persona",
    "content": "<full draft persona text -- not yet committed>"
  },
  "targets": [
    { "repo_id": "<uuid>", "spec_path": "specs/system/search.md" },
    { "repo_id": "<uuid>", "spec_path": "specs/system/identity.md" }
  ]
}

// Response 202
{
  "preview_id": "<uuid>",
  "agents": [
    { "agent_id": "<uuid>", "repo_id": "<uuid>", "spec_path": "...", "branch": "preview/<preview_id>/search" },
    { "agent_id": "<uuid>", "repo_id": "<uuid>", "spec_path": "...", "branch": "preview/<preview_id>/identity" }
  ]
}
```

> The draft doesn't need to be committed or approved. It's ephemeral -- the human is editing inline, hitting preview, seeing output. Multiple targets run in parallel.

Skipped in preview mode: spec approval check, quality gates, MR creation, merge queue, provenance recording. Budget accounting: separate preview budget (configurable, defaults to workspace budget). Token revocation: normal, preview agent tokens short-lived.

Preview cleanup (`preview/{preview_id}/*` branches):
- Auto-deleted after 24 hours (configurable)
- Manually deletable via `DELETE /api/v1/meta-specs/preview/{preview_id}`
- No refs, no provenance, no audit trail (scratch work)
- Preview agents are killed on completion (no idle state)

Iteration cycle step 4: "UI shows diff: existing code vs. new code produced under draft". The diff viewer updates as each preview agent completes; multiple targets show as tabs.

## Current State (verified 2026-09-29)

`crates/gyre-server/src/api/meta_specs.rs:314` (`post_meta_spec_preview`) is **hollow**: it spawns NO agents, marks every spec "complete" immediately, stores a `PreviewRecord` in kv_store, and returns a structural blast-radius estimate. The code comment at meta_specs.rs:365-368 admits agent-based preview runs are unimplemented. Routes currently registered (mod.rs:652-662):

- `POST /api/v1/workspaces/:id/meta-specs/preview` → `post_meta_spec_preview` (hollow)
- `GET /api/v1/workspaces/:id/meta-specs/preview/:preview_id` → `get_meta_spec_preview_status`

These are workspace-scoped; the spec specifies the global routes `POST /api/v1/meta-specs/preview` and `DELETE /api/v1/meta-specs/preview/{preview_id}`. This task implements the spec-conformant routes (spec is the contract).

## Implementation Plan

1. **Rework the preview endpoint into a real agent spawn path** (`crates/gyre-server/src/api/meta_specs.rs`):
   - Register `POST /api/v1/meta-specs/preview` and `DELETE /api/v1/meta-specs/preview/:preview_id` in `crates/gyre-server/src/api/mod.rs` (confirmed: neither path exists there today; the workspace-scoped routes at mod.rs:652-662 are the ones to replace).
   - New request shape per spec: `{ draft: { kind, content }, targets: [{ repo_id, spec_path }] }`. Replace `PreviewMetaSpecRequest`/`PreviewResponse` — clean cutover, remove the hollow structural-impact fields from the response; keep blast radius out of this endpoint (it belongs to §4's `/api/v1/meta-specs/{path}/blast-radius`).
   - Auth: Admin or Developer role (preview runs agents against real repos — keep the existing role check).
   - For each target: verify the repo exists and the caller has repo access (reuse `crate::abac::check_repo_abac`), verify the spec_path exists in the repo.
   - For each target spawn one preview agent in parallel on branch `preview/{preview_id}/{spec_slug}` where spec_slug is the spec file stem (e.g. `specs/system/search.md` → `search`; sanitize collisions by suffixing the parent dir if two targets share a stem).
   - Reuse the spawn machinery from `crates/gyre-server/src/api/spawn.rs` (`spawn_agent` handler internals): agent record (Active), pre-minted JWT, git worktree via `state.git_ops.create_worktree`, worktree recorded via `state.worktrees.create`. Do NOT reuse the parts preview skips: no task assignment requirement (preview agents are not task-driven — model them like interrogation agents: `agent_type: Some("preview")` with the task-type bypass), no spec-approval gate, no quality gates, no MR, no provenance recording, no refs/agents/*/refs/tasks ref writes.
   - Inject the draft into the agent environment: `GYRE_META_SPEC_DRAFT_KIND` + `GYRE_META_SPEC_DRAFT_CONTENT` env vars alongside the existing `GYRE_*` injection block (spawn.rs:594-601), plus `GYRE_SPEC_PATH` for the target spec. The entrypoint/prompt machinery must surface the draft meta-spec to the agent as governing "how to build" context — the agent implements the existing spec under the draft meta-spec.
   - Budget: apply the existing `super::budget::check_spawn_budget` per target repo's workspace (defaults to workspace budget). Add a configurable separate preview budget knob (server config, e.g. `GYRE_PREVIEW_BUDGET_...` following `docs/server-config.md` conventions) that, when set, caps preview spawns independently.
   - Response 202: `{ preview_id, agents: [{ agent_id, repo_id, spec_path, branch }] }` exactly per spec.
   - Persist a `PreviewRecord` in kv_store (`meta_spec_previews` table, as today) mapping preview_id → agents, branches, created_at, state.

2. **Extend preview status with real state and diffs**:
   - Keep a GET status route (move to spec-consistent `GET /api/v1/meta-specs/preview/:preview_id`): per-agent state derived from the real agent records (`state.agents.find_by_id` → Active/Completed/Failed), not a hardcoded "complete".
   - When a preview agent is completed, compute the diff of `preview/{preview_id}/{slug}` against its merge-base with the default branch via the git adapter (`state.git_ops` — use the same diff plumbing the MR diff endpoint uses) and expose it in the status response as `{ spec_path, diff }` per agent. The diff is what the UI iteration cycle (step 4) consumes.

3. **Preview completion semantics**:
   - Preview agents are killed on completion — no idle state. Hook the existing agent-complete path (or the stale-agent detector) so that when a preview agent's run ends, its worktree is removed and only the preview branch (with the produced commits) remains for diffing. Do not leave the agent Active.

4. **Manual cleanup endpoint**:
   - `DELETE /api/v1/meta-specs/preview/:preview_id`: look up the record, kill any still-running preview agents, delete every `preview/{preview_id}/*` branch and its worktree from each target repo, remove the kv_store record. 404 when the preview_id is unknown.

5. **Automatic GC**:
   - Register a background job in `crates/gyre-server/src/jobs.rs` (follow the merge-processor/stale-agent-detector registration pattern): `meta_spec_preview_gc`, default interval 1 hour.
   - Deletes preview branches (and kills agents) for previews older than the TTL — default 24 hours, configurable via server config. No refs, no provenance, no audit trail: kv records are deleted too.

6. **Clean cutover of the hollow implementation**:
   - Remove `compute_preview_blast_radius`, `StructuralImpact`, and the workspace-scoped preview routes from mod.rs once the new routes are live. The `PreviewBlastRadius`-style data stays where it belongs: §4's blast-radius endpoint.
   - Update `docs/api-reference.md` for the new/changed routes and `docs/server-config.md` for the new config knobs.

7. **Tests** (hard tests that fail when the behavior breaks — no self-confirming stubs):
   - POST creates real agent records (queryable via `state.agents`) with Active status, one per target, on `preview/{preview_id}/{slug}` branches with real worktrees on disk.
   - Draft content is actually injected: assert the spawn env/prompt context contains the draft text (not just that a field was set).
   - Response matches the spec shape: 202, preview_id, agents array with agent_id/repo_id/spec_path/branch.
   - Read-only role → 403; unknown repo_id → 404; caller without repo access → 403.
   - Preview agents produce no MR, no provenance record, no task assignment.
   - DELETE removes branches + worktrees + kv record; GET status afterwards → 404.
   - GC job deletes only previews older than the TTL (boundary: at/past TTL deleted, just-under retained).
   - Status reflects real agent state transitions and includes the diff once the agent completes.

## Acceptance Criteria

- [ ] `POST /api/v1/meta-specs/preview` spawns one real agent per target on `preview/{preview_id}/{slug}` branches — agent records, worktrees, and branches verifiably exist
- [ ] Draft meta-spec content reaches the agent (env/prompt), and the agent's job is to implement the target spec under the draft
- [ ] 202 response matches the spec shape exactly (preview_id + agents array)
- [ ] Preview agents skip gates, MR, merge queue, provenance, and task assignment; budget is enforced (workspace default, separate configurable preview budget)
- [ ] GET status reports real per-agent state and the produced diff after completion
- [ ] Preview agents do not linger in Active/idle state after completion
- [ ] `DELETE /api/v1/meta-specs/preview/{preview_id}` kills agents and deletes branches/worktrees/record
- [ ] Background GC deletes previews past the configurable TTL (default 24h)
- [ ] Hollow structural-impact preview implementation and workspace-scoped routes are fully removed (clean cutover)
- [ ] `docs/api-reference.md` and `docs/server-config.md` updated
- [ ] Tests prove spawn, draft injection, authz, skip-ceremony, DELETE, GC TTL boundary, and diff exposure

## Agent Instructions

- Read `specs/system/meta-spec-reconciliation.md` §5 (lines 151-233) in full — the endpoint shape, skip-table, iteration cycle, and cleanup rules are the contract.
- Read `crates/gyre-server/src/api/meta_specs.rs` (current hollow implementation, lines ~310-537) — replace it, don't extend it.
- Read `crates/gyre-server/src/api/spawn.rs` (`spawn_agent`, lines ~260-700) for the real spawn path: agent record creation, JWT minting, worktree creation, container env injection, budget check. Interrogation agents (lines ~81-247, 389-434) show the pattern for non-task-driven agents with special handling.
- Read `crates/gyre-server/src/api/mod.rs` around lines 647-662 for route registration; register the new routes there and remove the old ones.
- Read `crates/gyre-server/src/jobs.rs` for background-job registration (see how the merge processor or stale agent detector registers).
- Read `crates/gyre-server/src/api/mr.rs` (or wherever the MR diff endpoint lives) for the git diff plumbing to reuse for preview diffs.
- Read `docs/server-config.md` for config-knob conventions before adding the preview budget and GC TTL settings.
- Follow conventional commits; run `cargo test -p gyre-server` and `bash scripts/check-arch.sh` before completing.

## Shipped

Preview mode is a real agent run path, not an estimate (meta-spec-reconciliation.md §5):

- **Spawn** — `POST /api/v1/meta-specs/preview` (Admin/Developer; global route, per-spec) validates every target up front (repo exists, caller passes repo ABAC + tenant containment, spec_path exists on the default branch), then provisions per target a real `Active` agent row, a `preview/{preview_id}/{slug}` branch pinned to the default-branch tip, a real git worktree, and a short-lived preview JWT (`GYRE_PREVIEW_JWT_TTL_SECS`, default 1800 s). Response is exactly the spec shape: `202 {preview_id, agents:[{agent_id, repo_id, spec_path, branch}]}`. Provisioning is all-or-nothing per request.
- **Draft delivery** — the draft travels out-of-band in the agent process environment (`GYRE_PREVIEW_ID`, `GYRE_META_SPEC_DRAFT_KIND`, `GYRE_META_SPEC_DRAFT_CONTENT`, `GYRE_TARGET_SPEC_PATH`; `GYRE_TASK_ID` deliberately absent). `docker/gyre-agent/agent-runner.mjs` reads those vars and builds the prompt itself: draft meta-spec as governing "how to build" context over the target spec, task-mutating MCP tools withheld, push to the preview branch as the entire deliverable.
- **Skip-ceremony enforced, not just skipped** — the MCP dispatch refuses task/MR/provenance mutations for preview agents, and the REST surface does too: `POST /api/v1/agents/{id}/complete`, `POST /api/v1/merge-requests`, and `POST /api/v1/merge-queue/enqueue` return 403 for preview-agent callers. No spec-approval gate, no quality gates, no MR, no merge queue, no provenance, no `refs/agents/*`/`refs/tasks/*` writes, never `Idle`. Budget: workspace budget by default, `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` caps preview spawns independently when set (429 on overflow).
- **Completion** — every compute-target monitor routes exits through `spawn::on_agent_process_exit` → `finish_preview_agent`: token revoked, worktree removed, budget slot released (idempotent), agent row lands `Stopped` — never `Idle`. The branch survives for diffing.
- **Status + diff** — `GET /api/v1/meta-specs/preview/{preview_id}` reports per-agent state derived from the real agent rows and the produced `diff` (preview branch vs. pinned base sha, same git plumbing as MR diffs) once the branch exists.
- **Cleanup** — `DELETE /api/v1/meta-specs/preview/{preview_id}` kills agents, force-removes worktrees, closes open MRs on `preview/{id}/*` branches, deletes every preview branch, revokes tokens, drops the kv records; 404 on unknown id. Background job `meta_spec_preview_gc` (interval `GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS`, default 1 h; TTL `GYRE_META_SPEC_PREVIEW_TTL_HOURS`, default 24 h) tears down expired runs and reclaims orphaned agent entries. No refs, no provenance, no audit trail.
- **Clean cutover** — the hollow structural-impact preview and the workspace-scoped routes are gone; docs updated (`docs/api-reference.md`, `docs/server-config.md`, `docker/gyre-agent/README.md`).

### Repair rounds

- **Round 01195dea (69488298)** — the preview launch's started check guessed from the handle's shape (`pid.is_some()`), but SshDocker and Kubernetes handles carry `pid: None` by design: a successful remote launch was always reported "process never started". `dispatch_agent_launch`'s own `launched` flag is now propagated through `launch_taskless_agent_process` and the preview check reads it; `mark_preview_agent_failed` kills through the recorded handle/backend pair before reclaiming the row/token/worktree. Also collapsed a duplicated paragraph in `docs/api-reference.md`. New regression tests: `preview_launch_on_ssh_docker_target_is_not_failed`, `preview_launch_on_kubernetes_target_is_not_failed`, `mark_preview_agent_failed_kills_registered_process`.
- **Round f0e8b9dc (cc3080ff)** — three test regressions from the preview-agent auth extractors, all test-side (no production behavior changed): the merge_queue.rs test module's enqueue probes now send `Bearer test-token` (the `AuthenticatedAgent` extractor on `enqueue` answered 401 before the handler); rbac.rs sub-case 2 retargeted from the removed workspace-scoped preview route to the global `POST /api/v1/meta-specs/preview` with a well-formed body (Json extraction must succeed so the role guard is the surface that fires) and the expected route string updated; rbac.rs sub-case 5's MR seed now sends `Bearer test-token` (`create_mr` gained the extractor). Merge resolution of base 9f446d0 dropped the workspace-scoped preview exemption lines and the now-stale `orchestrator/spawn` exemption (resolver covers it) — the frozen exemption file shrank, never grew.
- **Round 0d041435 (05c06fde)** — the verification round's clippy and whitespace findings: the two env-mutating test helpers (`with_agent_command`, `with_path_prepend`) carry documented `#[allow(clippy::await_holding_lock)]` (the lock must span the awaited body; the tests are serialized on it process-wide), the single-use `format!` in the `agent-commit.sh` fixture is collapsed to a plain literal, the `needless_borrow` in `launch_taskless_agent_process` is removed, and the trailing blank line in `docs/server-config.md` is gone. `cargo clippy -p gyre-server --lib --tests` finishes with zero warnings on any line this branch introduces; `cargo fmt -- --check` clean; every mechanical check script passes.
- **Round 408f352b (8212fa5d)** — three independent-review findings on candidate 07d00fd0: (1) the `create_worktree` failure rollback in `provision_preview_agent` passed `released:false`, so it decremented workspace and tenant-global budget counters that were never incremented (the increment runs only after every provisioning step succeeds) — permanently biasing budget enforcement down one slot per occurrence; the path now uses the shared `preview_agent_ref` helper with `partial.released = true`, matching the sibling early-failure paths, and the related `state.agents.create` failure no longer orphans the just-created preview branch (deleted inline, since the GC orphan pass iterates the preview_agents kv index and would never see it). New regression tests: `preview_worktree_failure_releases_no_untaken_budget_slot` (pre-incremented slots stay at 1 through a forced worktree failure) and `preview_agent_row_failure_deletes_orphaned_branch` (new `CreateFailingAgents` wrapper). (2) `MetaSpecs.svelte` `handleWsChatMessage` called a nonexistent `api.specsAssistGlobal` — the swallowed TypeError meant the §9 workspace-scope chat always fell back to the canned local suggestion; it now calls `api.metaSpecsAssist(workspaceId, { instruction, draft_content, meta_spec_id })`, the phantom test mock is removed, and a wire-call test asserts the exact call shape against a real SSE `Response`. (3) Coverage row 9 (§5) flipped `task-assigned` → `implemented`; `SUMMARY.md` regenerated mechanically. Evidence: `/tmp/stage/review-evidence/repair-round.txt` + `clippy-gyre-server-lib-tests.log`.

### Test evidence

In-sandbox on the round-408f352b head (evidence at `/tmp/stage/review-evidence/repair-round.txt`):

- `cargo check -p gyre-server --lib` and `cargo check -p gyre-server --lib --tests` — both clean (the two new tests and the `CreateFailingAgents` wrapper type-check).
- `cargo clippy -p gyre-server --lib --tests` — zero warnings on `meta_specs.rs` (full log retained; every remaining warning is pre-existing on base and outside this branch's diff hunks); `cargo fmt -p gyre-server -- --check` clean; `bash scripts/check-arch.sh` and `bash scripts/check-abac-route-registry.sh` passed.
- `npx vitest run src/__tests__/MetaSpecs.test.js` (web/, lockfile-installed node_modules) — 111/111 passed, including the new workspace-assist wire-call test; `InlineChat.test.js` + `EditorSplit.test.js` — 70/70 passed (shared chat surfaces).
- The build wrapper blocks gyre-server test codegen in this stage, so execution is deferred to the independent verifier. Required probes:
  - `cargo test -p gyre-server --lib -- preview_` (full preview suite: spawn/draft/authz/skip-ceremony/DELETE/GC-TTL/diff, compute-backend launch regressions, budget cap, persist/worktree/agent-row failure rollbacks — incl. this round's `preview_worktree_failure_releases_no_untaken_budget_slot` and `preview_agent_row_failure_deletes_orphaned_branch`)
  - `cargo test -p gyre-server --lib -- merge_queue` (authorized enqueue probes)
  - `cargo test -p gyre-server --lib -- uninstrumented_role_guards` (retargeted preview guard + authorized MR seed)
  - `cd web && npm ci && npm test`

Sandbox limitation: TCP listener probes are unsupported here (capabilities.json), so no live HTTP/browser verification; the REST surface is exercised through the production `build_router` via `tower::ServiceExt::oneshot` against real git + real processes, and exact-head GitHub CI checks remain mandatory after review.
