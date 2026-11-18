# Review: task-206 — Meta-spec preview mode: real agent preview runs

**Reviewer:** Independent review agent (pi runtime)
**Round:** R6 (repair round 05ab61cb — preview budget slot release exactly-once)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** f70cfac6f829847f994daf0cb96631503b1a1561
**Candidate reviewed:** d1b21c4cd98650c2942e80ccdbd1ee40a4008687
**Status:** complete — approved

---

## Verdict

Approved. The candidate satisfies the task contract and the 05ab61cb repair
correctly closes the preview budget slot double-release class. Independent
probe evidence is saved under `/tmp/stage/review-evidence/`.

## Repair round 05ab61cb — exactly-once slot release (focus of this round)

The prior-round finding was real and structural: every generic terminal path
(`POST /agents/{id}/stop`, `/fail`, admin kill, `budget_stop_agent`,
stale-agent abort) decrements the workspace/tenant `active_agents` counters for
a preview agent, while the preview-owned releases (`finish_preview_agent`,
DELETE teardown, GC sweep) guarded their decrement solely on the preview
record's `released` flag, which no generic path ever set — a systematic
double release, and `decrement_active` is MAX(0, n-1), so each occurrence
permanently biases the counter down (over-admission past
`max_concurrent_agents`).

Verified fix, exhaustively against every `decrement_active_agents` call site
in the tree:

| Call site | Preview-reachable? | Exactly-once pairing |
|---|---|---|
| `api/spawn.rs` `fail_agent` | yes | decrement + `mark_preview_slot_released` (spawn.rs:3352/3361), terminal-state idempotency guard before |
| `api/spawn.rs` `stop_agent` | yes | decrement + mark (spawn.rs:3531/3537), terminal guard before |
| `api/admin.rs` `kill_agent` | yes | decrement + mark under `was_active` (admin.rs:437/446) |
| `api/budget.rs` `budget_stop_agent` | yes | decrement + mark (budget.rs:1494/1502) |
| `stale_agents.rs` `run_once` abort | yes | decrement + mark (stale_agents.rs:87/97) |
| `api/spawn.rs:3059` (complete→MR path) | no | `complete_agent` refuses preview callers AND targets (caller-keyed spawn.rs:2924, target-keyed spawn.rs:2947) |
| `mcp.rs:2193` `complete_via_mr` | no | MCP dispatch refuses `gyre_agent_complete` for preview agents (mcp.rs:4969-4986, `is_preview_agent` check) |
| `signal_chain.rs:482` (orchestrator Idle) | no | orchestrator-session transition; preview agents are never workspace orchestrators |
| `signal_chain.rs:1300` `teardown_failed_spawn` | no | only runs for agents the signal chain itself spawns; preview agents are spawned via `launch_taskless_agent_process` |
| `meta_specs.rs:2136` (`release_preview_slot`) | — | the durable-guarded release itself |

`mark_preview_slot_released` flags the durable `released` bit in both the
per-agent `preview_agents` index entry and the run record's embedded copy
(kept in step; `persist_preview` rewrites the index from the record) without
decrementing. `release_preview_slot` re-checks the durable flag from the
per-agent index before decrementing, so a stale `PreviewAgentRef` snapshot
can no longer double-release; `mark_preview_agent_failed` and
`rollback_preview_agent` route through the same guarded release. The
no-slot-taken rollback paths (`partial.released = true` before any
`increment_active_agents`) still short-circuit correctly — the snapshot flag
remains a sufficient condition for skipping.

The two regression tests (`generic_stop_fail_release_preview_slot_exactly_once`,
`generic_kill_and_budget_stop_release_exactly_once_across_gc`) use the
production REST routes and the real monitor transition
(`crate::api::spawn::on_agent_process_exit`), watch both counter keys
against a live background slot, and assert exactly-one-drop across
stop/fail + exit + DELETE and kill/budget-stop + exit + GC sweep. On the
pre-fix tree the exit-path release had no durable check and the generic
routes set no flag, so the second decrement is observable (counter drops
below the background agent's live slot) — the tests are real killers, not
self-confirming.

## Full-contract verification (cumulative, exact candidate tree)

- **Routes** — global `POST/GET/DELETE /api/v1/meta-specs/preview[/:preview_id]`
  registered (api/mod.rs:882-888); ABAC `RouteResourceMapping` entries present
  (abac_middleware.rs:742-744); no workspace-scoped preview routes remain;
  `check-abac-route-registry.sh` green.
- **Real spawn** — `provision_preview_agent` creates the Active agent row
  (workspace scope from the repo record, never caller-supplied), a
  `preview/{preview_id}/{slug}` branch pinned to the default-branch tip
  (diff base is exact), a real git worktree + worktree row, and a short-lived
  EdDSA JWT (`GYRE_PREVIEW_JWT_TTL_SECS`, default 1800 s); per-failure
  rollback with correct no-slot-taken flags; `persist_preview` failure rolls
  back everything provisioned.
- **Draft injection** — `GYRE_PREVIEW_ID` / `GYRE_META_SPEC_DRAFT_KIND` /
  `GYRE_META_SPEC_DRAFT_CONTENT` / `GYRE_TARGET_SPEC_PATH` travel in the
  agent env; `docker/gyre-agent/agent-runner.mjs` builds the preview prompt
  with the draft meta-spec as governing "how to build" context over the
  target spec (draft wins on conflict; no task, no MR, no
  `gyre_agent_complete`). `GYRE_TASK_ID` deliberately absent.
- **Budget** — workspace budget by default (grouped per-workspace
  `check_spawn_budget_for`, so N targets cannot slip past a workspace cap),
  independent `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` knob when set (429 on
  overflow, counting only unreleased+Active slots).
- **Status + diff** — per-agent state derived from the real agent rows
  (running/complete/failed/stopped/dead/unknown), diff of the preview branch
  against the pinned base SHA via `state.git_ops.diff` (MR-diff plumbing).
- **Completion semantics** — every monitor exit routes through
  `on_agent_process_exit` → `finish_preview_agent`: token revoked, worktree
  removed, slot released, row Stopped — never Idle.
- **DELETE / GC** — full teardown (kill via recorded backend handle, row,
  worktree, MRs on preview branches closed, branch, token, slot, kv records);
  404 on unknown id; `meta_spec_preview_gc` job registered in jobs.rs
  (interval `GYRE_META_SPEC_PREVIEW_GC_INTERVAL_SECS` default 1 h; TTL
  `GYRE_META_SPEC_PREVIEW_TTL_HOURS` default 24 h) plus an orphan pass that
  reclaims index entries whose run record is gone.
- **Clean cutover** — no `compute_preview_blast_radius`/`StructuralImpact` in
  the preview response; blast radius lives in §4's endpoint only.
- **Docs** — `docs/api-reference.md` (routes, shapes, error codes) and
  `docs/server-config.md` (all four knobs) updated.
- **Coverage matrix** — `specs/coverage/system/meta-spec-reconciliation.md`
  §5 marked `implemented` with a description that matches the code.

## Probes run (evidence in /tmp/stage/review-evidence/)

- `cargo check -p gyre-server` (via /tmp/stage/bin/cargo, cold build):
  **exit 0** (`cargo-check-gyre-server.txt`; re-run warm: exit 0 in 0.33 s).
- `bash scripts/check-arch.sh`: exit 0.
- `bash scripts/check-task-commit-attribution.sh`: exit 0 (all task-labeled
  product-surface commits recorded).
- `bash scripts/check-abac-route-registry.sh`: exit 0.
- `check-inert-enforcement`, `check-mem-port-contracts`,
  `check-abac-exempt-handlers`, `check-forwarded-header-trust`,
  `check-forged-scope-fields`, `check-unbounded-external-http`,
  `check-lossy-secret-conversion`, `check-scope-literal-defaults`,
  `check-fabricated-scope-defaults`, `check-byte-slice-truncation`,
  `check-fail-open-ref-resolution`, `check-in-memory-state-stores`:
  all exit 0.
- Frontend: `npm ci` then `npx vitest run src/__tests__/MetaSpecs.test.js`:
  **111/111 passed** (`vitest-metaspecs.txt`).
- Working tree verified clean after probes (web/dist rebuilt as a build.rs
  side effect was restored to the committed state — generated output belongs
  to the verifier).

## Deferred to the verifier (sandbox build-budget constraint)

`cargo test -p gyre-server` (test compilation exceeds this sandbox's 4 GiB
wrapper limit — standing task-206 constraint). Required commands:
`cargo test -p gyre-server preview_` plus
`generic_stop_fail_release_preview_slot_exactly_once` and
`generic_kill_and_budget_stop_release_exactly_once_across_gc`, alongside the
existing suite (`preview_agent_finish_teardown_and_diff`,
`preview_worktree_failure_releases_no_untaken_budget_slot`,
`preview_persist_failure_rolls_back_every_provisioned_agent`,
`mark_preview_agent_failed_kills_registered_process`, GC/DELETE suite,
REST-ceremony refusals, `caller_is_preview_agent_detects_array_scoped_jwt`).

## Non-blocking observations

- **Residual TOCTOU window (inherent to the kv abstraction):**
  `release_preview_slot`'s check-decrement-mark sequence is not atomic — the
  `KvStore` port has no compare-and-swap. Two terminal events for the same
  agent firing within the millisecond-scale window between the durable flag
  read and its write could still double-release. The systematic double
  release (the blocking prior finding) is fixed; the residual race is the
  same class normal agents' generic terminal paths have always had and would
  need a kv CAS to close. Not blocking.
- **Status labeling nuance (reporting-only):** an operator-stopped preview
  agent reports `complete` in GET status, because the `released` flag
  conflates slot release with run completion. No safety decision reads this
  value; the diff/branch state is unaffected.
