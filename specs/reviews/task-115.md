# Review — task-115 (Implement spec approval signal chain & orchestrators)

Spec: `agent-runtime.md` §1 Phases 1–3 (Signal Chain, Workspace Orchestrator, Repo Orchestrator).
Candidate under review: `fef8e0aa9f4b645c655fb52ed4729509700993b5` on branch
`pipeline/task-115/f59aa9894630472eb3c00b3c310471ac-1`, base `fefde2946f3a0af1f212588f54591516fee0f7b2`.
Evidence: `/tmp/stage/review-evidence/task-115-review.md`.

## Verdict: approved (no findings)

### What was verified

**Phase 1 (approve → SpecApproved):** the handler keeps the base's mode-based
approval resolution and fires only on transition INTO Approved; it emits the
bus message with the spec'd payload (`repo_id, spec_path, spec_sha,
approved_by, approval_id`, Workspace destination) and then routes the same
payload through `signal_chain::on_spec_approved`. Mutation probe: disabling
the routing call makes `approve_spec_emits_spec_approved_and_triggers_chain`
fail (missing delegation task) — the end-to-end wiring is real.

**Phase 2 (workspace orchestrator):** `OrchestratorRegistry` provides
per-workspace/per-repo tokio mutexes (Arc-shared, stable identity per scope —
directly tested); spawn-on-demand reuses the task-093 core (scoped JWT,
keypair, budget, durable one-live check). The run reads the approved blob at
the pinned SHA (summary lands in the delegation description — dedicated test
against a real git repo), queries `spec_links`, creates the Delegation task
(`spec_ref: path@sha`, bare `spec_path` so `reject_spec` cancellation reaches
it — dedicated test), Coordination tasks for dependents, and p4
`CrossWorkspaceSpecChange` notifications with the tenant resolved from the
dependent workspace record (skip-and-log, never a fabricated "default").
Inbox ack is targeted (escalations stay unacked — dedicated test), and budget
accounting is balanced (Active→Idle decrements once; Idle→Idle is an invalid
domain transition, so reuse cannot double-decrement).

**Phase 3 (repo orchestrator):** `scheduler_run_once` serializes whole
cycles on a run lock (the admin trigger and the 30 s loop cannot race the
non-atomic claim), claims Delegation/Coordination only (untyped push-hook and
Implementation tasks untouched — dedicated tests), holds the per-repo mutex,
re-reads the task after the lock, and releases failed claims to `Blocked`
for retry (test drives a real budget-capped failure and recovery). Stale
InProgress claims are re-claimed (crash recovery test). Decomposition uses
the LLM (latency-bounded at 120 s; Vertex AI HTTP clients now carry real
`.timeout()` — the unbounded-HTTP exemption file went 5→0) with an honest
deterministic fallback derived from the real spec blob; sub-tasks carry
`spec_ref`, `parent_task_id`, `order`, and `depends_on` chaining, and the
delegation is marked Done. Mutation probes: removing the cycle-lock
acquisition fails `scheduler_cycles_serialize_on_the_run_lock`; skipping the
live-orchestrator check fails `second_approval_reuses_then_respawns`.

**Supporting changes:** `spec_ref` column via migration
`2026-10-10-000057_task_spec_ref` (next unused number, portable SQL, up/down,
wired in both SQLite and PG adapters); SQLite `create` persists
`workspace_id` (regression test covers insert and upsert paths); the
stale-agent detector refreshes system-spawned orchestrators instead of
churning spawn/kill/restart (test proves two consecutive cycles keep exactly
one live orchestrator) and releases the budget slot on Dead for previously
Active agents.

**CI repair (head `787510310`):** the e2e job is pinned to `ubuntu-24.04`
because the locked Playwright 1.58.2 cannot install browsers on
ubuntu-26.04 (durable finding, run 42603492236). Empirically confirmed on
this tree: the locked playwright-core 1.58.2 host registry contains only
ubuntu20/22/24 — no ubuntu26. The pin is the minimal repair; no product
source, assertions, or visual baselines changed. Exact-head GitHub checks
remain for the verification gate.

**Suites:** 18/18 mechanical invariant scripts pass; `cargo test -p
gyre-server --lib` 1504 passed / 0 failed; `gyre-adapters --lib` 384 / 0;
`gyre-domain` 436 / 0; `cargo fmt --check` clean. All 15 frontmatter commits
are ancestors of the candidate; attribution script passes.

### Non-blocking notes (documented design decisions, accepted)

- The workspace orchestrator's Phase-2 impact analysis is mechanical (direct
  `spec_links` query) rather than an LLM judgment call; every enumerated
  Phase-2 step is implemented and the persona-scoped agent record exists.
  The repo orchestrator's decomposition does use LLM judgment.
- A Delegation task with no spec reference at all completes as a no-op with
  a warn (avoids infinite retry of permanently malformed input); the primary
  chain path always pins a real `spec_ref`, and empty-SHA refs hard-fail to
  `Blocked` (dedicated tests).
- The repo orchestrator stays live after completing (documented) rather than
  terminating, so future coordination tasks in the same repo reuse it under
  the per-repo mutex.
