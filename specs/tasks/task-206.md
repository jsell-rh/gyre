---
title: "Implement meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup"
spec_ref: "meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop"
depends_on: []
progress: not-started
coverage_sections:
  - "meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop"
commits: []
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
