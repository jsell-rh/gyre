# Review — task-206 (Meta-Spec Preview Mode: Real Agent Preview Runs)

Spec: `specs/system/meta-spec-reconciliation.md` §5 "Preview Mode: The Fast Iteration Loop".
Candidate: `248f2dab6fcba06835ba2a7a52682a37f1c3d8b6` (base `5196f44488fab3a7e1e2fcb25186ee4ea116ffcb`).
Task: `specs/tasks/task-206.md`.
Verdict: **approved**.

## Prior rounds

Two earlier review rounds (attempts `2f672369...` and `14e29589...`) found the same two defect classes; both are fixed in commit `4fc19fca` and independently re-verified here:

1. REST ceremony bypass — `POST /api/v1/agents/{id}/complete` and `POST /api/v1/merge-requests` accepted a preview agent's own JWT (201 + real MR + Idle transition + provenance kv + snapshot refs). Now `complete_agent` (spawn.rs:1283) and `create_mr` (merge_requests.rs:260, keyed off the authenticated caller's agent-scoped JWT) both return 403 for preview agents.
2. `kill_process_handle` had no ssh arm — SSH handle pids are remote pids, so DELETE/GC/admin-kill ran `kill -TERM <remote-pid>` locally (remote process survives, unrelated local process terminated). Now ssh handles dispatch through `SshTarget::from_handle` (destination `user@host:pid` + `identity_file`/`port` restored from handle options); malformed ids are refused, never killed locally.

## This round — evidence

Full log: `/tmp/stage/review-evidence/task-206-review.md`. All probes on the exact candidate with `/tmp/stage/bin/cargo`; working tree verified clean after all probes (temporary mutation edits restored, `web/dist` rebuilt by build.rs restored to committed state).

- `cargo test -p gyre-server --lib preview` — 9/9 ok (spawn+branch+worktree+draft-env via real spawned processes, finish→Stopped-never-Idle+diff, DELETE teardown, validation/role gates, budget cap 429, GC TTL boundary, REST ceremony refusal, MR close on delete).
- `cargo test -p gyre-server --lib` — **1232 passed, 0 failed**.
- `cargo test -p gyre-adapters --lib from_handle` — 3/3 ok; `kill_process_handle_refuses_malformed_ssh_handle` ok.
- Web: `npm ci` + `vitest run src/__tests__/MetaSpecs.test.js` — 106/106.
- Mechanical gates all OK: arch, abac-route-registry (exemptions file *shrank* by 2 — workspace-scoped preview routes deleted, not grandfathered), mcp-write-tools, mem-port-contracts, fabricated-scope-defaults, lossy-secret-conversion, in-memory-state-stores, relative-path-defaults, byte-slice-truncation.

**Mutation probes (test strength):** each review fix was individually disabled and its regression test re-run:

- Disable `complete_agent` preview gate → `preview_agent_rest_ceremony_is_refused` fails (201 vs 403). ✅ detects.
- Disable `create_mr` preview gate → same test fails. ✅ detects.
- Disable the ssh arm in `kill_process_handle` → test binary killed by SIGTERM — local dispatch killed the test runner's own pid, reproducing exactly the defect class the fix prevents. ✅ detects.

All three mutations restored; suite re-verified green afterwards.

## Verified working (no findings)

- **Spec-conformant routes**: global `POST/GET/DELETE /api/v1/meta-specs/preview[/:preview_id]` (mod.rs:736-742) with ABAC registry entries (abac_middleware.rs:445-450); the hollow workspace-scoped routes are gone from mod.rs and the exemption file. `compute_preview_blast_radius`/`StructuralImpact` fully removed (grep clean).
- **Real spawn path** (not a stand-in): per target — branch `preview/{preview_id}/{slug}` pinned to the default-branch tip, `Active` agent row scoped from the repo record (no fabricated scope), real git worktree + worktree row, short-lived JWT in `agent_tokens` (separate `GYRE_PREVIEW_JWT_TTL_SECS`), real process launch via the shared `launch_agent_process`. All-or-nothing: validation pass (repo exists, repo ABAC + tenant containment, spec_path exists on default branch) completes before anything is created; partial failures roll back.
- **Draft delivery**: env-only (`GYRE_PREVIEW_ID`, `GYRE_META_SPEC_DRAFT_KIND`, `GYRE_META_SPEC_DRAFT_CONTENT`, `GYRE_TARGET_SPEC_PATH`; `GYRE_TASK_ID` deliberately absent); `docker/gyre-agent/agent-runner.mjs` builds the draft-governed prompt from those vars. Test asserts the dumped process environment contains the draft text.
- **Skip-ceremony enforced on every reachable surface**: MCP dispatch refuses `gyre_create_task`/`gyre_update_task`/`gyre_create_mr`/`gyre_agent_complete`/`conversation_upload` for preview agents; REST `complete_agent`/`create_mr` 403; teardown closes any open MRs on preview branches. No provenance, no refs/agents|tasks writes, no task assignment, never Idle (`on_agent_process_exit` → `finish_preview_agent`: token revoked, worktree removed, budget released, row → Stopped with the terminal flip ordered last).
- **Budget**: workspace budget by default; `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` caps independently (429 on overflow), slots released on finish/DELETE/GC and on launch failure.
- **Cleanup**: DELETE kills (ssh-aware), force-removes worktrees, deletes branches, revokes tokens, drops kv records, 404 on unknown id, scoped by role + per-repo ABAC + tenant containment (`require_preview_scope`). GC job `meta_spec_preview_gc` registered (jobs.rs:508, main.rs:74) with TTL boundary proven (at-TTL deleted, just-under retained, orphans reclaimed).
- **Docs**: api-reference.md (routes, shapes, skip-table, enforcement), server-config.md (4 knobs), docker/gyre-agent/README.md.

## Limitations

TCP listener probes are unsupported in this sandbox (capabilities.json, errno 95); the REST surface is exercised through the production `build_router` (auth + ABAC middleware, seeded builtin policies) via `tower::ServiceExt::oneshot` with the preview agent's own live JWT — the mutation probes prove this harness detects regressions. Live HTTP/browser checks remain for CI.
