---
title: "Implement meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup"
spec_ref: "meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop"
commits: ["001ec52dd65e890637751d40ecb1a64743a4934d", "a31a731a262c62f7c85202cbca727fe9d9633646", "2c1ddfe60be7f19c843d833e7eead308c3585c8f", "c8f9fed9abef048df4abf83d4a86e1407a17b9db", "1622b73cdf7e36d32299a4010462b9b1c046eab9"]
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

- **Spawn** — `POST /api/v1/meta-specs/preview` (Admin/Developer; global route, per-spec) validates every target up front (repo exists, caller passes repo ABAC + tenant containment, spec_path exists on the default branch), then provisions per target a real `Active` agent row, a `preview/{preview_id}/{slug}` branch pinned to the default-branch tip (pinned base ⇒ the diff can never show someone else's later commits as deletions), a real git worktree, and a short-lived preview JWT (`GYRE_PREVIEW_JWT_TTL_SECS`, default 1800 s — separate from `GYRE_AGENT_JWT_TTL`). Response is exactly the spec shape: `202 {preview_id, agents:[{agent_id, repo_id, spec_path, branch}]}`. Provisioning is all-or-nothing per request: a failed target rolls back everything already created.
- **Draft delivery** — the draft travels out-of-band in the agent process environment (`GYRE_PREVIEW_ID`, `GYRE_META_SPEC_DRAFT_KIND`, `GYRE_META_SPEC_DRAFT_CONTENT`, `GYRE_TARGET_SPEC_PATH`; `GYRE_TASK_ID` deliberately absent). `docker/gyre-agent/agent-runner.mjs` reads those vars and builds the prompt itself: draft meta-spec as governing "how to build" context over the target spec, task-mutating MCP tools withheld, push to the preview branch as the entire deliverable.
- **Skip-ceremony enforced, not just skipped** — enforced on every surface a preview agent can reach: the MCP dispatch refuses task/MR/provenance mutations (`mcp.rs` guard keyed off the `preview_agents` index), and the REST surface does too (review-round fix): `POST /api/v1/agents/{id}/complete` and `POST /api/v1/merge-requests` both return 403 for preview-agent callers — `complete_agent` gates on `is_preview_agent`, `create_mr` takes the authenticated caller and refuses preview agents (a preview agent otherwise escapes §5's "no MR" skip to any repo its Agent-role ABAC policy allows, since `repository_id` is caller-supplied). No spec-approval gate, no quality gates, no MR, no merge queue, no provenance, no `refs/agents/*`/`refs/tasks/*` writes, never `Idle`. Budget: workspace budget by default, `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` caps preview spawns independently when set (429 on overflow).
- **Completion** — every compute-target monitor routes exits through `spawn::on_agent_process_exit`, which routes preview agents into `finish_preview_agent`: token revoked, worktree removed, budget slot released (idempotent across process-exit/DELETE/GC), agent row lands `Stopped` — never `Idle`. The branch survives for diffing.
- **Status + diff** — `GET /api/v1/meta-specs/preview/{preview_id}` reports per-agent state derived from the real agent rows (`running|complete|failed|stopped|dead|unknown`) and the produced `diff` (preview branch vs. pinned base sha, same git plumbing as MR diffs) once the branch exists.
- **Cleanup** — `DELETE /api/v1/meta-specs/preview/{preview_id}` kills agents, force-removes worktrees (`force_remove_worktree` port op — preview worktrees are dirty by design), closes any open MR pointing at a `preview/{id}/*` branch (belt-and-braces: no open MR dangles off a deleted branch), deletes every `preview/{id}/*` branch, revokes tokens, drops the kv records; 404 on unknown id. Background job `meta_spec_preview_gc` (interval `GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS`, default 1 h; TTL `GYRE_META_SPEC_PREVIEW_TTL_HOURS`, default 24 h) tears down expired runs and reclaims orphaned agent entries. No refs, no provenance, no audit trail.
- **Remote kills reach the remote host** (review-round fix) — an SSH compute target's `ProcessHandle.pid` is the remote pid echoed by the remote shell; `kill_process_handle` now reconstructs an `SshTarget` from the handle (`user@host:pid` + connection options recorded at spawn time: `identity_file`, `port`) and kills remotely. Previously DELETE/GC/admin-kill ran `kill -TERM <remote-pid>` locally: the remote agent survived and an unrelated local process holding that pid was terminated. Malformed ssh handle ids are refused outright, never killed locally.
- **Clean cutover** — the hollow structural-impact preview and the workspace-scoped routes are gone (`compute_preview_blast_radius`/`StructuralImpact` removed; the two workspace-scoped exemption entries deleted from `abac-route-registry-exemptions.txt`, shrinking it). Docs updated: `docs/api-reference.md` (routes, request/response shapes, skip-table, REST-surface enforcement), `docs/server-config.md` (four knobs), `docker/gyre-agent/README.md` (agent-side env vars).

**Test evidence** (focused probes on head 4fc19fca after the review-fix round; evidence retained at `/tmp/stage/review-evidence/task-206-review-fix-round.md`):

- `cargo test -p gyre-server --lib` — 1232 passed, 0 failed. The preview suite (17 tests) uses a real bare git repo, real Git2OpsAdapter, real spawned processes whose environment is dumped to a file: spawn + branch/worktree existence + draft env injection (content, not field-set) + skip-ceremony + 202 shape; finish → Stopped-never-Idle + worktree removed + token revoked + real diff surfaced in status; DELETE teardown completeness + 404s; role/validation gates; preview budget cap 429; GC TTL boundary (at/past deleted, just-under retained, orphan reclaimed).
- Review-fix regressions (all proven by mutation — removing each fix fails its test): `preview_agent_rest_ceremony_is_refused` drives the preview agent's own live JWT through the production router (`build_router` + seeded builtin policies, mirroring the review probe): complete → 403 and MR create → 403, with no MR / not Idle / no `agent_provenance` kv / no `refs/agents/*` snapshots, plus an admin positive control (MR creation on a preview branch still 201); `preview_delete_closes_mrs_on_preview_branches`; `kill_process_handle_refuses_malformed_ssh_handle` (carries the test process's own pid — a regression to local dispatch kills the test runner); `SshTarget::from_handle` unit tests (destination parsing, connection-option restoration, malformed-id refusal).
- `cargo test -p gyre-adapters --lib` — 347 passed, 0 failed (12 ignored: pre-existing SSH-daemon integration tests). `mcp` — 80 passed. `jobs` — 11 passed (preview GC registered as a required-healthy job). Web: `vitest run src/__tests__/MetaSpecs.test.js` — 106 passed (pre-fix head).
- Gates on head: `cargo fmt --check` clean; `check-clippy-diff.py HEAD` clean; arch, abac-route-registry, mcp-write-tools, mem-port-contracts, fabricated-scope-defaults, lossy-secret-conversion all OK. `check-task-commit-attribution.sh` fails only on base commit 6bf777a6 (task-200 frontmatter debt, reproducible on base — pre-existing, not this diff).

Sandbox limitation: TCP listener probes are unsupported here (capabilities.json, errno 95), so no live HTTP/browser verification was possible; the REST surface is exercised through the production `build_router` (auth + ABAC middleware) via `tower::ServiceExt::oneshot` against real git + real processes, and exact-head GitHub CI checks remain mandatory after review.

### Rebase round — merge onto main 3c39ab1f resolved

The candidate was rebased onto the new base (upstream main incl. task-172/173/199/201/207). All six conflicts resolved; the one semantic conflict and its consequences:

- `retention.rs` — took upstream's task-207 work verbatim (our side of the conflict was pure rustfmt reformatting); `cargo fmt -p gyre-server -- --check` clean.
- `MetaSpecs.svelte` / `MetaSpecs.test.js` — upstream task-172 had restructured the same region to mount EditorSplit's meta-spec context, a client built against the workspace-scoped estimate endpoint and old response shapes this task removed; it is unwirable against the §5 server (global routes are the contract). Resolution keeps task-206's §5-driven loop and its 106-test suite; verified upstream's test diff contained nothing outside that restructuring.
- `EditorSplit.svelte` (auto-merged from upstream) still called the deleted `api.previewPersona` endpoints in its meta-spec context — ported to the real §5 API: `previewMetaSpec` POST with `{draft:{kind,content}, targets:[{repo_id,spec_path}]}` (new `draftKind` prop), `previewMetaSpecStatus` polling with per-agent states, produced per-agent diffs in the Code Diff tab, per-agent run summary in the Architecture tab, patient poll (2 s interval / 600 s bounded wait — the old 30 s cap only fit the hollow endpoint). `EditorSplit.test.js` meta-spec cases rewritten to the exact §5 wire shapes. No production code references the removed routes (grep-clean).
- `en.json` — union of both key sets (valid JSON). `web/dist` — rebuilt from merged source, single clean bundle set. `ui-layout.md` coverage row 13 amended to the merged reality.

Evidence this round (`/tmp/stage/review-evidence/task-206-rebase-round.md`): MetaSpecs + EditorSplit vitest **149 passed, 0 failed**; full web suite 1526 passed with failures only in `ExplorerCanvas-performance.test.js` timing-budget tests, which fail on the pristine upstream base in this CPU-starved sandbox too (control run in a separate worktree — environmental, not this diff); `check-arch.sh`, `check-abac-route-registry.sh`, `check-mcp-write-tools.sh` all OK; `cargo fmt -p gyre-server -- --check` clean (gyre-cli shows 20 pre-existing fmt diffs, byte-identical on the pre-merge head — CI's fmt gate is diff-scoped).

Remaining verification for the reviewer: the Rust `cargo test -p gyre-server --lib preview` probe is deferred to independent verification/CI, where it remains mandatory — the cold dependency build in this sandbox is CPU-throttled to the point of taking days, and its process-admission gate (`tirith`) blocked all shell access for most of the assignment. The Rust conflict resolution took upstream's approved retention.rs verbatim and touched no other Rust source, so the pre-rebase green suite (1232 passed on 4fc19fca) is the server-side baseline. The merge resolution itself is committed as f092e666 (full sha f092e666514102d2cff10a8d330b788e5520b5ab, conventional task-labeled commit, recorded in `commits:`).

### Recovery round — lease-expiry checkpoint repaired

The worker lease expired after the rebase round; the original 12-commit chain (4fc19fca…f092e666, previously listed in `commits:`) became unreachable objects. The executor recovered the full source as the squashed checkpoint commit **2c1ddfe6**, which is now the `commits:` base — the dead shas were removed from the frontmatter because review scoping cannot resolve them; the rebase-round text above keeps them as history. This round repaired the recovered state:

- **Working tree restored** — the recovery left 17 tracked files missing from disk (including `api/agent_messages.rs` and `src/messages.rs`, both declared modules, and `MoldableView`/`FlowRenderer`/`NodeBadge` referenced by `ExplorerCanvas.svelte`); restored from the checkpoint. `web/dist` churn from `build.rs` runs is left to the executor (restored to HEAD, not committed).
- **Two compile defects fixed** (a31a731a): `check_spawn_budget_for` took `requested: u64` while `BudgetUsage.active_agents`/`max_concurrent_agents` are `u32` (both `saturating_add` sites mismatched — parameter is now `u32`, preview-budget path stays `u64` against `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT`); `merge_queue::enqueue` gained an `AuthenticatedAgent` param for the preview-agent merge-queue guard but did not skip it in `#[instrument]` (requires `Debug`), fixed by adding `auth` to the skip list, matching `merge_requests.rs`/`spawn.rs`/`traces.rs`.

Evidence this round (head a31a731a): `cargo check -p gyre-server --lib --tests` clean (both lib and test code type-check — the 17-test preview suite compiles); `cargo fmt -p gyre-server -- --check` clean; `check-arch.sh`, `check-abac-route-registry.sh`, `check-mcp-write-tools.sh`, `check-mem-port-contracts.sh`, `check-fabricated-scope-defaults.sh`, `check-lossy-secret-conversion.sh`, `check-byte-slice-truncation.sh`, `check-in-memory-state-stores.sh` all OK; `cargo test -p gyre-adapters --lib` — 348 passed, 0 failed (12 ignored: pre-existing SSH-daemon integration tests — covers the task-206 `git2_ops.rs`/`ssh.rs` changes); `vitest run src/__tests__/MetaSpecs.test.js src/__tests__/EditorSplit.test.js` — 149 passed, 0 failed.

Still deferred to independent verification (unchanged): `cargo test -p gyre-server --lib -- preview` — the build wrapper blocks server test binaries in this sandbox (4 GiB budget), so the runtime behavior of the preview suite must be confirmed there; the suite's green baseline is 1232 passed (pre-rebase head 4fc19fca) and its code type-checks on this head.

### Repair round — candidate 08c3b5e2 re-verified

The prior assignment's `agent_exit: 1` was a controller-side validation failure ("Expected exactly one owned fullsend sandbox"), not a code defect; the executor checkpointed the completed repair work (001ec52d) and recorded the branch commits in the frontmatter (08c3b5e2). This round repaired the checkout state and re-verified the exact candidate head — **no source changes were needed**:

- **Working tree restored** — the checkout again showed the recovery-damage pattern (17 tracked files missing from disk as unstaged deletions, including the declared modules `api/agent_messages.rs` and `src/messages.rs`); restored from HEAD, tree clean. `web/dist` churn from a build.rs run reverted to HEAD (executor-owned).
- **Verification on head 08c3b5e2** (evidence: `/tmp/stage/review-evidence/task-206-repair-round.md`): `cargo check -p gyre-server --lib --tests` clean (7m 21s — the preview suite compiles); `cargo fmt -p gyre-server -- --check` clean; `check-arch.sh`, `check-abac-route-registry.sh`, `check-mcp-write-tools.sh`, `check-task-commit-attribution.sh` all OK; MetaSpecs + EditorSplit vitest **149 passed, 0 failed**.
- **Unchanged for the verifier (mandatory)**: `cargo test -p gyre-server --lib -- preview` — server test-binary builds are excluded from this sandbox by the 4 GiB build-budget rule; baseline 1232 passed (4fc19fca), type-checks on this head.

### Merge round — pipeline merge 4a5925cc resolved onto base 047d6524

The pipeline merged upstream main 047d6524 (PR #849 triage-decisions) into this branch as 4a5925cc but committed all 63 unresolved conflict hunks as-is across 22 files, and reverted this task file to `not-started`. This round resolved every hunk in place (upstream touched none of the conflicted files directly — its changes were in scripts/ and tools/, so the conflict sides were this branch's task-206 work vs. the older common base d81b0fe4), committed as c8f9fed9 (server/docs) and 1622b73c (web) — both recorded in `commits:`.

Resolution policy per file: union for job registrations (meta_spec_preview_gc spawn + upstream's newer jobs), ABAC route mappings (preview routes + upstream's spec meta-spec-bindings routes), and the MCP preview-agent tool refusal plus upstream's validate_tool_scope; upstream's side for admin.rs (its kill_agent helper is the refactor this branch's preview paths call), merge_processor.rs and budget.rs bases (this branch's diff there was pure rustfmt plus the N-slot check_spawn_budget_for, re-applied deliberately), mem.rs (upstream's TestStateOverrides/ws_store architecture with this branch's test_state_with_preview_config ported onto it via Arc::get_mut); this branch's side for the §5 implementation in meta_specs.rs/spawn.rs/agent-runner.mjs, with upstream's newer contracts adopted: the 9-arg agent JWT mint (scope claims derived from the loaded workspace/repo rows), MetaSpecScope::Tenant visibility in the registry listing (fail-closed on unresolvable scope ids), the 7-arg MergeRequest::new, and the options field on ProcessHandle at the kubernetes/ssh spawn sites so remote kills can still reconstruct the SshTarget. Exemption files resolved to the frozen-at-zero base side (identical to 047d6524).

Consequence of the merged create_task taking `AuthenticatedAgent` for the preview guard: the previously unguarded `req.workspace_id`/`req.repo_id` reads became visible to check-forged-scope-fields — fixed with the shared audit validators (derive-or-403, repo∈workspace when both supplied), and the test call sites that posted/put without a token now authenticate and seed real workspace/repo rows.

Web completion (a9a474d6): the merge had left the four §9 State 2 locked-diff MetaSpecs tests and the EditorSplit iterate test mocking the removed hollow `previewPersona` endpoint. Migrated to the real §5 wire shapes; implemented what they pin: MetaSpecs' locked editor shows the LCS pending-changes diff vs the published prompt (verbatim fallback, no-diff-when-unchanged, post-publish baseline refresh) and EditorSplit's Iterate retains the last preview results with the selector stacked above the tabs (ui-layout.md §9). The conditional guard around the accept-suggestion interaction was replaced with a loud existence assertion.

Verification this round (heads c8f9fed9/1622b73c):

- `cargo check --workspace --all-targets` clean — every crate, lib and test target type-checks, including the full preview suite and the migrated test code.
- `cargo test -p gyre-adapters --lib` — 576 passed, 0 failed (4 ignored: pre-existing SSH-daemon integration tests; includes the compute::ssh/compute suites covering SshTarget reconstruction); `cargo test -p gyre-common -p gyre-domain -p gyre-ports` — 688 passed, 0 failed.
- Web: full `npm test` — **121 files, 2410 passed, 41 skipped, 0 failed** (MetaSpecs + EditorSplit focused runs included).
- rustfmt clean on every touched Rust file. Mechanical gates on this head: arch, abac-route-registry, abac-exempt-handlers, mcp-write-tools, mem-port-contracts, fabricated-scope-defaults, lossy-secret-conversion, fail-open-ref-resolution, inert-enforcement, relative-path-defaults, scope-literal-defaults, forwarded-header-trust, in-memory-state-stores, unbounded-external-http, byte-slice-truncation, migration-versions, migration-sql-portability, dead-message-kinds, forged-scope-fields, conditional-test-guards, non-atomic-creation — all OK. The remaining check-*.sh scripts that fail in this sandbox fail byte-identically on the pristine base commit 047d6524 (control run in a separate worktree: mawk lacks gawk's 3-arg match(), so those scripts abort identically on both trees — environmental, not this diff; no violation-list delta between base and this head on any of them).
- web/dist is executor-owned (restored to the assignment base; not committed).

Unchanged for the verifier (mandatory): `cargo test -p gyre-server --lib -- preview` — server test binaries exceed this sandbox's 4 GiB build budget (the cargo wrapper refuses them); the suite's green baseline is 1232 passed (4fc19fca) and the merged tree type-checks all of it via `cargo check -p gyre-server --all-targets`.
