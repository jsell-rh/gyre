# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Independent review (pi runtime)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** 97062a0952b48e3270e4ef7c7e6e34d6a64b4f5b
**HEAD reviewed:** 02f9726bdf03f8c72e2bde6e1ae5dfbac13a533b
**Status:** complete — approved
**Evidence:** /tmp/stage/review-evidence/summary.md (exact commands, exit codes, outputs)

---

## Verdict

Approved. Every acceptance criterion is met by production code wired through
the real entry points; the skip-ceremony invariant is enforced on both the MCP
and REST surfaces (including the identity-laundering escape class closed in
repair rounds F1–F5); the tests are hard tests through the production router
with real git repos, real spawned processes and real JWTs. Full evidence and
the deferred-verifier probe list are in `/tmp/stage/review-evidence/`.

## Acceptance-criteria audit

- **Real spawn path** — `POST /api/v1/meta-specs/preview` (global route per
  spec; Admin/Developer, audited rbac_denied) validates every target up front
  (repo exists → 404, repo ABAC + tenant containment → 403, spec_path exists
  on the default branch → 400), then per target provisions a real `Active`
  agent row, a `preview/{preview_id}/{slug}` branch pinned to the
  default-branch tip (no fail-open `resolve_ref` — `None` → 400), a real git
  worktree, and a short-lived preview JWT (`GYRE_PREVIEW_JWT_TTL_SECS`,
  default 1800 s) with scope claims derived from the loaded workspace/repo
  rows. Response is exactly the spec shape `202 {preview_id, agents:[…]}`.
  Provisioning failures roll back everything (branch deleted inline when the
  agent row fails; persist failure rolls back all agents + the partial
  record — covered by dedicated rollback tests).
- **Draft delivery** — env-only (`GYRE_PREVIEW_ID`, `GYRE_META_SPEC_DRAFT_KIND`,
  `GYRE_META_SPEC_DRAFT_CONTENT`, `GYRE_TARGET_SPEC_PATH`; `GYRE_TASK_ID`
  absent by construction); `agent-runner.mjs` preview mode builds the prompt
  with the draft as governing "how to build" context over the target spec,
  withholds task-mutating MCP tools, uploads no conversation, never calls
  complete. Happy-path test dumps the real process environment and asserts
  the draft text plus the absence of `GYRE_TASK_ID`.
- **Skip-ceremony enforced, not just skipped** — MCP dispatch refuses
  task/MR/provenance mutations for preview agents; REST surface refuses
  preview-agent callers on complete (both directions), stop/fail (plus
  tenant containment and the usage self-report contract), status routes,
  agent registration (the only token-minting route — closes identity
  laundering), task create/update/status, MR create/status, merge-queue
  enqueue/cancel, spec approve, spec save, commit provenance recording,
  compose apply/teardown, orchestrator spawn on both tiers (Admin/Developer
  role gate + preview refusal; dev bootstrap unaffected — system token
  resolves as Admin). No `refs/agents/*`/`refs/tasks/*` writes, never Idle.
- **Budget** — workspace budget by default via `check_spawn_budget_for`
  (N-target requests reserve all slots up front per workspace);
  `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` caps preview spawns independently
  (429 on overflow), documented in server-config.md.
- **Status + diff** — `GET /api/v1/meta-specs/preview/{preview_id}` derives
  per-agent state from the real agent rows (`running|complete|failed|stopped|
  dead|unknown`) and exposes the produced diff (preview branch vs pinned base
  sha, MR-diff plumbing) once the branch exists. Completion test drives a
  real agent process that commits and pushes, then asserts Stopped (never
  Idle), worktree gone, branch kept, token revoked, diff surfaced.
- **Completion semantics** — every compute-target dispatch registers the
  shared exit monitor (`dispatch_agent_launch` → `register_spawned_agent` →
  `monitor_spawned_agent_exit` → `on_agent_process_exit` →
  `finish_preview_agent`), so preview completion is backend-independent:
  token revoked, worktree removed, budget slot released (idempotent, exactly
  once), agent row Stopped. Remote-launch verification keys off the
  dispatch's `launched` flag, never the handle shape.
- **DELETE + GC** — `DELETE` kills processes through the backend recorded at
  spawn time, force-removes worktrees, closes open MRs on preview branches,
  deletes branches, revokes tokens, drops kv records; 404 unknown id. GC job
  `meta_spec_preview_gc` (registered in jobs.rs, spawned in main.rs;
  `GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS` default 1 h, TTL
  `GYRE_META_SPEC_PREVIEW_TTL_HOURS` default 24 h) tears down expired runs
  and reclaims orphaned agent entries. TTL boundary test covers at/past-TTL
  deletion, just-under retention, and the orphan pass.
- **Clean cutover** — hollow structural-impact implementation, workspace-
  scoped preview routes, and their ABAC exemption entries are fully removed
  (grep clean; exemptions file shrank by exactly the two removed routes, and
  the frozen-count registry check passes). Docs updated: api-reference.md
  (full Preview Mode section incl. the skip table and guard list),
  server-config.md (4 new knobs), docker/gyre-agent/README.md.
- **Frontend** — MetaSpecs.svelte / EditorSplit.svelte ported to the §5 API
  (global routes, per-agent status polling, tabbed diff viewer, Clean Up);
  malformed-response and failure paths render honest errors; api.js uses the
  global routes. 127 test files / 2572 tests pass in this sandbox
  (`npm ci` + `npm test`).

## Independent probes run here

`cargo check -p gyre-server --lib --tests` (exit 0); all 19 mechanical
invariant scripts pass (arch, ABAC route registry + exempt handlers, MCP write
tools, migrations, task-commit attribution, in-memory stores, inert
enforcement, byte-slice, relative paths, fail-open refs, mem port contracts,
fabricated/lossy/scope literals, forged scope fields, forwarded headers,
unbounded HTTP, dead message kinds, SQL portability); rustfmt-diff clean;
`cargo test -p gyre-adapters --lib git2_ops` (35 passed — crate owning the new
`force_remove_worktree`/`delete_branch` port methods); frontend suite green;
all 16 attributed commits verified ancestors of HEAD.

## Deferred to the independent verifier

gyre-server test codegen exceeds this stage's memory budget (wrapper-enforced).
Required probes (also listed in the evidence summary):
`cargo test -p gyre-server --lib -- preview_`; `--lib api::spawn api::orchestrator
api::agents api::specs_assist api::agent_tracking`; `--test api_integration
auth_integration e2e_ralph_loop git_integration m18_oidc_integration`;
`python3 scripts/check-clippy-diff.py 97062a0`; re-run `cd web && npm ci && npm test`
on the integrated tree.

## Minor observations (non-blocking)

- `count_running_preview_agents` fails open (returns 0, warns) when the
  preview-agent kv listing errors — only the optional
  `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` cap is affected; the spec-default
  workspace budget path uses a separate store and remains enforced.
- `require_preview_scope` skips repos deleted after a run started
  (documented in-code); read-only status surface, GC reclaims the record.
- `web/dist/` was not rebuilt in this stage — owned by the deterministic
  verifier per checkpoint policy; authored frontend source and tests reviewed
  and green.
