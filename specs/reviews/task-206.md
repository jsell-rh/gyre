# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Independent review (pi runtime)
**Round:** R1 (candidate a8121f4a02149c147b9aa6fdf1c7465749ced5d9)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** 876c77b73a5e987f4cbf3a1bf4b20c2a71c626f9
**Status:** findings — one blocking containment gap; core implementation verified real

---

## What was verified as real (independently probed)

The preview mode is a genuine agent run path, not an estimate. Evidence for
each acceptance criterion (probe outputs under `/tmp/stage/review-evidence/`):

- **Spawn** — `POST /api/v1/meta-specs/preview` (global route, Admin/Developer,
  per-target repo ABAC + tenant containment, spec_path existence check) creates
  a real `Active` agent row per target, a `preview/{preview_id}/{slug}` branch
  pinned to the default-branch tip, a real git worktree, and a scoped
  short-TTL JWT (`GYRE_PREVIEW_JWT_TTL_SECS`, default 1800 s). 202 shape is
  exactly `{preview_id, agents:[{agent_id, repo_id, spec_path, branch}]}`.
  All-or-nothing provisioning with tested rollback on worktree/persist failure.
- **Draft delivery** — draft travels out-of-band in the process environment
  (`GYRE_PREVIEW_ID`, `GYRE_META_SPEC_DRAFT_KIND`, `GYRE_META_SPEC_DRAFT_CONTENT`,
  `GYRE_TARGET_SPEC_PATH`; `GYRE_TASK_ID` deliberately absent — asserted
  negatively). `docker/gyre-agent/agent-runner.mjs` builds the preview prompt
  from those vars verbatim and withholds task/MR/complete tools client-side.
- **Skip-ceremony** — enforced server-side, not just advisory: MCP dispatch
  refuses `gyre_create_task`/`gyre_update_task`/`gyre_create_mr`/
  `gyre_agent_complete`/`conversation_upload` for preview agents; REST
  `POST /tasks`, `PUT /tasks/:id`, `POST /merge-requests`,
  `POST /merge-queue/enqueue`, `POST /agents/:id/complete` return 403 for
  preview-agent callers. No MR/task/provenance/`refs/agents/*`/`refs/tasks/*`
  side effects (asserted in tests). Budget: workspace budget by default
  (per-request N-slot reservation via new `check_spawn_budget_for`),
  `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` caps preview spawns independently (429).
- **Completion** — every dispatch arm registers a monitor;
  `monitor_poll_once` → `on_agent_process_exit` → `finish_preview_agent`:
  token revoked, worktree force-removed, budget slot released (idempotent via
  the `released` flag), agent lands `Stopped` — never `Idle`. The
  `launched` flag is the dispatch's authority (pid-less SshDocker/K8s handles
  handled correctly, with dedicated regression tests using fake ssh/kubectl).
- **Status + diff** — `GET /api/v1/meta-specs/preview/{preview_id}` reports
  per-agent state from the real agent rows and the produced diff via the same
  `git_ops.diff` plumbing as the MR endpoints (pinned base SHA).
- **Cleanup** — `DELETE` kills processes through the recorded backend (not
  handle-shape guessing), force-removes worktrees, closes MRs dangling off
  preview branches, deletes branches, revokes tokens, drops kv records; 404 on
  unknown id. Background `meta_spec_preview_gc` job (interval
  `GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS` default 1 h; TTL
  `GYRE_META_SPEC_PREVIEW_TTL_HOURS` default 24 h) with a TTL-boundary test
  and an orphan pass.
- **Clean cutover** — hollow structural-impact preview, workspace-scoped
  routes, and their ABAC exemption entries are gone; web UI migrated to the
  new global routes (no stale `previewPersona` references); docs updated and
  consistent with code defaults.

Probes: `cargo check -p gyre-server --lib --tests` clean (7m27s); `cargo fmt
--check` clean; all 19 mechanical `scripts/check-*.sh` pass (commit
attribution verified against the candidate's own task file — the working-tree
failure is the review stage's reset of task-206.md, not a defect); full web
suite 125 files / 2513 passed / 0 failed. gyre-server test execution is
deferred to the verifier (wrapper blocks server test compilation in review
stage); the full deferred list is in `/tmp/stage/review-evidence/summary.md`.

---

## Findings

### F1 (blocking) — Preview-agent REST containment is incomplete: task-status, agent-status, MR-status and merge-queue-cancel mutations remain reachable by a live preview-agent JWT

The task's own invariant (code comments in tasks.rs:192-195, test
`preview_agent_rest_task_ceremony_is_refused` at meta_specs.rs:4925) is that
skip-ceremony must hold on the REST surface too, because "a preview agent's
live JWT authenticates as UserRole::Agent and the builtin agent-scoped-access
policy grants 'write' on '*'". Guards were added for POST /tasks,
PUT /tasks/:id, POST /merge-requests, POST /merge-queue/enqueue and
POST /agents/:id/complete — but the same escape class remains open on:

- `PUT /api/v1/tasks/:id/status` → `tasks::transition_task_status`
  (api/mod.rs:359, tasks.rs:359) — no auth extractor, no preview guard. A
  preview agent can transition any task's status; on Done/Cancelled of a
  reconciliation/drift-labeled task it fires
  `on_reconciliation_task_terminal` (tasks.rs:398-403), advancing §7 rollouts
  — durable production automation triggered by throwaway scratch work that
  §5 says must leave nothing behind.
- `PUT /api/v1/agents/:id/status` → `agents::update_agent_status`
  (api/mod.rs:302, agents.rs:266) — a preview agent can flip itself back to
  `Idle`, the exact state `finish_preview_agent` exists to prevent
  ("an Idle preview agent could be re-targeted onto real work",
  meta_specs.rs:1096-1099).
- `PUT /api/v1/merge-requests/:id/status` (merge_requests.rs:906) and
  `DELETE /api/v1/merge-queue/:id` (merge_queue.rs:159) — MR/queue ceremony
  mutations reachable by a preview-agent caller.

Reachability chain verified statically (see
`/tmp/stage/review-evidence/summary.md`): route → unguarded handler →
ABAC `("task"|"agent"|"merge_request"|"merge_queue", "write")` → builtin
`agent-scoped-access` Allow (write, `*`). The task's own F2 test proves a
preview JWT passes ABAC and reaches task-route handlers. Fix: extend the
`caller_is_preview_agent` guard to these handlers (and mirror
`gyre_update_task`-class MCP coverage where applicable), plus one hard test
per route in the style of `preview_agent_rest_task_ceremony_is_refused`
(preview JWT → 403, admin positive control, no ledger mutation).

### Non-blocking observations (for the repair round; not verdict findings)

1. `finish_preview_agent` returns false on a kv `kv_get` error → the exited
   preview agent takes the normal Active→Idle transition until the GC sweep.
   Documented fail-open, bounded by TTL.
2. GC orphan pass treats a transient `kv_get` Err as "run gone" and tears down
   a possibly-live run (fail-destructive on store hiccups).
3. Server restart orphans running preview processes until the TTL GC sweep
   (in-memory monitors — same pre-existing property as normal spawns).
4. `monitor_poll_once` treats an `is_alive` probe error as exit
   (`.unwrap_or(false)`): a momentary docker-daemon outage can prematurely
   tear down a live preview agent. Pre-existing monitor behavior inherited
   by preview.
5. Draft content rides the agent process environment (visible via
   `docker inspect`/remote `ps` in ssh-bare mode) — mirrors the existing
   GYRE_META_SPEC_PROMPT pattern; documented tradeoff.

## Verdict

Not approved on F1. Everything else in the task contract is real, tested, and
independently verified. F1 is a small, precisely-scoped repair (per-handler
guards + tests) in the exact class this task already treats as mandatory.
