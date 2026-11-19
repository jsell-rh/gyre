# Review: task-206 — Meta-spec preview mode (real agent preview runs)

**Reviewer:** Independent review agent
**Round:** R1 (this candidate)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** b381514b7b5661f988dacd8e98d303ced0952595
**HEAD reviewed:** 30e323dae4de0e730edf28cf678999bd1d208083
**Status:** complete — approved (no blocking findings)

---

## Verdict

Approved. The hollow structural-impact preview implementation is fully replaced
by a real agent run path that satisfies every acceptance criterion of the task
contract. Independent probes and source inspection support the approval; the
gyre-server test suite itself is deferred to the deterministic verifier (the
review-stage build wrapper reserves server code generation for its 4 GiB
sandbox) and is listed below.

Note on diff scope: the base b381514b is a task-262 branch tip whose SPIFFE
federation work never reached the candidate's history (merge-base is upstream
main e1cf9d8d), so `git diff base..candidate` shows federation deletions that
are not candidate actions. The task-206 work itself is the meta_specs/spawn/
jobs/routes/guards/UI/docs changes reviewed here.

## Independent evidence

All probe logs are retained under `/tmp/stage/review-evidence/`
(`review-notes.md` summarizes):

- `cargo check -p gyre-server` — clean, 0 errors (9m33s, single-job wrapper).
- `cd web && npm ci && npm test` — 127 files / 2576 tests passed, 41 skipped,
  exit 0.
- 19 static invariant gates (arch, ABAC route registry + exempt handlers,
  MCP write tools, migration versions/SQL portability, dead message kinds,
  byte-slice truncation, relative path defaults, fail-open ref resolution,
  mem port contracts, fabricated scope defaults, lossy secret conversion,
  scope literal defaults, inert enforcement, forwarded header trust,
  in-memory state stores, unbounded external HTTP, forged scope fields) —
  19/19 PASS.
- `python3 -m unittest scripts.test_dev_checks` — 10/10 OK, including the two
  regression tests for the verifier-overlay exemption-data fix (d28e4a32).
- `cargo test -p gyre-adapters from_handle` (3/3) and
  `cargo test -p gyre-adapters git2` (all) — the new ProcessHandle.options /
  SshTarget::from_handle and git worktree/branch/diff adapter code verified
  behaviorally.
- `python3 scripts/check-rustfmt-diff.py b381514b` — changed lines clean.

## Acceptance criteria — evidence map

1. **Real spawn per target** — `post_meta_spec_preview` (meta_specs.rs:832)
   validates every target first (repo 404, `check_repo_abac` + tenant
   containment 403, spec-path-on-default-branch 400), then provisions per
   target: real Active `Agent` row scoped to the repo's workspace, a
   `preview/{preview_id}/{slug}` branch pinned to the default-branch tip via
   `resolve_ref`, a real git worktree (`state.git_ops.create_worktree` +
   `state.worktrees.create`), a minted EdDSA JWT stored in `agent_tokens`.
   Slug collisions disambiguate via the sanitized full path
   (`preview_branch_name`). Proven by
   `preview_spawns_real_agents_with_branches_worktrees_and_draft` (real git
   repos, real spawned processes via a GYRE_AGENT_COMMAND env-dump script).
2. **Draft reaches the agent** — `launch_preview_agent` injects
   `GYRE_META_SPEC_DRAFT_KIND`/`GYRE_META_SPEC_DRAFT_CONTENT`/
   `GYRE_TARGET_SPEC_PATH`/`GYRE_PREVIEW_ID` (no `GYRE_TASK_ID`);
   `agent-runner.mjs` builds the preview prompt with the draft as governing
   context over the target spec. The env-dump test asserts the literal draft
   text in the spawned process's environment.
3. **202 shape** — `PreviewResponse { preview_id, agents: [{agent_id, repo_id,
   spec_path, branch}] }` exactly; asserted in tests.
4. **Skip-ceremony + budget** — no task assignment, no gates, no MR, no merge
   queue, no provenance, no `refs/agents/*`/`refs/tasks/*` writes (asserted);
   enforced server-side: MCP dispatch refuses task/MR/complete/provenance
   tools for preview agents, and every REST escape route (complete, MR
   create/update, task create/update, merge-queue enqueue/cancel, agent
   register/status, compose, workspace-orchestrator spawn, spec approve,
   specs assist, commit provenance) carries a `caller_is_preview_agent`
   guard, driven in tests by the preview agent's live minted JWT through the
   production router, each with an admin positive control. Budget: grouped
   workspace reservation via `check_spawn_budget_for` by default;
   `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` caps independently when set and fails
   closed on enumeration errors
   (`preview_budget_enumeration_failure_fails_closed`).
5. **GET status + diff** — `get_meta_spec_preview_status` derives per-agent
   state from the real agent rows and computes the diff of the preview branch
   against the pinned base SHA via `state.git_ops.diff` (MR-diff plumbing).
   `preview_agent_finish_teardown_and_diff` runs a real agent process that
   commits and pushes, then asserts Stopped (never Idle), worktree gone,
   branch alive, token revoked, and the diff surfaced with the changed file.
6. **No lingering Active/Idle** — every compute-target monitor routes through
   `on_agent_process_exit` → `finish_preview_agent` (Stopped + teardown);
   agents.rs status changes are refused for preview-agent callers.
7. **DELETE** — kills processes through the recorded backend
   (`spawned_backends`, so ssh-docker/kubernetes kills are not silent local
   no-ops), stops rows, bulk-acks inboxes, removes worktrees, closes MRs on
   preview branches, deletes branches, revokes tokens, releases slots
   exactly-once (durable `released` flag shared with the generic terminal
   paths — stale-agent detector, stop/fail routes, admin kill), removes the
   kv records; 404 on unknown id
   (`preview_delete_tears_down_everything`).
8. **Background GC** — `meta_spec_preview_gc` registered in jobs.rs and
   spawned in main.rs; TTL default 24h (`GYRE_META_SPEC_PREVIEW_TTL_HOURS`),
   interval default 1h; `preview_gc_ttl_boundary` covers the at/past-TTL vs
   just-under boundary plus orphan reclamation.
9. **Clean cutover** — workspace-scoped preview routes removed from mod.rs,
   `StructuralImpact`/`compute_preview_blast_radius` gone, the two
   abac-route-registry exemption entries removed together with their routes
   (removals-only), and `check-abac-route-registry`/`check-abac-exempt-handlers`
   pass on the candidate tree.
10. **Docs** — api-reference.md (routes, shapes, error codes, skip table),
    server-config.md (four new knobs), docker/gyre-agent/README.md (preview
    env contract) all updated.
11. **Tests** — the in-file suite (~30 preview tests) uses real git repos,
    real spawned processes, the production router, live minted JWTs, and
    positive controls; not self-confirming.

## Verifier-overlay change (d28e4a32) — checked for gate weakening

The candidate modified `scripts/verifier-script-overlay.py` so the verifier's
upstream phase judges the merged tree with the merged tree's own frozen
exemption data instead of the base's (which references routes the clean
cutover legitimately removed). Reviewed: the exemption diff is removals-only;
additions relative to base remain forbidden by dev-check.sh, so merged data is
entrywise a subset and can only re-expose findings to the upstream checks; the
accompanying `test_candidate_stale_exemption_entry_still_fails` proves
inconsistent candidate data still fails phase one. Both new tests pass here.
This resolves the prior verification artifact (durable finding ac215fe3)
without weakening any gate.

## Non-blocking observations

- `GYRE_PREVIEW_JWT_TTL_SECS` defaults to 1800s (vs 300s for normal agent
  JWTs). Still short-lived in absolute terms, revoked at completion/DELETE,
  configurable; the spec fixes no number.
- `EditorSplit.svelte` retains a meta-spec preview loop alongside
  MetaSpecs.svelte's own implementation; both target the new global endpoints
  and are test-covered. Duplication, not a defect.
- `web/dist` generated output is verifier-owned at this checkpoint (restored
  to the committed state after the frontend test run rebuilt it locally).

## Required verifier probes (deferred from review sandbox)

- `cargo test -p gyre-server --lib -- preview_`
- `cargo test -p gyre-server --lib` (jobs/rbac/spawn/agents/tasks/orchestrator/
  compose/specs_assist/agent_tracking in-file suites touched by the guards)
- `cargo test -p gyre-server --test api_integration --test auth_integration
  --test e2e_ralph_loop --test git_integration --test m18_oidc_integration`
- `cargo test --all`; web rebuild + `npm ci && npm test`
