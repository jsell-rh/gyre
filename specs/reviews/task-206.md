# Review — task-206 (Meta-Spec Preview Mode: Real Agent Preview Runs)

Spec: `specs/system/meta-spec-reconciliation.md` §5 "Preview Mode: The Fast Iteration Loop" (lines 151-233).
Candidate: `6a5852447922459af61b4299f8b36342f75ca216` (base `6bf777a6a44f28052ed5af28bf6fb013fde6df48`).
Verdict: **needs-revision** (2 findings, one major).

Evidence directory: `/tmp/stage/review-evidence` (diffs, probe sources + outputs, check-script logs, `focused-test-results.md`, `probe-rest-ceremony-bypass.md`).

## What was verified working

The core preview loop is genuinely implemented — this is a real agent run path, not the hollow estimate it replaces:

- **Spawn**: `POST /api/v1/meta-specs/preview` (global route per spec, Admin/Developer role gate) resolves every target first (repo exists, `check_repo_abac` + tenant containment, spec_path readable on the default branch), then provisions per target a real Active agent row, a `preview/{preview_id}/{slug}` branch pinned to the default-branch tip, a real worktree via `git_ops.create_worktree`, a worktree row, and a short-lived JWT minted with `state.preview_jwt_ttl_secs` (separate from the agent JWT TTL). Response is exactly the spec shape (`202 {preview_id, agents:[{agent_id, repo_id, spec_path, branch}]}`). Provisioning failure rolls back everything created (branch, worktree, worktree row, agent row, token, budget slot) — the rollback paths pass `released` correctly so the saturating budget decrement cannot double-release.
- **Tests are real**: `preview_spawns_real_agents_with_branches_worktrees_and_draft` runs a real bare repo + real Git2OpsAdapter + real spawned process dumping its environment to a file, then asserts the draft content string is present in the dumped env (not a field-set check), `GYRE_TASK_ID` absent, branches/worktrees exist on disk, no MR/task/provenance/`refs/*` writes, token TTL uses the preview knob. Finish/DELETE/GC/budget-cap/validation tests are equally concrete (15 passed, 0 failed at candidate).
- **Completion semantics**: every compute-target monitor routes through the new `on_agent_process_exit`, which sends preview agents to `finish_preview_agent` — token revoked, worktree removed, budget slot released idempotently (`released` flag), agent row lands `Stopped`, never `Idle`. The branch survives for diffing. `preview_agent_finish_teardown_and_diff` proves this end-to-end with a real process committing on the branch and the diff surfacing in GET status.
- **Cleanup**: `DELETE /api/v1/meta-specs/preview/{preview_id}` kills processes, force-removes dirty worktrees (new `force_remove_worktree` port op, implemented in git2 + mem + configurable adapters), deletes branches, revokes tokens, drops both kv namespaces; 404 on unknown id. GC job `meta_spec_preview_gc` registered in jobs.rs + spawned in main.rs, TTL boundary tested both directions, plus an orphan-agent reclaim pass.
- **Clean cutover**: `compute_preview_blast_radius`/`StructuralImpact`/`PreviewBlastRadius` have zero remaining references; the workspace-scoped preview routes and their ABAC exemption entries are gone; web `api.js`/`MetaSpecs.svelte` call the new global routes and render per-agent diff tabs; docs (api-reference, server-config, agent README) match the implementation.
- **Hard tests pass at candidate**: `api::meta_specs` 15/15, `api::spawn` 29/29, `mcp::tests::mcp_preview_agent_denied_ceremony_tools` 1/1. All mechanical check scripts OK except `check-task-commit-attribution.sh`, which fails on **base** commit 6bf777a6 (task-200 frontmatter debt) — pre-existing, reproduced on the base, not introduced here.

Sandbox limitation: TCP listener probes unsupported (capabilities.json, errno 95), so no live HTTP/browser check; the REST surface was exercised through `build_router` + `tower::ServiceExt::oneshot` against real git and real processes.

## Findings

### F1 (major): the "skip-ceremony enforced" guarantee does not hold on the REST surface — a preview agent's own JWT can create MRs, provenance, and refs writes

The implementation's central enforcement claim — "the runner withholds these tools client-side, but allowedTools is advisory — the server must not execute task/MR/complete mutations for a preview agent" — is implemented only in the MCP dispatch (`mcp.rs` guard keyed off `is_preview_agent`). But the same mutations exist as ordinary REST endpoints with no auth extractor and no preview-membership check, and the preview agent's JWT is a fully valid credential for them:

- `POST /api/v1/agents/:id/complete` (`spawn.rs:1251`) — **probe result: 201 Created**, real MR created on `preview/{id}/{slug}` with `author_agent_id` = the preview agent. The handler additionally does everything §5 forbids and the finish path carefully avoids: transitions the agent to **Idle** (`spawn.rs:1362`, violating "Preview agents are killed on completion (no idle state)"), writes `agent_provenance` kv when `conversation_sha` is supplied (`spawn.rs:1353-1359`), and writes `refs/agents/{id}/snapshots/*` (`spawn.rs:1394-1417`).
- `POST /api/v1/merge-requests` (`merge_requests.rs:249`) — **probe result: 201 Created** with a live preview token, MR on the preview branch.

Why the middleware doesn't stop it: `require_auth_middleware` accepts any `agent_tokens` entry, and the seeded builtin `agent-scoped-access` policy (priority 700) allows role=Agent the `write` and `complete` actions for resource type `*` — production `main.rs:47` seeds these at startup, so the probe configuration mirrors production.

Threat model is real, not theoretical: the preview agent is an LLM-driven process with `Bash` in its allowedTools and `GYRE_AUTH_TOKEN` in its environment; the runner's own tool-withholding is exactly the advisory layer the implementation notes say the server must not rely on. Consequences: an MR row (durable ledger state, contractually forbidden in preview mode) survives `DELETE`/GC — `teardown_preview` removes branch/worktree/token/kv but not MR rows — leaving a permanently dangling MR pointing at a deleted branch; an Idle preview agent is re-targetable onto real work, which is the precise hazard `finish_preview_agent`'s comments call out.

Reproduce (probe source and output preserved in `/tmp/stage/review-evidence/probe1-preview-rest-ceremony-bypass.rs` and `probe-rest-ceremony-bypass.md`): copy the probe into `crates/gyre-server/tests/`, `cargo test -p gyre-server --test preview_ceremony_probe` — both assertions fail with 201. Fix direction: apply the same `is_preview_agent` gate inside `complete_agent` and `create_mr` (403 before any write), and consider having `teardown_preview` close MRs that reference preview branches as defense in depth.

### F2 (moderate): `kill_process_handle` dispatches SSH handles to the local kill — remote-pid confusion can kill an unrelated local process

`spawn.rs:753-766` special-cases `target_type == "container"` (correct: kills by container id) but routes **every other** handle — including `target_type == "ssh"` — to `LocalTarget::kill_process`, which runs `kill -TERM <pid>` locally. An SSH compute target's `ProcessHandle.pid` is the pid echoed by the **remote** shell (`compute/ssh.rs:217-226`; handle id is `user@host:pid`). So `DELETE /api/v1/meta-specs/preview/{id}` (and admin kill, which now shares this helper via `admin.rs:278`) on an SSH-spawned preview agent terminates nothing remotely, and if the local machine happens to have a process with that pid it is killed — an unrelated-process termination. The correct dispatch (`SshTarget::kill_process`) exists; the handle just doesn't carry enough target info to reconstruct it. Fix direction: add an ssh arm that reconstructs the target from `handle.id` (`user@host:pid`), or persist the compute-target identity alongside the process handle.

## Notes (non-blocking)

- The registry-authz additions (`require_registry_admin`, tenant-scoped visibility filtering, admin-only mutations on the exempt registry routes) go beyond the task's stated scope but close a real `check-abac-exempt-handlers` gap and are covered by `registry_mutations_require_admin`. They are correct as written; noting the scope expansion for the record.
- The GC job's `run_once` sweeps both namespaces on every tick with full `kv_list` scans; at preview volumes implied by the budget cap this is fine, but it is O(all previews) per hour — acceptable.
- The two legacy exemption baselines that shrank (`fabricated-scope-defaults`, `lossy-secret-conversion`) were genuinely fixed by the spawn.rs refactor (tenant-scope skip instead of `"default"` fallback; `String::from_utf8` with skip+warn instead of lossy conversion) — verified by reading the new code and the passing checks, not just the baseline counts.
