# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Independent review (stage)
**Round:** R2 (post F1-repair `7cad1261`)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** e9ed695561dabf1dc5de3239851d07686a43daaa
**HEAD reviewed:** 8f04ecc52180a31af3abd5c487c1583365756a1e
**Status:** needs-revision
**Evidence:** /tmp/stage/review-evidence/summary.md

---

## Verdict

Not approved. One blocking finding: the preview-agent escape class opened in
round R1 (F1) and declared closed by repair `7cad1261` is **still open** on
five more mutating REST surfaces. The core preview implementation itself is
real and well-built — spawn, draft delivery, skip-ceremony, completion,
status/diff, DELETE, and GC all check out against §5 — but §5's "no footprint"
contract is only as strong as the weakest route a live preview-agent JWT can
reach, and the containment the candidate itself established as its standard is
incomplete.

## F1′ (blocking, code/security): preview-agent JWT still reaches mutating routes with durable production effects

The candidate's own hard test (`preview_agent_rest_status_routes_are_refused`,
meta_specs.rs:5109) proves the mechanism: a live preview-agent JWT (Agent role;
builtin `agent-scoped-access` allows read/write/complete/approve on `*` at
priority 700) passes `require_auth_middleware` and ABAC through the production
router; the 403s come only from per-handler guards. Routes that still lack the
guard:

1. **`POST /api/v1/agents/:id/stop`, `POST /api/v1/agents/:id/fail`**
   (`crates/gyre-server/src/api/spawn.rs`): the handlers take no auth extractor
   and perform zero authorization; the routes are ABAC-exempt
   (`scripts/abac-route-registry-exemptions.txt:17-18` — exempt means *no*
   policy evaluation). A preview agent can stop or fail **any agent in any
   tenant**, revoking its token and task secrets, decrementing its workspace
   budget, and firing orchestrator death handling (replacement spawns). This is
   a strictly larger blast radius than `DELETE /api/v1/merge-queue/:id`, which
   this candidate guarded in R1 for exactly this rationale.
2. **`POST /api/v1/agents/:id/usage`** (exemptions:19): same shape; corrupts
   usage accounting for arbitrary agents.
3. **`POST /api/v1/specs/:path/approve`** (`crates/gyre-server/src/api/specs.rs`
   `approve_spec`): the role gate's minimum approving role is Agent, and the
   builtin agent policy's action set includes `approve` (route maps to
   `spec`/`approve`). A preview agent can emit production `SpecApprovalEvent`s
   for its target repo. Spec approval is §5's skip-table row 1 — the single
   most on-point ceremony route there is.
4. **`POST /api/v1/compose/apply`, `POST /api/v1/compose/teardown`**
   (`crates/gyre-server/src/api/compose.rs`): neither handler takes an auth
   parameter. `compose_apply` creates task and agent rows — a direct bypass of
   the guarded `POST /api/v1/tasks` — and `compose_teardown` kills agents.
5. **`POST /api/v1/workspaces/:id/orchestrator/spawn`**
   (`crates/gyre-server/src/api/orchestrator.rs`): tenant containment only, no
   role gate; the route maps to `workspace`/`write`, which is not repo-addressed
   so the `agent-repo-scope` containment Deny cannot fire. A preview agent can
   spawn a workspace orchestrator, which decomposes specs into tasks and spawns
   workers.

Reproduction: extend `preview_agent_rest_status_routes_are_refused` — POST a
preview, take the live JWT from `agent_tokens/{agent_id}`, then
`POST /api/v1/agents/<any-other-agent>/stop` with that bearer token; the
handler mutates the other agent with no authorization decision anywhere.

Scope note: most of these routes were reachable by regular agent JWTs before
this task (baseline debt — `check-abac-exempt-handlers.sh` explicitly skips
handlers with no auth parameter). They are findings against this candidate
because the candidate introduced preview agents, the R1 review established
this exact reachability class as blocking for task-206, and the candidate's
shipped claim that "the REST surface does too [refuse preview-agent callers]"
is false for these routes.

Cleared after checking: the MCP spawn/decompose tools
(`gyre_spawn_worker`, `gyre_decompose_spec`, `gyre_cross_repo_task`,
`gyre_spawn_repo_orchestrator`) are not preview-reachable (they require
orchestrator-scoped tokens); `put_meta_spec_set`, `put_rollout_policy`,
`reject_spec`, `release_prepare`, and secrets routes gate on Admin/Developer;
repo-addressed mutations are contained to the preview's own target repo by the
`agent-repo-scope` Deny.

## What checks out (no finding)

- **Spawn is real** (meta_specs.rs:832, provision at :1529): four-pass
  all-or-nothing provisioning — up-front target validation (repo exists, repo
  ABAC + tenant containment, spec path readable on the default branch), budget
  (workspace budget grouped per workspace; separate
  `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` cap), rollback compensation covering
  agent row / branch / worktree / kv index / budget slot (the `released` flag
  prevents double-decrement), persist-before-launch with partial-persist
  cleanup, launch-failure marking. 202 response matches the spec shape
  exactly; branches pinned to the resolved default-branch tip (no fail-open
  ref resolution); JWT minted with tenant/workspace/repo claims from the
  loaded rows at short TTL.
- **Skip-ceremony** is enforced server-side where guarded: no task assignment
  (`task_id: None`, no task row), no gates, no MR, no merge queue, no
  provenance, no `refs/agents/*` writes; MCP dispatch refuses the five
  ceremony tools for preview agents.
- **Completion** (meta_specs.rs:1103): every compute-target monitor routes
  through `on_agent_process_exit` → `finish_preview_agent`: token revoked,
  worktree removed, budget slot released (idempotent), terminal `Stopped` —
  never `Idle`; the branch survives for diffing.
- **Status + diff**: per-agent state derived from real agent rows; diff
  computed against the pinned base sha with the MR diff plumbing.
- **Cleanup**: DELETE kills agents, removes worktrees/branches, closes open
  MRs on preview branches, revokes tokens, drops records; GC job
  `meta_spec_preview_gc` (jobs.rs:705) with env-tunable interval/TTL and an
  orphan pass.
- **Clean cutover**: hollow structural-impact preview and workspace-scoped
  routes removed; the registry exemption file shrank by the two preview
  entries (shrinkage is allowed).
- **Docs**: api-reference.md, server-config.md, docker/gyre-agent/README.md
  updated for the new routes and knobs.
- **F1 repair itself** (7cad1261): the four new guards and the hard
  live-JWT test are correct and well-constructed.

## Probes run in this stage

`check-arch`, `check-abac-route-registry`, `check-abac-exempt-handlers`,
`check-mcp-write-tools` — all pass. `cargo check -p gyre-server --lib --tests`
— clean (7m51s). gyre-server test codegen is wrapper-blocked in this stage
(exit 64, 4 GiB budget); the full server test list the verifier must run is
recorded in /tmp/stage/review-evidence/summary.md (preview suite, REST guard
regressions, in-file suites touched by the extractor additions, merge_queue,
uninstrumented_role_guards, prompt, and `cd web && npm ci && npm test`).
