# Review — task-206 (meta-spec-reconciliation §5 Preview Mode)

**Reviewer:** independent review stage
**Round:** R1 (this checkpoint)
**Spec ref:** `specs/system/meta-spec-reconciliation.md` §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** `9f446d0eb33ecb24b04c821e47007d8884882b63`
**Candidate reviewed:** `07d00fd0fbadd4196655d8e33849477bf00454bb`
**Verdict:** needs-revision (2 code findings, 1 docs finding)

---

## What was verified as real (no finding)

The candidate replaces the hollow structural-impact preview with a genuine
agent run path; independent inspection and probes corroborate every shipped
claim that was checkable in this sandbox:

- **Routes / clean cutover.** `POST /api/v1/meta-specs/preview`,
  `GET|DELETE /api/v1/meta-specs/preview/:preview_id` registered
  (api/mod.rs:864-877) with ABAC resolver entries; the workspace-scoped
  routes and the hollow blast-radius estimate are gone, and the frozen
  exemption file shrank by exactly the two removed lines (never grew).
- **Real spawns.** `post_meta_spec_preview` resolves and authorizes every
  target up front (repo exists → 404, `check_repo_abac` + tenant containment
  → 403, spec_path readable on the default branch → 400), then provisions per
  target a real `Active` agent row (scoped to the repo's workspace, never a
  literal default), a `preview/{preview_id}/{slug}` branch pinned to the
  default-branch tip (collision-sanitized), a real git worktree, and a
  short-lived preview JWT (separate TTL knob). 202 body is exactly
  `{preview_id, agents:[{agent_id, repo_id, spec_path, branch}]}`.
- **Draft delivery.** `GYRE_PREVIEW_ID`, `GYRE_META_SPEC_DRAFT_KIND`,
  `GYRE_META_SPEC_DRAFT_CONTENT`, `GYRE_TARGET_SPEC_PATH` injected via
  `launch_taskless_agent_process` (extra_env merged before the server-owned
  identity vars so it cannot shadow them; `GYRE_TASK_ID` deliberately absent);
  `agent-runner.mjs` builds the preview prompt from those vars, withholds the
  task-mutating MCP tools, skips conversation upload and the complete call.
- **Skip-ceremony enforced, not just skipped.** `POST /agents/{id}/complete`,
  `POST /merge-requests`, `POST /tasks`, `PUT /tasks/{id}`,
  `POST /merge-queue/enqueue` all gain preview-agent guards keyed off the
  `preview_agents` kv discriminator (403), and the MCP dispatch refuses
  `gyre_create_task` / `gyre_update_task` / `gyre_create_mr` /
  `gyre_agent_complete` / `conversation_upload` for preview agents. No
  `refs/agents/*` / `refs/tasks/*` writes.
- **Completion semantics.** All compute backends register through
  `dispatch_agent_launch` → `register_spawned_agent` → the single exit
  monitor, now routed through `on_agent_process_exit` → `finish_preview_agent`:
  token revoked, worktree removed, budget slot released (idempotent via the
  `released` flag), row lands `Stopped` — never `Idle`. The launch
  started/failed verdict reads `dispatch_agent_launch`'s own `launched` flag
  (SshDocker/Kubernetes handles are pid-less by design), and
  `mark_preview_agent_failed` kills through the recorded handle/backend pair.
- **Status + diff.** GET status derives per-agent state from the real agent
  rows and computes the diff of the preview branch against the pinned base sha
  with the same git plumbing as MR diffs.
- **Cleanup.** DELETE kills (through `spawned_backends`), force-removes
  worktrees (new `force_remove_worktree` port method, implemented in git2 +
  mem + configurable adapters), closes open MRs on preview branches, deletes
  branches, revokes tokens, drops the kv records; 404 on unknown id. GC job
  `meta_spec_preview_gc` registered (jobs.rs + main.rs) with configurable
  interval/TTL and an orphan pass over `preview_agents`.
- **Probes** (logs under `/tmp/stage/review-evidence/`): all 19 mechanical
  check scripts PASS (arch, ABAC registry, exempt handlers, MCP write tools,
  migrations, mem port contracts, etc.); `cargo check -p gyre-server --lib`
  PASS; `cargo test -p gyre-adapters --lib -- ssh:: git2_ops::` 50/50 PASS
  (incl. `SshTarget::from_handle` round-trip/refusal tests); `web npm ci &&
  npm test` 122 files / 2441 passed / 41 skipped; focused
  `MetaSpecs.test.js` + `EditorSplit.test.js` 164/164 passed, 0 skipped.
- The Rust preview test suite itself (spawn/draft-injection via a real
  process env-dump, authz, skip-ceremony via the production router with the
  preview agent's live JWT, DELETE, GC TTL boundary, diff exposure, ssh-docker
  and kubernetes launch regressions against fake `ssh`/`kubectl`) is
  well-constructed and hard — but the build wrapper blocks gyre-server test
  codegen in this stage (exit 64, evidence retained), so execution is deferred
  to the verifier; required probes listed below.

## Findings

### F1 (major, code): unmatched budget decrement on the worktree-creation failure path

`provision_preview_agent` takes the workspace/tenant budget slot only at the
very end (`increment_active_agents`, meta_specs.rs:1700). The
`create_worktree` failure rollback (meta_specs.rs:1588-1605) passes
`released: false` (meta_specs.rs:1602), so `rollback_preview_agent` runs
`decrement_active_agents` on the workspace key **and the tenant-global key**
with no matching increment — while every sibling early-failure path correctly
sets `released = true` with the comment "no slot was taken"
(meta_specs.rs:1639/1670/1691). `decrement_active` (sqlite/postgres
kv_store.rs:334, mem.rs:3551) decrements whatever usage row exists, so in any
workspace or tenant with ≥1 genuinely active agent the concurrency count is
permanently biased down by one per occurrence, letting
`check_spawn_budget` / `check_spawn_budget_for` over-admit past
`max_concurrent_agents` and the tenant ceiling. That corrupts the **shared**
budget enforcement used by all spawns — an acceptance-criterion area ("budget
is enforced"). Reachable whenever `git worktree add` fails after branch +
agent-row creation succeeded (stale directory at the worktree path — git
refuses existing paths — unwritable repo dir, disk full). No test covers this
failure point; the persist-failure test exercises a later point where the
increments have happened. Remedy: set `released = true` on that rollback ref
(no slot was taken), and add a regression test that pins the workspace/tenant
usage counters across a forced worktree-creation failure.

Related, same function: `state.agents.create(&agent).await?`
(meta_specs.rs:1580) propagates without rollback, orphaning the just-created
`preview/{preview_id}/{slug}` branch with no record — invisible to the GC
orphan pass, which iterates the `preview_agents` kv index. Route it through
the same compensation.

### F2 (moderate, code): dead `api.specsAssistGlobal` call — workspace meta-spec editor chat never reaches the LLM

`MetaSpecs.svelte:649` (`handleWsChatMessage`) calls
`api.specsAssistGlobal({ persona_id, message })`. That method does not exist
in `web/src/lib/api.js` (grep: 0 hits; the real method is
`metaSpecsAssist(workspaceId, { instruction, draft_content?, meta_spec_id? })`
at api.js:759, matching the documented `/workspaces/{id}/meta-specs/assist`
body). The `TypeError` is swallowed by the surrounding
`catch { /* fall through */ }`, so the InlineChat in the workspace-scope
meta-spec editor silently degrades to the canned local suggestion
(`+ <text>\n# Suggested addition`) on every message. Regression from base
9f446d0, where MetaSpecs mounted `EditorSplit context="meta-spec"` whose chat
correctly called `api.metaSpecsAssist(workspaceId, …)` (EditorSplit.svelte:571,
still correct on the candidate). `MetaSpecs.test.js:97` mocks
`specsAssistGlobal` — the suite validates the fallback, not the assist
(stub-backed gap). Remedy: call
`api.metaSpecsAssist(workspaceId, { instruction: text, draft_content: selectedMsContent, meta_spec_id: selectedMsId })`
and add a test asserting that call shape.

### F3 (minor, docs): §5 coverage matrix row still describes the hollow implementation

`specs/coverage/system/meta-spec-reconciliation.md` row 9 is unchanged and
still reads `task-assigned | task-206 | Hollow (code-verified 2026-09-29) —
POST …/preview spawns NO agents …`. The shipped code is a real agent run
path, so the matrix row — the input the verifier regenerates
`specs/coverage/SUMMARY.md` from — is now inaccurate (in the conservative
direction). Repo convention has the implementer flip the row when recording
shipped notes (cf. "docs(task-176): record shipped notes, coverage row 37
implemented"); the candidate flipped ui-layout.md row 13 but not its own §5
row. Flip it to `implemented` with an accurate note in the repair round.

## Non-blocking observations

- `require_preview_scope` (meta_specs.rs:1336) skips ABAC for agent entries
  whose repo row was deleted (`None => continue`): a developer outside the
  run's tenant could read the status/diff of a preview whose repos are all
  deleted. Scratch data, low impact; recorded for a future hardening pass.
- The working tree carries a controller-owned uncommitted revert of
  `specs/tasks/task-206.md` to the base task body; left untouched per the
  review assignment.
- Generated `web/dist/` not reviewed (verifier-owned).

## Required verifier probes (blocked in this sandbox by the build wrapper)

- `cargo test -p gyre-server --lib -- preview_`
- `cargo test -p gyre-server --lib -- merge_queue`
- `cargo test -p gyre-server --lib -- uninstrumented_role_guards`
- `cargo test -p gyre-server --lib -- mcp_preview_agent_denied_ceremony_tools`
- `cargo test -p gyre-server --lib -- monitor_`
- `cargo test -p gyre-server --lib -- kill_process_handle`
- exact-head CI checks (`cargo clippy`, `cargo fmt --check`, full test matrix)
