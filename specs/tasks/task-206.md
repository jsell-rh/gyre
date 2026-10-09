---
title: "Implement meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup"
spec_ref: "meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop"
depends_on: []
progress: needs-revision
coverage_sections:
  - "meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop"
commits: ["2ae25c17198e8c3d45bd7508d70af636bd33fd65", "b40714fa5c5f1f534abdbe13bca219d4f3ec1495", "608fd050412238f29da0706ef7bd52b41a932902", "05a8b4d1118c13a1a9e7ad791f961566373a629a", "930e6b1fefa73cfd1897e14362ed85d5699d861a", "f7d9168dea13760201ae40dfde2b45177a757056", "5955934352a6c3610b87c3727648782755dc11c0"]
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


## Repair Log (2026-10-09)

The prior round migrated `web/src/lib/api.js` to the spec-conformant global
preview routes but left `web/src/components/MetaSpecs.svelte` referencing four
deleted state variables (`previewProgress`, `previewApiResult`,
`isSimulatedPreview`, `impactTab`) and calling the removed
`api.previewPersona`/`api.previewPersonaStatus` — clicking Preview threw at
runtime and silently fell back to a simulated diff (the handoff's reported
defect).

Repaired in commit `92093ee` (this branch): the workspace preview flow now
targets the real contract — POST `/api/v1/meta-specs/preview` with
`{draft, targets}` built from the `/specs` ledger entries, 2s status polling
that mirrors per-agent state and the produced `DiffResult` patches, per-target
tabs, bounded polling with a stalled notice (no fake completion), and Clean Up
wired to `DELETE /api/v1/meta-specs/preview/{id}`. The simulated fallback,
structural-impact tabs, dead styles, and orphaned i18n keys were removed.
Workspace-scope tests rewritten against the real API mocks (106 pass; the only
other full-suite failure, `ExplorerCanvas-performance`, pre-exists on HEAD and
is a timing-sensitive test unrelated to this task).

## Review Findings (2026-10-09, HEAD 30422d2, base 66422bd4)

Verdict: **complete**. Independent inspection of the diff, call sites, and
wiring; persisted evidence reused only after confirming source unchanged at
HEAD 30422d2 (tree clean).

### Behavior verified against §5

- **Routes & authz**: `POST/GET/DELETE /api/v1/meta-specs/preview` registered
  (mod.rs:735-739) with ABAC `RouteResourceMapping` entries
  (abac_middleware.rs:449-452); old workspace-scoped routes and the hollow
  structural-impact preview fully removed — clean cutover confirmed by grep
  (no `workspaces/:id/meta-specs/preview`, no `StructuralImpact`, no
  `compute_preview_blast_radius` anywhere). Role gate Admin|Developer on all
  three handlers; POST validates repo existence (404), per-target
  `check_repo_abac` + tenant containment (403), spec on default branch (400).
- **Real spawns**: `provision_preview_agent` creates real agent rows (Active,
  workspace-scoped from the repo record, no task), real `preview/{id}/{slug}`
  branches pinned to the default-branch tip, real worktrees via
  `Git2OpsAdapter::create_worktree`, and short-lived JWTs
  (`state.preview_jwt_ttl_secs`, default 1800s — distinct from
  `GYRE_AGENT_JWT_TTL`). Provisioning failure rolls back everything created
  so far; launch failure marks the agent Failed and releases its slot while
  keeping the run visible. The launch requires a real process handle — a
  spawn that never started cannot sit "running".
- **Draft reaches the agent**: env injection (`GYRE_META_SPEC_DRAFT_KIND`,
  `GYRE_META_SPEC_DRAFT_CONTENT`, `GYRE_TARGET_SPEC_PATH`, `GYRE_PREVIEW_ID`,
  no `GYRE_TASK_ID`) proven against a dumped process environment, not a
  field; `agent-runner.mjs` builds the preview prompt from the draft content
  and instructs reading the target spec and pushing the branch.
- **Skip-ceremony enforced server-side**: MCP dispatch refuses
  `gyre_create_task`/`gyre_update_task`/`gyre_create_mr`/
  `gyre_agent_complete`/`conversation_upload` for preview agents
  (mcp.rs:2958-2980) — allowedTools withholding is advisory, the server gate
  is not. Git push path coherent for preview agents: `current_task_id` is
  None so TASK-008 attestation-chain enforcement and constraint checks are
  skipped (§5 skip-table), and repo pre-accept gates are opt-in per repo.
- **Completion semantics**: all three compute-target monitors route through
  `on_agent_process_exit` → `finish_preview_agent` (spawn.rs:737): Stopped
  (never Idle), worktree removed, token revoked, budget slot released, branch
  kept for diffing. Test asserts Stopped-not-Idle through a real process
  lifecycle.
- **GET status**: per-agent state derived from real agent rows +
  `released` flag; diff computed against the pinned `base_sha` once the
  branch exists (null before). Test drives a real commit+push from the
  spawned agent script and asserts the patch surfaces.
- **DELETE**: kills processes (container-aware via `kill_process_handle`),
  stops rows, removes worktrees, deletes branches, revokes tokens, releases
  slots, drops both kv namespaces; 404 on unknown id; second DELETE 404.
- **GC**: `meta_spec_preview_gc` registered (jobs.rs:508-524, default 1h) and
  spawned in main.rs:74; TTL default 24h via `GYRE_META_SPEC_PREVIEW_TTL_HOURS`;
  saturating age computation (clock-skew safe); orphan pass reclaims agent
  records whose run is gone (token/branch/slot). TTL boundary tested
  (at/past deleted, just-under retained).
- **Budget**: workspace budget by default (check_spawn_budget +
  increment/decrement), separate `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` cap with
  429 on overflow; slot release is single-shot via the `released` flag.
- **UI repair**: dead references gone (grep clean), generation-guarded
  polling, per-agent tabs, diff rendering, Clean Up → DELETE; all used i18n
  keys present in en.json.
- **Exemption changes are shrinkages, not gate weakening**: two route-registry
  exemptions removed with the routes; the fabricated-scope-default and
  lossy-secret-conversion entries for spawn.rs were fixed in code
  (skip-and-warn on unresolvable tenant / non-UTF-8 secret) and their frozen
  counts lowered accordingly; relative-path line numbers re-anchored only.
- **Commit attribution**: all 7 task-labeled commits in the review range
  (5955934, f7d9168, 930e6b1, 05a8b4d, 608fd05, b40714f, 2ae25c1) are in the
  frontmatter `commits:` list. The repair log's `92093ee` is pre-rebase local
  numbering; the reworked UI repair landed as `2ae25c1` with equivalent
  content (verified by reading the commit body).

### Evidence (persisted under /tmp/stage/review-evidence/)

- `task206-rust-tests.md`: `cargo test -p gyre-server --lib meta_spec` — 18
  passed, exit 0, at HEAD 30422d2 (fresh private target dir, 909s cold).
- `task206-mcp-spawn-tests.md`: `mcp_preview_agent_denied_ceremony_tools` (1)
  and the spawn module suite (37) pass at the same HEAD — the
  `on_agent_process_exit` refactor regressed nothing in normal-agent behavior.
- `task206-web-tests.md`: `vitest run src/__tests__/MetaSpecs.test.js` — 106
  passed, exit 0, same HEAD. (Repair log's note: `ExplorerCanvas-performance`
  failure pre-exists on HEAD, unrelated.)
- All 19 mechanical invariant scripts pass at HEAD (incl. check-arch,
  check-abac-route-registry, check-abac-exempt-handlers, exemption-count
  checks).

### Non-blocking nits (recorded, no repair required)

- Millisecond-wide window where process-exit and a concurrent DELETE/GC can
  both read `released=false` and double-decrement the budget counter. Impact
  is saturating (`MAX(0, active_agents-1)` in all three adapters) and
  accounting-only; no safety decision reads the counter.
- `preview_agent_status_label` maps `Dead` → "dead" and `Failed` → "failed";
  the run-level `state` stays "complete" when no agent is running even if
  some failed — the per-agent statuses carry the failure signal, and the UI
  renders them per tab.

## Shipped

- Real meta-spec preview runs: `POST /api/v1/meta-specs/preview` spawns one
  agent per target on `preview/{preview_id}/{slug}` branches (real worktrees,
  short-lived JWTs, draft injected via env and surfaced as the governing
  prompt by the agent runner); `202` returns the spec's `{preview_id,
  agents[]}` shape.
- Ceremony stripped and enforced server-side: preview agents are denied
  task/MR/completion/provenance MCP tools, create no tasks/MRs/provenance/
  refs writes, and land Stopped (never Idle) with token revoked and budget
  slot released when their process exits — the pushed branch is the entire
  deliverable.
- Full cleanup lifecycle: `GET` status with real per-agent state + produced
  diffs; `DELETE /api/v1/meta-specs/preview/{id}` tears down processes,
  worktrees, branches and records; background `meta_spec_preview_gc` expires
  runs past the configurable TTL (default 24h) and reclaims orphans.
- Workspace preview UI wired to the real contract: spec-ledger target
  selection, bounded 2s status polling with per-agent tabs and diff
  rendering, Clean Up calling DELETE; config knobs documented in
  server-config.md and routes in api-reference.md.

## Review

### Review changed source code

- specs/coverage/system/meta-spec-reconciliation.md

Preserved these edits for implementation. Review cannot approve its own source or verifier edits. Repair them within task scope and request a fresh independent review.
