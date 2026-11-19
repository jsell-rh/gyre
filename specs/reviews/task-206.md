# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Independent review agent
**Round:** R-final (candidate cf73cd1c, repair round 70df2336)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** e1cf9d8d22352f8434addbb32b77d27b56b883db
**HEAD reviewed:** cf73cd1ce7859b0a08caecfa1d63cce6261b7787
**Status:** complete — approved

---

## Verdict

Approved. No findings. The candidate ships the §5 preview loop as a real agent
run path, removes the hollow structural-impact implementation and the
workspace-scoped routes cleanly, and hardens every ceremony surface a preview
agent could otherwise use to escape its scratch scope.

## Evidence (independent probes on the exact candidate tree)

- All 19 repo static gates exit 0, including `check-abac-route-registry` and
  `check-abac-exempt-handlers` — the two gates the prior controller
  verification round failed on the upstream-overlay artifact (stale exemption
  entries restored from main for routes this tree deliberately removed).
- `python3 scripts/test_dev_checks.py`: 10/10 OK. The verifier-overlay fix
  (d28e4a32) is tested in both directions: the clean-cutover scenario passes,
  and a candidate that removes a route but keeps its exemption entry still
  fails phase one — the data-follows-merge behavior cannot blind the upstream
  gates (merged exemption data is a frozen subset of base data; additions are
  forbidden separately by dev-check.sh).
- `cd web && npm ci && npm test`: 127 files, 2576 tests passed, exit 0.
- `cargo check -p gyre-server`: exit 0.
- `cargo test -p gyre-adapters --lib compute::ssh` (15 passed) and
  `--lib git2_ops` (35 passed): the new `SshTarget::from_handle`
  reconstruction (with connection-option round-trip and malformed-id
  rejection) and the new `force_remove_worktree` / `delete_branch` port
  methods are covered and green.
- Source identity: `git diff 1ee9cf7e..cf73cd1c -- crates web/src web/tests
  scripts docker` is empty — the Rust/web/scripts/docker sources are
  bit-identical to those the controller's retained verification run already
  compiled with rustfmt and full-target clippy green.

## Implementation review against the task contract

- **Real spawn per target** — `post_meta_spec_preview` resolves and validates
  every target first (repo exists, `check_repo_abac` + explicit tenant
  containment, spec_path readable on the default branch), then provisions per
  target: real `Active` agent row scoped to the repo's workspace, a
  `preview/{preview_id}/{slug}` branch pinned to the default-branch tip
  (so a moving default branch can never pollute the diff), a real git
  worktree, and a short-lived preview JWT. Response is exactly the spec shape:
  202 `{preview_id, agents:[{agent_id, repo_id, spec_path, branch}]}`.
- **Draft reaches the agent** — `GYRE_META_SPEC_DRAFT_KIND`,
  `GYRE_META_SPEC_DRAFT_CONTENT`, `GYRE_PREVIEW_ID`, `GYRE_TARGET_SPEC_PATH`
  travel in the process env; `docker/gyre-agent/agent-runner.mjs` builds the
  preview prompt with the draft as the governing how-to-build context over the
  target spec, withholds the task-mutating MCP tools, and never uploads a
  conversation or signals completion. `GYRE_TASK_ID` is deliberately absent.
- **Skip-ceremony is enforced, not just skipped** — the MCP dispatch refuses
  task/MR/complete/provenance tools for preview agents (allowedTools is
  advisory); the REST escape class (spawn, complete, stop/fail, spec approve,
  compose, orchestrator spawns, agent registration, commits/record,
  merge-queue, MR creation, task mutation) refuses preview-agent callers; no
  gates, no MR, no merge queue, no provenance, no `refs/agents/*` /
  `refs/tasks/*` writes.
- **Completion semantics** — every compute-target monitor exit routes through
  `finish_preview_agent`: token revoked, worktree removed, budget slot
  released, row Stopped — never Idle (an Idle preview agent could be
  re-targeted onto real work). The branch stays for diffing.
- **Status + diff** — `GET /api/v1/meta-specs/preview/{preview_id}` reports
  per-agent state from the real agent rows plus the diff of the preview branch
  against the pinned base SHA via the git adapter.
- **DELETE + GC** — `DELETE` kills agents (through the recorded backend, not
  the handle shape — the SshDocker/Kubernetes pid-less case is handled),
  force-removes worktrees, deletes branches, closes dangling MRs
  (belt-and-braces), removes the record; the `meta_spec_preview_gc` job
  (1 h interval, 24 h TTL, both configurable) expires old runs and reclaims
  orphans holding budget slots or live tokens.
- **Budget** — workspace budget by default (N targets of one POST reserve all
  N slots up front via `check_spawn_budget_for`), separate configurable
  `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` cap with fail-closed enumeration, and
  exactly-once slot release coordinated with every generic terminal path
  (stop/fail routes, admin kill, budget stop, stale-agent abort) via a durable
  per-agent `released` flag.
- **Clean cutover** — the hollow `compute_preview_blast_radius` /
  `StructuralImpact` preview implementation and both workspace-scoped preview
  routes are removed; the two stale `abac-route-registry-exemptions.txt`
  entries are removed with them (the file shrinks, which is its intended
  direction). New global routes are registered in the ABAC resolver.
- **Docs** — `docs/api-reference.md` (new §Meta-Spec Preview Mode section,
  per-endpoint preview-refusal notes) and `docs/server-config.md` (four new
  knobs) are updated, plus `docker/gyre-agent/README.md`.
- **Tests** — the in-file suite is hard, not self-confirming:
  `preview_spawns_real_agents_with_branches_worktrees_and_draft` drives a real
  spawned process whose env is dumped and asserted to contain the draft text,
  checks real branches/worktrees on disk, real agent rows, absent
  GYRE_TASK_ID, empty MR/task/provenance/ref stores, preview-TTL JWTs, budget
  accounting; further suites cover role/ABAC/tenant denials, budget cap and
  fail-closed enumeration, GC TTL boundary, DELETE teardown, completion
  teardown + diff, REST/MCP escape-class refusals, kill routing, and
  persist/worktree/agent-row failure rollback.

## Notes for the verifier (not source defects)

1. **Server test compilation is deferred to the verifier** per the sandbox
   build budget. Recommended commands:
   `cargo test -p gyre-server --lib -- preview_`,
   the `api::spawn`/`api::orchestrator`/`api::agents`/`api::specs_assist`/
   `api::agent_tracking` in-file suites,
   `cargo test -p gyre-server --test api_integration auth_integration
   e2e_ralph_loop git_integration m18_oidc_integration`, `cargo test --all`,
   and `cd web && npm ci && npm test`.
2. **Upstream-overlay artifact may recur once more**: the controller's ROOT
   copies of `scripts/verifier-script-overlay.py` are unaffected by candidate
   edits, so until d28e4a32 reaches the controller's main checkout, a
   verification round may still restore main's stale exemption entries and
   fail `check-abac-exempt-handlers` on the two removed workspace-scoped
   routes. The merged tree itself passes both gates (re-run directly in this
   review). This is controller-state, not a candidate defect; the repo-side
   fix is present, tested, and documented.
3. `web/dist/` regeneration is owned by the verifier (not reviewed; authored
   frontend source and tests reviewed instead — green above).
