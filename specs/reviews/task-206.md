# Review: task-206 — Meta-spec preview mode (real agent preview runs)

**Reviewer:** Independent review (stage: review)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** e69fd990ce241ff586e5b42218ffe592900026ca
**HEAD reviewed:** 4a63f70287d55f79758a5c9f50442ae377bd9d70
**Status:** needs-revision (one finding)

---

## What was verified (independently)

### Spec-conformant surface (§5 contract)

- `POST /api/v1/meta-specs/preview` (global, Admin/Developer) validates every
  target up front — repo exists (404), caller passes repo ABAC + tenant
  containment (403), spec_path readable on the default branch (400) — then
  provisions per target a real `Active` agent row, a `preview/{preview_id}/{slug}`
  branch pinned to the default-branch tip (`resolve_ref`, fail-closed), a real
  git worktree, and a short-lived preview JWT (`GYRE_PREVIEW_JWT_TTL_SECS`,
  default 1800 s). 202 response is exactly the spec shape:
  `{preview_id, agents:[{agent_id, repo_id, spec_path, branch}]}`.
  Slug collisions inside one request are disambiguated (full-path fallback,
  `-N` suffix); the sanitizer rejects `..`, leading/trailing `-`/`.`, and
  `.lock` tails.
- `GET /api/v1/meta-specs/preview/{preview_id}` reports per-agent state derived
  from the real agent rows plus the produced `diff` (preview branch vs the
  pinned base sha, MR-diff plumbing). 404 on unknown id.
- `DELETE /api/v1/meta-specs/preview/{preview_id}` kills agents through the
  recorded handle/backend pair, force-removes worktrees (before branches — git
  refuses deleting checked-out branches), closes open MRs on preview branches,
  deletes branches, revokes tokens, drops kv records; 404 on unknown id.
- Clean cutover: the hollow structural-impact preview, the workspace-scoped
  routes, and their frozen ABAC exemption lines are gone (exemption file
  shrank, never grew); the new routes are in the ABAC resolver; `grep` for
  `compute_preview_blast_radius`/`StructuralImpact`/`previewPersona` returns
  nothing. Docs updated (`api-reference.md` § Meta-Spec Preview Mode,
  `server-config.md` knobs, agent README).

### Draft delivery and skip-ceremony

- Draft travels out-of-band (`GYRE_PREVIEW_ID`, `GYRE_META_SPEC_DRAFT_KIND`,
  `GYRE_META_SPEC_DRAFT_CONTENT`, `GYRE_TARGET_SPEC_PATH`; `GYRE_TASK_ID`
  deliberately absent); `agent-runner.mjs` builds the preview prompt itself
  (draft as governing "how to build" context over the target spec, push to the
  preview branch as the entire deliverable) and withholds task-mutating tools.
  `node --check` clean.
- Skip-ceremony is enforced server-side, not just skipped: the MCP dispatch
  refuses `gyre_create_task`/`gyre_update_task`/`gyre_create_mr`/
  `gyre_agent_complete`/`conversation_upload` for preview agents, and the REST
  surface refuses `POST /agents/{id}/complete`, `POST /merge-requests`,
  `POST /merge-queue/enqueue`, `POST/PUT /tasks` for preview-agent callers
  (positive admin controls in the tests prove the gates scope to preview
  agents, not the routes).
- Completion semantics: every compute-target dispatch arm registers through
  `register_spawned_agent` → `monitor_poll_once` → `on_agent_process_exit` →
  `finish_preview_agent`: token revoked, worktree removed, budget slot
  released (idempotent via the persisted `released` flag), row `Stopped` —
  never `Idle`. The branch survives for diffing. The started check reads
  `dispatch_agent_launch`'s own `launched` flag, never the handle's shape
  (SshDocker/Kubernetes handles are pid-less by design).
- Budget: workspace budget by default with all N targets reserved up front
  (`check_spawn_budget_for` — a single POST cannot over-admit against
  `max_concurrent_agents < N`); `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` caps
  preview admission independently when set (§5 "separate preview budget,
  configurable, defaults to workspace budget").
- GC: `meta_spec_preview_gc` job registered (jobs.rs) and spawned (main.rs),
  TTL `GYRE_META_SPEC_PREVIEW_TTL_HOURS` (default 24 h), interval default 1 h;
  the sweep also reclaims orphaned agent index entries (slot, token, worktree,
  branch).
- Rollback paths are real and pinned by tests with genuine failure injection
  (`NamespaceFailingKv` persist failure, ENOTDIR worktree failure with
  pre-incremented slots proving no phantom decrement, `CreateFailingAgents`
  proving the orphaned branch is deleted inline).

### Probes run in this sandbox

- `cargo check -p gyre-server --lib --tests` — exit 0 (test code type-checks;
  suite execution deferred to the verifier per the stage build wrapper).
- `cargo test -p gyre-adapters --lib -- compute::ssh` — 15 passed, 2 ignored
  (real sshd required), including the new `SshTarget::from_handle`
  round-trip/refusal tests.
- `cd web && npm ci && npx vitest run MetaSpecs.test.js EditorSplit.test.js
  InlineChat.test.js` — 181/181 passed.
- Mutation probe: removing `kind` from the preview POST draft in
  `MetaSpecs.svelte` makes `Preview POST sends the spec-conformant draft and
  targets` fail (1 failed); restored, it passes. The wire-shape test bites.
- All mechanical checks pass: arch, ABAC route registry (exemptions shrank),
  ABAC exempt handlers, mem port contracts, MCP write tools, byte-slice
  truncation, relative path defaults, fail-open ref resolution, in-memory
  state stores, lossy secret conversion, fabricated/constructor scope
  defaults, inert enforcement, forwarded-header trust, unbounded external
  HTTP, dead message kinds.
- Evidence: `/tmp/stage/review-evidence/` (review-summary.txt + logs).

### Required verifier probes (gyre-server execution deferred from this stage)

- `cargo test -p gyre-server --lib -- preview_` (spawn/draft/authz/
  skip-ceremony/DELETE/GC-TTL/diff, compute-backend launch regressions,
  budget cap, persist/worktree/agent-row failure rollbacks)
- `cargo test -p gyre-server --lib -- merge_queue`
- `cargo test -p gyre-server --lib -- uninstrumented_role_guards`
- `cargo test -p gyre-server --lib -- kill_process_handle`
- `cargo test -p gyre-server --lib -- monitor_`
- `cargo test -p gyre-adapters --lib`
- `cd web && npm ci && npm test`

---

## Finding (blocking)

**F1 (test): the repo-access / cross-tenant containment branches of the new
global preview routes have no negative test.**

`post_meta_spec_preview` enforces `crate::abac::check_repo_abac` plus a
tenant-containment check per target (meta_specs.rs ~862-884), and
`require_preview_scope` enforces the same containment for GET/DELETE
(meta_specs.rs ~1336-1377) — but every preview test authenticates with the
global test token, which carries no JWT claims (ABAC admin bypass) and the
Admin role (tenant check skipped). The only JWT-based preview authz test is
the readonly role gate. The task's own test plan enumerates "caller without
repo access → 403" and the acceptance criteria require tests to prove authz.

Reproduce the gap: delete the tenant-containment block at meta_specs.rs:876-883
(or the `check_repo_abac` call) and run `cargo test -p gyre-server --lib --
preview_` — everything still passes. A Developer from tenant A spawning
preview compute against tenant B's repos is a cross-tenant scope escape on a
global route that the suite would not catch.

Fix: add a test that signs a Developer-role JWT for a different tenant (and/or
seeds an ABAC policy denying the repo) against a valid preview target and
asserts 403 on POST, plus a cross-tenant GET/DELETE of a live preview id
asserting 403 through `require_preview_scope`.

## Non-blocking observations

- `specs/coverage/system/ui-layout.md` row 37 (task-176 note) still describes
  the §9 loop as "mounted by the MetaSpecs workspace view" against the removed
  `/workspaces/:id/meta-specs/preview` endpoints; row 13 (updated by this
  task) correctly records the reconciliation onto the §5 API. Historical-note
  staleness only; row 13 is the accurate one.
- `npm ci` in web/ re-runs the prepare step (vite build) and rewrites
  `web/dist` asset hashes — generated output owned by the verifier; the tree
  was restored to clean after the probe.

## Verdict

**needs-revision.** The implementation itself is real, spec-conformant and
defense-in-depth (no hollow paths found; enforcement is server-side), and the
test suite is unusually rigorous — but one enumerated authz behavior
("caller without repo access → 403") is enforced in production code yet
unproven by any test, on exactly the branch class (cross-tenant scope on a
global route) where a regression would be silent.
