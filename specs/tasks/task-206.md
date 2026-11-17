---
title: "Implement meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup"
spec_ref: "meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop"
commits: ["3f408a013d9d640631fdd6bb712091cb9e970077", "683da5a1afe0bb249a8fa0aa3e84e65006d9cde2", "a26619ecca7785d7a514ae7b269186e2d17599c0", "f4ee98ba5521f5b8e973ae3172481dec4f2d34de", "4a63f70287d55f79758a5c9f50442ae377bd9d70", "285ec0b71aaf82b8f6f64c36980a84ed1a9ab7d0", "8212fa5d945070afac1e859a96e1caae27af950c", "07d00fd0fbadd4196655d8e33849477bf00454bb", "18c14c8f985ed3e3c300664355103330da693741", "05c06fde1c501cd1f9f5ab5b729c1ec2a1ba2860", "109c9594840b43e2368759e6be47ac43cba8077e", "3f8575fc846ae2ee4d6123c926c3d7424ba48c13", "caf121f688d7e58e5990b5980faca6312b6fb26a", "6bfdaac97844a1772e1084d652e5f2aa8bd9fc1c", "eed3d0b270b720180096e58969b27b0ce77fb89d", "944bed7233f930b4d45f199dd78c538eba40c100", "dc974a1d7fef86e774d49c301b72823bdaef5a4f", "1622b73cdf7e36d32299a4010462b9b1c046eab9", "c8f9fed9abef048df4abf83d4a86e1407a17b9db", "5e61026db12fd8f40bfd469df10a5a08a69a1899", "08c3b5e2e0af006504396351726c681e85a70fa6", "001ec52dd65e890637751d40ecb1a64743a4934d", "1d2564fa7b295d398ea09f4e5a078496ca680bbe", "a31a731a262c62f7c85202cbca727fe9d9633646", "fd1bf277193caa0fa41fab8333fbb7ed23355063", "7cc0ca33312f3f2147fb6c07701871c69a67ec6d"]
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

### Merge round 7cc0ca33 (this checkpoint)

Rebased the task-206 source onto base `1f86b006` (task-186: Ghost Overlays + §3 progressive preview). Conflicts resolved in `web/src/lib/EditorSplit.svelte` and `web/src/locales/en.json`:

- **EditorSplit.svelte** — kept both features: the §5 real meta-spec preview loop (`pollPreviewRun`, per-agent `agentStatus*` labels, real status shape) and task-186's §3 ghost-overlay machinery (`normalizeGhosts`, `ghostMode` `previewing`→`confirmed` pulsing). Dropped only base's `pollPreviewPersona` path (its hollow workspace-scoped endpoint was removed by this task's clean cutover). Kept base's thorough-tier improvements that apply to the still-existing §3 task-driven path: `previewAgentId`, `loadPreviewMrDiff` (MR-diff mapping onto the Code Diff tab), and the real task-status wire vocabulary (`review`/`done`/`blocked`/`cancelled`, never `completed`/`failed`). The right-pane template adopts base's layout — the per-agent progress strip renders ABOVE the Architecture/Code Diff tabs (not instead of them) so the ghost canvas stays visible while agents run — with HEAD's real §5 per-agent status rendering inside the strip.
- **en.json** — kept this task's rewritten `no_line_diff_available` text and took task-186's new keys (`predicting`, `impact_bar_aria`, `impact_connected_specs`, `impact_repos_implementing`, `impact_governed_nodes`, `impact_governed_nodes_unknown`).
- The two preview paths coexist by design: task-186's thorough tier (§3) is task-driven (preview task + completion MR), this task's §5 preview agents are taskless and MR-less; the 403-for-preview-agent guards on complete/MR/merge-queue apply only to §5 preview agents, not to §3 thorough-tier workers.

### Test evidence

In-sandbox on merge head `7cc0ca33` (evidence at `/tmp/stage/review-evidence/merge-round-7cc0ca33.txt`):

- `cd web && npm ci && npm test` — **124 files, 2493 passed, 41 skipped, 0 failed** (both the task-206 §5 preview suite and task-186's ghost-overlay/§3 progressive-preview suite, including the two conflicted files' tests: EditorSplit 63/63).
- `cargo check -p gyre-server --lib --tests` — clean; build.rs web-asset build succeeded on the merged Svelte sources (regenerated `web/dist` artifacts restored to committed state afterwards — dist is rebuilt/remotely published by integration, not committed here).
- `bash scripts/check-arch.sh`, `check-abac-route-registry.sh`, `check-mem-port-contracts.sh`, `check-migration-versions.sh`, `check-dead-message-kinds.sh`, `check-byte-slice-truncation.sh`, `check-mcp-write-tools.sh`, `check-inert-enforcement.sh`, `check-fabricated-scope-defaults.sh`, `check-scope-literal-defaults.sh`, `check-lossy-secret-conversion.sh`, `check-forwarded-header-trust.sh`, `check-in-memory-state-stores.sh`, `check-unbounded-external-http.sh`, `check-fail-open-ref-resolution.sh`, `check-relative-path-defaults.sh`, `check-forged-scope-fields.sh` — all pass. `check-task-commit-attribution.sh` passes with this task file's commits list.
- The build wrapper blocks gyre-server test codegen in this stage, so server test execution is deferred to the independent verifier. Required probes:
  - `cargo test -p gyre-server --lib -- preview_` (full preview suite: spawn/draft/authz/skip-ceremony/DELETE/GC-TTL/diff, compute-backend launch regressions, budget cap, persist/worktree/agent-row failure rollbacks, cross-tenant containment)
  - `cargo test -p gyre-server --lib -- merge_queue` (authorized enqueue probes)
  - `cargo test -p gyre-server --lib -- uninstrumented_role_guards` (retargeted preview guard + authorized MR seed)
  - `cd web && npm ci && npm test`

Sandbox limitation: TCP listener probes are unsupported here (capabilities.json), so no live HTTP/browser verification; the REST surface is exercised through the production `api_router` via `tower::ServiceExt::oneshot` against real git + real processes, and exact-head GitHub CI checks remain mandatory after review.
