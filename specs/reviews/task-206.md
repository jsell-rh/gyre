# Review: task-206 — Meta-spec preview mode: real agent preview runs

**Reviewer:** Review agent (independent)
**Round:** R1 (candidate af5e88ddc4894e08d7ff63d44ed5c42aca5fd792)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** ce3361149826b047fef77ae9ed61f60385855402
**Status:** rejected — one blocking finding (missing enforcement); everything else verified sound

---

## Verdict

**Not approved.** One blocking finding: the preview-agent REST ceremony fence
this change introduces is incomplete, which falsifies the invariant the change
itself documents. All other acceptance criteria are met with strong evidence.

## Blocking finding — incomplete preview-agent ceremony fence on REST mutation routes

The diff adds `caller_is_preview_agent` 403 guards to five REST handlers
(POST/PUT /tasks, POST /merge-requests, POST /merge-queue/enqueue,
POST /agents/:id/complete) and documents the invariant in
docs/api-reference.md:443: *"A preview agent cannot mint ceremony through the
REST surface with its own live JWT."* The guard rationale stated in
api/tasks.rs:190-195 is that `allowedTools` is advisory and the builtin ABAC
policy `builtin-agent-scoped-access` (abac_middleware.rs:1621-1652) grants the
Agent role `write` on `*`.

That same rationale applies to five same-class mutation routes that remain
unguarded, all resolver-mapped to action `write` and thus pass middleware ABAC
for an Agent-role JWT (the middleware authenticates extractor-less routes
itself, abac_middleware.rs:2166):

- `PUT /api/v1/tasks/:id/status` → `transition_task_status`
  (api/tasks.rs:356) — no auth extractor, no preview guard. A preview agent
  can drive real tasks through the full status lattice to `Done`/`Cancelled`,
  which also fires `reconciliation::on_reconciliation_task_terminal` for
  reconciliation-labeled tasks.
- `PUT /api/v1/merge-requests/:id/status` → `transition_mr_status`
  (api/merge_requests.rs:906) — blocks Agent role only from `Approved`.
  `Open→Closed` deletes the MR's gate traces (HSI §3a evidence destruction);
  `Approved→Merged` promotes the gate trace to permanent attestation and
  emits `MrMerged` with no code merged and no queue processing — merge
  ceremony minted through the REST surface with the preview agent's own live
  JWT, exactly what the docs claim cannot happen.
- `POST /api/v1/merge-requests/:id/reviews` (`submit_review`,
  api/merge_requests.rs:1087) and `POST .../comments` (`add_comment`,
  api/merge_requests.rs:1043) — review/comment rows landable on real MRs.
- `DELETE /api/v1/merge-queue/:id` (`cancel_entry`, api/merge_queue.rs:159) —
  queue entries cancellable.

Repro: run a preview (POST /api/v1/meta-specs/preview); the spawned agent
process holds a live Agent-role JWT (`GYRE_AGENT_TOKEN`) scoped to the target
workspace and has shell access through the runner. From inside the agent,
`PUT $GYRE_SERVER_URL/api/v1/merge-requests/<id>/status {"status":"closed"}`
returns 200 and deletes the gate trace; on an approved MR, `{"status":"merged"}`
returns 200, promotes the trace to attestation and emits `MrMerged`. Full
premises and commands: `/tmp/stage/review-evidence/rest-ceremony-fence-gap.md`.

Repair: add `caller_is_preview_agent` guards (403) to `transition_task_status`,
`transition_mr_status`, `submit_review`, `add_comment`, and `cancel_entry`,
with tests mirroring `preview_agent_rest_ceremony_is_refused` (assert 403 plus
absence of side effects, plus a positive control for an authorized caller).
These routes predate this task and are equally open to normal agent JWTs, so
this is an enforcement-completeness gap in the fence this task introduced —
not a regression — but the fence is the task's own shipped invariant
("Skip-ceremony is enforced, not just skipped") and the docs claim is false
as shipped.

## Verified sound (evidence)

Probes run on the exact candidate (logs under /tmp/stage/review-evidence/):

- `cargo check -p gyre-server --lib --tests` — clean, exit 0 (9m38s,
  cargo-check-gyre-server.log).
- All 18 mechanical invariant scripts pass (check-scripts-root.log):
  arch, abac-route-registry (no stale exemptions — the previously-failing
  check from the merge round), abac-exempt-handlers, mcp-write-tools,
  migration versions/SQL-portability, dead-message-kinds,
  byte-slice-truncation, relative-path-defaults, fail-open-ref-resolution,
  mem-port-contracts, fabricated-scope-defaults, lossy-secret-conversion,
  scope-literal-defaults, inert-enforcement, forged-scope-fields,
  forwarded-header-trust, in-memory-state-stores, unbounded-external-http.
- `check-task-commit-attribution.sh` passes.

Acceptance criteria, independently inspected:

- **Spawn is real** — `post_meta_spec_preview` (api/meta_specs.rs:832)
  resolves/authorizes every target up front (repo ABAC + tenant containment +
  spec_path on default branch), then provisions per target: pinned base sha,
  `preview/{preview_id}/{slug}` branch (stem-slug with collision
  disambiguation), real `git worktree`, Active agent row with real
  workspace scope, short-lived scoped JWT. Provisioning is all-or-nothing
  with rollback compensation on every failure path (branch-create, agent-row,
  worktree, worktree-row, mint, token-persist, preview-persist, launch).
- **Draft reaches the agent** — env-only delivery
  (`GYRE_PREVIEW_ID`/`GYRE_META_SPEC_DRAFT_KIND`/`GYRE_META_SPEC_DRAFT_CONTENT`/
  `GYRE_TARGET_SPEC_PATH`, no `GYRE_TASK_ID`); `agent-runner.mjs:309-340`
  embeds the draft verbatim in the prompt as governing context over the
  target spec; server-side `extra_env` merged before server-owned vars so it
  cannot shadow identity/credential injection (spawn.rs taskless launch).
- **202 shape exact** — `{preview_id, agents:[{agent_id, repo_id, spec_path,
  branch}]}`; response structs carry no structural-impact fields.
- **Skip-ceremony** — no task assignment, no spec-approval gate, no gates,
  no MR/queue/provenance, no `refs/agents/*`/`refs/tasks/*` writes; MCP
  dispatch refuses ceremony tools for preview agents; never Idle (every
  monitor exit routes through `finish_preview_agent` → Stopped, token
  revoked, worktree removed, slot released). Budget: workspace budget by
  default with up-front per-workspace batched reservation
  (`check_spawn_budget_for`); `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` caps
  independently when set (429). *Except for the blocking finding above.*
- **Status + diff** — GET derives per-agent state from real agent rows;
  diff = preview branch vs pinned base sha through the same git plumbing as
  MR diffs; `null` until the branch exists.
- **DELETE** — kills via recorded spawn backends (not handle shape — the
  ssh-docker/k8s kill regression is pinned by test), force-removes worktrees,
  closes open MRs on preview branches, deletes branches, revokes tokens,
  drops kv records; 404 unknown id.
- **GC** — `meta_spec_preview_gc` registered (jobs.rs:705, main.rs:151);
  TTL boundary honored (`age >= ttl`), orphan pass reclaims index entries
  whose run is gone.
- **Clean cutover** — hollow structural-impact preview and workspace-scoped
  routes gone from mod.rs; the two stale exemption lines are removed; docs
  updated (api-reference.md, server-config.md, docker/gyre-agent/README.md)
  and match the implemented routes/knobs.
- **Tests are hard** — real git repos in tempdirs, real spawned processes
  through the production launch path, env-dump assertion on the child's
  actual environment for draft injection, end-to-end commit-and-push diff
  test, denial tests that pin error text + assert no side effects + positive
  controls, failure injection at real port boundaries, GC boundary test.
  Notably `preview_agent_rest_ceremony_is_refused` also covers the
  merge-queue enqueue refusal (403 + empty queue) that the module-local
  tests miss.

## Non-blocking observations (for the record, not grounds for rejection)

- `agent-runner.mjs:309-311` falls back to the literal `'unknown'` when
  `GYRE_META_SPEC_DRAFT_CONTENT`/`GYRE_TARGET_SPEC_PATH` are unset instead of
  exiting loudly. Unreachable in the wired flow (server 400s empty drafts and
  validates spec paths), but it is a fail-open pattern per repo convention.
- `EditorSplit.svelte` meta-spec mode duplicates the §9 preview loop already
  implemented in `MetaSpecs.svelte` with divergent poll policy and has no
  production caller (test-only surface).
- kubernetes.rs reports `launched: true` even when the pod was never observed
  scheduled (`let _ = scheduled`, warn-only) — a quota-rejected pod reads as
  a successful preview launch.
- Launch-failure rollback (`mark_preview_agent_failed`) is exercised at unit
  level only; the other failure points have handler-level tests.

## Verifier must run (blocked in this stage by the build wrapper)

- `cargo test -p gyre-server --lib -- preview_`
- `cargo test -p gyre-server --lib -- merge_queue`
- `cargo test -p gyre-server --lib -- uninstrumented_role_guards`
- `cargo test -p gyre-server --lib -- prompt` (+ `notifications`/`briefing`
  suites from the merged task-013 base)
- `cd web && npm ci && npm test`
