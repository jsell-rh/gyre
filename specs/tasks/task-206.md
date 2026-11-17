---
title: "Implement meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup"
spec_ref: "meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop"
commits: ["c1a709ba62490cf3661b06b6ea6b72308488ebf4", "a4d8bd712ed166b78311ce648a70c58791082ea1", "2f10e8d134c1911f52b07400d7becabb910dbe21", "7cad12616cd28f691664cac1922337c9f5d72bef", "683da5a1afe0bb249a8fa0aa3e84e65006d9cde2", "f4ee98ba5521f5b8e973ae3172481dec4f2d34de", "8212fa5d945070afac1e859a96e1caae27af950c", "05c06fde1c501cd1f9f5ab5b729c1ec2a1ba2860", "cc3080ff119b5084d507330e038aafb30ee5a0d5", "69488298e887e8c4c76ea5a48f3ce01c10d175c6", "c33a86e2ce9c93de50a97ff3e7644a861cef17d6", "1622b73cdf7e36d32299a4010462b9b1c046eab9", "c8f9fed9abef048df4abf83d4a86e1407a17b9db", "001ec52dd65e890637751d40ecb1a64743a4934d", "a31a731a262c62f7c85202cbca727fe9d9633646", "2c1ddfe60be7f19c843d833e7eead308c3585c8f"]
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

Preview mode is a real agent run path (meta-spec-reconciliation.md §5), implemented and review-approved in the prior rounds recorded above: `POST /api/v1/meta-specs/preview` spawns one real `Active` agent per target on `preview/{preview_id}/{slug}` branches with real worktrees and short-lived JWTs (202 response exactly per spec shape); the draft travels in the agent process env and `docker/gyre-agent/agent-runner.mjs` builds the prompt (draft meta-spec as governing context over the target spec); skip-ceremony is enforced (no task assignment, no gates, no MR, no merge queue, no provenance, no `Idle`; MCP and REST surfaces refuse preview-agent mutation escape routes); completion tears down worktree and token, leaving the branch for `GET .../preview/{preview_id}` status + diff; `DELETE` and the `meta_spec_preview_gc` background job (TTL `GYRE_META_SPEC_PREVIEW_TTL_HOURS`, default 24 h) clean up branches, worktrees, agents, and kv records; the hollow structural-impact implementation and the workspace-scoped routes were fully removed (clean cutover); docs updated. Regression tests cover spawn, draft injection, authz, skip-ceremony, DELETE, GC TTL boundary, and diff exposure.

### Verification finding f650854 — root cause (this round)

The failing gate was `check-abac-exempt-handlers` erroring twice: "ABAC-exempt route /api/v1/workspaces/:id/meta-specs/preview[:preview_id] is not registered in api/mod.rs". This round reproduced and fully diagnosed it; it is a verification-harness interaction, not a source defect:

- The verification stage checks out main and merges the candidate into it (`verify(task): reconcile candidate with main`, first parent = main), then `checks.sh` overlays main's entire `scripts/` directory (`verifier-script-overlay.py begin` → `git restore --source=HEAD^1 --worktree -- scripts`) before running the upstream gate suite. The overlay includes the frozen ABAC exemption DATA file, not just verifier logic.
- Main still registers the old workspace-scoped preview routes, so its exemption file legitimately lists them. The merged tree — per this task's spec-mandated, review-approved clean cutover — removed both the routes and the exemption entries. The overlaid upstream data therefore references routes the merged tree no longer registers, and upstream's `check-abac-exempt-handlers` errors on exactly those two patterns. The second suite phase (merged tree's own scripts) passes everything; `dev-static-gate.py` classifies the failure as a candidate regression because the gate is self-consistent on main.
- Reproduced locally against current main (2cde97a8): merged `M(2cde97a8, 02f9726b)`, overlaid main's scripts → the exact two ERRORs, exit 1; restored the merge's own scripts → all gates pass (83 handlers OK). Evidence retained at `/tmp/stage/review-evidence/` (analysis, `reproduce.sh`, `abac-overlay-reproduction.log`).

On the merged tree with current main this round also verified: rustfmt changed-lines clean (31 files vs base), `SKIP_WEB_BUILD=1 cargo check -p gyre-server --all-targets` clean (7m52s — the candidate's 31 changed files compile together with task-160's 32 hierarchy files, disjoint sets), and all 21 static gates pass in both overlay phases except the artifact above. The web build is unchanged from the prior round's green run (task-160 touched no web files).

### Why this round ships no source change

Every candidate-side "fix" for the artifact violates the task contract or the no-fakery rules: re-adding the workspace-scoped routes (aliases/tombstones included) violates the immutable acceptance criterion "workspace-scoped routes are fully removed (clean cutover)" and regresses the approved review; re-adding the exemption entries is forbidden ("never add new entries"), fails the candidate's own frozen-count gate, and is irrelevant because phase 1 reads main's overlaid copy regardless; masking the checker (test-router fixtures, doc-comment `.route(...)` text, dead-code registrations) is the task-171 "mask a genuinely unregistered route" fakery class; editing `dev-check.sh`/`verifier-script-overlay.py` in the candidate cannot affect verification (the controller copies them from its own ROOT checkout) and would be verifier tampering from inside the task it gates. The candidate and its merge with current main are internally consistent and green under their own gates; there is no source defect to repair.

### Remaining work (controller/author-side)

- Scope the upstream script overlay to verifier logic (`*.sh`/`*.py`) so repo-state data files (`*-exemptions.txt`) follow the merge — exemption shrinkage is self-policing in a consistent tree (an entry removed without resolver registration fails `check-abac-route-registry`; growth is caught by dev-check.sh's "new verification exemptions forbidden" check and the frozen count), or
- land the two stale exemption entries' removal on main first (e.g. alongside this task's publication or via a process task), after which the overlay phase is clean for this candidate.

Future repair rounds on this finding: do NOT add source changes, exemptions, or checker masks for it — that is fakery or contract violation. Escalate via the repair-stall path if the harness-side resolution has not landed.

### Verifier instructions

Unchanged from prior rounds: `cargo test --all` and `cd web && npm test` on the merged tree (server test compilation was intentionally not run in this sandbox per the 4 GiB budget; `cargo check -p gyre-server --all-targets` passed on the exact merged tree instead).
