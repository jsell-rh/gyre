# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Independent review agent
**Round:** R4 (candidate `caf121f688d7e58e5990b5980faca6312b6fb26a`; prior repair rounds
`a31a731a`, `c33a86e2`, `69488298` verified folded in)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** c66ee2ac9d0fff7e45cffc6dbeabdf90a3f3a70e
**Status:** needs-repair (3 findings, all test regressions introduced by the candidate;
the §5 production implementation itself is real and thorough)

---

## What was verified as real (no action required)

The §5 implementation is genuine, not structural. Independently confirmed against
source (probe logs under `/tmp/stage/review-evidence/`):

- **Routes**: `POST/GET/DELETE /api/v1/meta-specs/preview[/:preview_id]` registered
  (api/mod.rs:871-877), ABAC resolver entries added (abac_middleware.rs:725-730),
  workspace-scoped routes and the hollow structural-impact estimator removed
  (exemptions file shrank by the two legacy entries). `cargo check -p gyre-server
  --lib --tests` exits 0.
- **Spawn**: `post_meta_spec_preview` resolves every target up front (repo exists,
  `check_repo_abac` + explicit tenant containment, spec_path exists on the default
  branch, branch pinned to the default-branch tip `base_sha`), provisions a real
  `Active` agent row scoped to the repo's workspace (no "default" scope literal),
  a `preview/{preview_id}/{slug}` branch, a real worktree, a short-lived JWT
  (`preview_jwt_ttl_secs`), and takes a budget slot per agent. Rollback on
  provision/persist failure is compensating and tested.
- **Draft delivery**: `GYRE_PREVIEW_ID`, `GYRE_META_SPEC_DRAFT_KIND/CONTENT`,
  `GYRE_TARGET_SPEC_PATH` injected via `launch_taskless_agent_process`
  (extra_env merged before server-owned vars, so it cannot shadow identity vars);
  `GYRE_TASK_ID` deliberately absent. `docker/gyre-agent/agent-runner.mjs` builds
  the preview prompt from those vars (draft as governing context over the target
  spec), withholds task-mutating MCP tools, skips provenance upload and completion.
  The k8s regression test asserts the draft reaches the applied Pod manifest.
- **Skip-ceremony enforced, not just skipped**: REST guards on
  `POST /agents/:id/complete`, `POST /merge-requests`, `POST /tasks`,
  `PUT /tasks/:id`, `POST /merge-queue/enqueue`; MCP dispatch refuses
  `gyre_create_task`/`gyre_update_task`/`gyre_create_mr`/`gyre_agent_complete`/
  `conversation_upload` for preview agents. Exercised through the production
  `build_router` (ABAC middleware + seeded builtin policies) with the preview
  agent's own live JWT, including admin positive controls.
- **Completion**: every compute-target monitor routes through
  `on_agent_process_exit` → `finish_preview_agent`: token revoked, worktree
  removed, budget slot released (idempotent via the persisted `released` flag),
  row lands `Stopped` — never `Idle`. Branch survives for diffing; the status
  endpoint reports per-agent state from the real agent rows and the produced
  diff (branch vs pinned base sha, same plumbing as MR diffs).
- **Launch verdicts** judge by the dispatch's own `launched` flag, never the
  handle shape (repair round `69488298`); ssh-docker and kubernetes regressions
  pin this with fake `ssh`/`kubectl` binaries; `mark_preview_agent_failed` and
  DELETE kill through the recorded `spawned_backends` pair, purging both
  lifecycle registries together.
- **Cleanup**: DELETE tears down process/worktree/branch/token/slot/records and
  closes any open MR on a `preview/{id}/*` branch; GC job `meta_spec_preview_gc`
  registered (jobs.rs + main.rs spawn) with TTL boundary test (at/past TTL
  deleted, just-under retained) and an orphan-pass reclaim.
- **Frontend**: MetaSpecs.svelte implements the §9 loop against the real §5 API
  (202 shape validation, per-agent status/diff polling, tabs per target, Clean Up
  = DELETE, Iterate); web suite green (`npm ci` + `npm test`: 2441 passed /
  41 skipped, 122 files).
- **Mechanical invariants** all pass in-sandbox: check-arch, abac-route-registry,
  abac-exempt-handlers, mcp-write-tools, in-memory-state-stores, inert-enforcement,
  lossy-secret-conversion, dead-message-kinds, mem-port-contracts,
  byte-slice-truncation, forged-scope-fields, fail-open-ref-resolution,
  migration-versions, scope-literal-defaults, fabricated-scope-defaults,
  relative-path-defaults, unbounded-external-http, forwarded-header-trust,
  migration-sql-portability.

## Findings (blocking — the verifier's `cargo test -p gyre-server --lib` will be red)

The sandbox build wrapper blocks gyre-server test codegen (4 GiB budget; attempt
logged in `rbac-test-attempt.txt`), so these were established by source analysis
with base-comparison evidence, not by execution. All three are the same root
cause class: guards/extractors were added and routes removed without updating
dependent tests outside the files the implementer edited.

### F1 — rbac.rs posts to the removed workspace-scoped preview route

`crates/gyre-server/src/rbac.rs:366-390`
(`uninstrumented_role_guards_record_rbac_denied_audit_event`, sub-case 2) still
POSTs `/api/v1/workspaces/ws-1/meta-specs/preview` with a readonly JWT and
asserts `403` plus one `rbac_denied` audit event. The candidate removed that
route (base mod.rs:808 registered it; candidate mod.rs has only the global
`/api/v1/meta-specs/preview`), and `api_router()` has no fallback — axum answers
`404`, no handler runs, no audit event is recorded. Both assertions fail.
Fix: retarget the probe at `POST /api/v1/meta-specs/preview` and update the
expected route string to `"POST /api/v1/meta-specs/preview"` (the new
`require_preview_role` records exactly that, with minimum role
"Admin or Developer" — the probe's intent is preserved).

### F2 — merge_queue.rs tests post to `enqueue` without a Bearer token

`crates/gyre-server/src/api/merge_queue.rs:250,279,300,326,340,382,542`
(the `enqueue_entry` helper and six direct posts). The candidate added an
`AuthenticatedAgent` extractor to `enqueue`
(merge_queue.rs:59-70) — a correct, necessary change for the preview-agent
guard — but did not update this file's test module. The extractor rejects a
missing Bearer token with `401` before the handler runs (auth.rs:1345), so
`enqueue_returns_created`, `enqueue_with_priority`,
`enqueue_nonexistent_mr_returns_404` (expects 404, gets 401),
`enqueue_is_idempotent_per_open_mr`,
`enqueue_after_terminal_entry_creates_fresh_entry`, and the reverted-MR test
all fail. Fix: add `authorization: Bearer test-token` to the helper and the
direct posts (exactly what the candidate already did for the tasks.rs and
merge_requests.rs test modules).

### F3 — rbac.rs seeds an MR through `create_mr` without a token

`crates/gyre-server/src/rbac.rs:460-486` (same test, sub-case 5) documents
"create_mr has no auth extractor, so seed the MR without a token", POSTs
`/api/v1/merge-requests` with no Authorization header and asserts `201`. The
candidate added the `AuthenticatedAgent` extractor to `create_mr`
(merge_requests.rs:268-280) — again correct for the preview guard — so the seed
request now gets `401` and the assertion fails. Fix: add the header (the
downstream `PUT .../status` probe with the agent JWT is unaffected).

## Non-blocking observations (no repair required)

- The task's test plan listed "caller without repo access → 403". The
  enforcement is real (meta_specs.rs:875-884 tenant containment on POST;
  `require_preview_scope` on GET/DELETE), and the readonly-role 403 is tested,
  but no test drives the cross-tenant/ABAC-denied branch directly. Worth adding
  opportunistically.
- `specs/coverage/system/meta-spec-reconciliation.md` row 9 still reads
  `task-assigned | Hollow (2026-09-29)`; per the stage contract the coverage
  matrix/SUMMARY regeneration is verifier-owned — re-audit row 9 against the
  merged tree after verification (the §5 source is implemented; the row is
  stale, not the code).
- `EditorSplit.svelte` retains a `context="meta-spec"` mode, but after this
  change no production component mounts it that way (MetaSpecs.svelte implements
  the §9 loop directly; DetailPanel mounts `context="spec"`). The mode is kept
  green only by EditorSplit.test.js — production-dead, kept as a parallel client.
  Not a spec breach; note for a future cleanup task.
- `web/dist/` was not rebuilt in this checkpoint (verifier-owned; recorded
  separately per the stage contract — not a repair finding).

## Probes the verifier must run (build wrapper blocked them here)

- `cargo test -p gyre-server --lib -- preview_` — full preview suite (spawn,
  draft injection, authz, skip-ceremony REST+MCP, DELETE, GC TTL, diff,
  ssh-docker/kubernetes launch, persist-failure rollback, budget caps).
- `cargo test -p gyre-server --lib -- merge_queue` — **currently red (F2)**.
- `cargo test -p gyre-server --lib -- uninstrumented_role_guards` — **currently
  red (F1, F3)**.
- `cargo test -p gyre-server --lib` and the workspace suite after repair.
- `cargo clippy -p gyre-server --lib --tests`, `cargo fmt --check`.
- `cd web && npm ci && npm test` (green here: 2441 passed / 41 skipped).

## Verdict

**needs-repair.** The §5 production implementation is real, spec-conformant and
well-tested, but the candidate leaves three guaranteed-red tests in the
mandatory `cargo test -p gyre-server` run (F1-F3 above, all in test code the
candidate did not update when it removed the workspace-scoped routes and added
auth extractors). Repair is mechanical (retarget one route string, add Bearer
headers in two test modules); no production-code change is required by these
findings.
