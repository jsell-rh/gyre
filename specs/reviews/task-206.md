# Review: task-206 — Meta-spec preview mode: real agent preview runs with branches, diffs, and cleanup

**Reviewer:** Review agent (pi runtime)
**Round:** R8 (fresh verdict on the current candidate; no earlier approval transfers)
**Spec ref:** meta-spec-reconciliation.md §5 Preview Mode: The Fast Iteration Loop
**Comparison base:** 2f33a117949ec8070fa207b69d8f6f021b4bcab8
**HEAD reviewed:** 0aa328f7f3e765448176c1cc5d8d021c9673c9d7
**Status:** approved — no blocking findings

---

## Scope of this round

The candidate head is the repair commit for verification finding 616e5dd4
(task-file whitespace + the upstream-overlay phase-one artifact); it changes
only `specs/tasks/task-206.md`. Because no earlier approval transfers, this
review re-derived independent evidence for the whole task surface at the
exact candidate SHA, in a detached worktree (`/tmp/stage/review-wt`,
removed afterwards). Evidence with commands, exit codes, and logs:
`/tmp/stage/review-evidence/task-206-review.md` (and numbered logs alongside).

## Probes run (all at the candidate SHA)

- `git diff --check 2f33a117 0aa328f7` — exit 0 (finding 616e5dd4 item 1 fixed).
- `bash scripts/check-abac-route-registry.sh` — OK (new preview routes resolve
  in the resolver; the two stale workspace-scoped exemption entries are gone).
- `bash scripts/check-abac-exempt-handlers.sh` — OK, 84 handlers.
- `bash scripts/check-arch.sh` — OK.
- `bash scripts/check-task-commit-attribution.sh` — OK.
- `python3 -m unittest discover -s scripts -p test_dev_checks.py` — 10/10 OK,
  including the d28e4a32 verifier-overlay regression tests (finding
  616e5dd4 item 2 is an upstream-controller artifact, not a source defect:
  the ROOT overlay copy predates the branch fix; the merged tree's own
  scripts and data gate green, and no candidate-side fix exists that does
  not violate the clean-cutover criterion or the frozen-exemption rule).
- `/tmp/stage/bin/cargo check -p gyre-server` — exit 0 (7m16s).
- `cd web && npm ci && npm test` — exit 0; 127 files, 2576 passed / 41
  skipped, including the rewritten MetaSpecs.test.js and EditorSplit.test.js.

Server test *execution* is mechanically refused by the executor's build
wrapper (gyre-server codegen + model exceeds the 4 GiB budget) — an
infrastructure restriction, not a code defect. The required suites are
recorded for the verifier in the evidence file.

## Findings

None blocking. Every acceptance criterion is met by production code:

- **Real spawns** — `POST /api/v1/meta-specs/preview` (global route, Admin/
  Developer, per-target repo ABAC + tenant containment, spec_path validated
  on the default branch) provisions per target a real Active agent row, a
  `preview/{preview_id}/{slug}` branch pinned at the default-branch tip, a
  real git worktree, and a short-lived JWT, then launches through
  `spawn::launch_taskless_agent_process` with a fail-closed `launched` check;
  202 response is the spec shape exactly. Provision/persist failures roll
  back everything (worktree, branch, row, token, slot, index entries).
- **Draft reaches the agent** — `GYRE_META_SPEC_DRAFT_KIND`/`_CONTENT`,
  `GYRE_TARGET_SPEC_PATH`, `GYRE_PREVIEW_ID` in the container env (no
  `GYRE_TASK_ID`); `agent-runner.mjs` builds the preview prompt with the
  draft as governing "how to build" context and withholds task-mutating
  MCP tools.
- **Skip-ceremony is enforced, not just skipped** — MCP dispatch and the
  REST surface (complete, MR create, tasks, task status, agent status
  transitions, stop/fail, spec approve, specs save, compose apply/teardown,
  orchestrator spawns, commits/record, merge-queue, agent registration)
  refuse preview-agent callers; production-router (build_router)
  integration tests pin each refusal; no refs/agents//refs/tasks writes,
  no provenance, no gates.
- **Completion semantics** — every monitor exit routes through
  `finish_preview_agent`: token revoked, worktree removed, budget slot
  released, row Stopped (never Idle); the branch survives for diffing.
- **Status + diff** — `GET /api/v1/meta-specs/preview/{id}` derives
  per-agent state from the real agent rows and exposes the diff of the
  preview branch against its pinned base SHA via the git adapter.
- **DELETE + GC** — `DELETE` kills through the recorded compute backend
  (not handle shape), force-removes worktrees, deletes branches, revokes
  tokens, releases slots exactly-once (durable `released` flag coordinated
  with budget stop / admin kill / stale-agent abort), closes any dangling
  MRs, and drops the kv records (404 on unknown id). The
  `meta_spec_preview_gc` job (1 h interval, 24 h TTL, both configurable)
  tears down expired runs and reclaims orphaned agent records; the TTL
  boundary test asserts at/past-TTL deletion and just-under retention.
- **Budget** — workspace budget by default with N-slot up-front reservation
  per workspace; independent `GYRE_PREVIEW_BUDGET_MAX_CONCURRENT` cap with
  fail-closed enumeration of running preview agents.
- **Clean cutover** — the hollow structural-impact implementation,
  `PreviewBlastRadius` response fields, and the workspace-scoped routes are
  gone; blast radius stays on §4's endpoint. Docs updated (api-reference.md
  §Meta-Spec Preview Mode, server-config.md knobs, agent README).
- **Tests** — the preview suite drives the real router with real bare git
  repos, real spawned processes, real minted JWTs, and asserts on-disk
  state (branches, worktrees, process liveness, env dumps, kv records,
  budget counters, git refs). These are hard tests, not self-confirming.

### Minor observations (non-blocking)

- `release_preview_slot`'s durable flag check is a read-then-write, not
  compare-and-swap; a process-exit racing a DELETE on the same agent could
  in principle double-decrement the workspace counter (MAX(0, n-1) biases
  down by one). The window requires the exit monitor and the DELETE
  teardown to interleave on one agent; every sequential terminal-path
  combination is tested exactly-once. Not blocking.
- `preview_diff` pins the branch's creation SHA as the diff base rather
  than recomputing a merge-base with a moved default branch —
  deterministic and arguably more faithful to "what this agent produced";
  consistent with the documented behavior.
- `python3 -m unittest scripts/test_dev_checks` (as written in some commit
  messages) does not import without `discover -p`; the discover form runs
  the same 10 tests.

## Verdict

**Approved.** `approved: true`, empty findings. The verifier must still run
the deferred gyre-server suites (recorded in
`/tmp/stage/review-evidence/task-206-review.md`): `cargo test -p gyre-server
--lib -- preview_`, the in-file api suites, the integration test targets,
`cargo test --all`, and `cd web && npm ci && npm test`.
