# Review — task-157 (Implement meta-spec rollout policies)

Spec: `specs/system/meta-spec-reconciliation.md` §7 Rollout Policy (live spec:
`RolloutPolicy { strategy, max_concurrent_agents, priority, auto_merge }`,
strategies Immediate/Rolling/Background, default Rolling / Normal /
auto_merge false — NOT the task excerpt's immediate/rolling/manual with
repo-count auto-selection; the spec file is byte-identical to base
`124a852be`, untouched by this candidate, so following the live spec is the
correct call per "specs are the contract").

Base `124a852be` → candidate `1ca46e9e7` (product commits `a1ff51f2c`,
`e67c46719`, `aa73b69a5`). Verdict: **needs revision** — one reproduced
defect (F1); everything else verified real and correct.

## Verification performed (evidence in /tmp/stage/review-evidence/task-157-*)

- `cargo test -p gyre-domain meta_spec` → 6 passed (default, wire-name
  round-trips, priority mapping, full JSON shape).
- `cargo test -p gyre-server --lib reconciliation::tests` → 28 passed.
- `cargo test -p gyre-server --lib api::meta_specs` → 25 passed (GET default,
  PUT round-trip, partial update, real developer-JWT 403, invalid
  strategy/priority/batch-size 400, unknown workspace 404).
- `cargo test -p gyre-adapters rollout_policy` → 5 passed (SQLite adapter:
  missing/upsert/overwrite/delete/domain round-trip).
- Mutation probe (restored; tree clean at candidate after): hardcoding an
  Immediate policy in `run_reconciliation` (replacing the
  `get_workspace_rollout_policy` load) makes all 5 strategy tests FAIL
  (rolling ×3, background, priority mapping) — the tests are genuinely
  anchored to the policy load, not self-confirming.
- `scripts/check-migration-versions.sh` OK — `2026-11-06-000057` derives
  version `20261106000057`, distinct from `2026-10-08-000057`'s
  `20261008000057`; portable SQL (no dialect-only constructs).
- `scripts/check-abac-route-registry.sh` OK — both rollout-policy routes
  registered in the resolver (`meta_spec` resource), no new exemptions;
  `git diff base..candidate -- scripts/` is empty (no gates weakened).
- `scripts/check-arch.sh` OK — RolloutPolicy in gyre-domain, port in
  gyre-ports, adapters in gyre-adapters, wiring in server.

## Verified correct (no findings)

- **Spec fidelity**: implementation matches live §7 exactly — three
  strategies with the specced semantics; Immediate creates all tasks with
  the §6 Delegation signal; Background creates tasks without `task_type`
  so they sit in Backlog and cannot spawn agents (the `spawn_agent_core`
  gate at api/spawn.rs:384-389 rejects task_type-less spawns — verified);
  Rolling batches by `max_concurrent_agents`; priority maps
  High/Normal/Low → TaskPriority High/Medium/Low; auto_merge enqueues
  reconciliation MRs into the merge queue at agent completion (priority 50
  matches the explicit enqueue default, api/merge_queue.rs:64) — gate
  passage still required by the merge processor.
- **Persistence is real**: `RolloutPolicyRepository` port + SQLite/Postgres
  adapters + mem adapter, wired via the `store!` pattern; an unset policy
  is the spec default; PUT is Admin-only in-handler with real-JWT 403 test.
- **Terminal hook coverage**: all three terminal-transition surfaces (REST
  `transition_task_status`, REST `archive_repo`, MCP `update_task`) call the
  shared `on_reconciliation_task_terminal`, which advances the rollout
  before the §11 completion check — `ReconciliationCompleted` fires exactly
  once at rollout end (tested). The general PUT /tasks/:id cannot change
  status; no other production surface moves reconciliation tasks to
  terminal.
- **Tests are hard**: concrete observable state assertions (task counts,
  pending KV state, statuses, task_type, message counts), confirmed
  discriminating by mutation probe.

## F1 (reproduced): overlapping waves lose the first wave's deferred remainder

`persist_pending_rollout` writes the rolling remainder under a
per-workspace singleton KV key (`rollout_pending` / workspace_id) with an
unconditional `kv_set`, and `run_reconciliation` has no in-flight guard.
When a second meta-spec set change triggers a new wave while a rolling
rollout is in flight (realistic: rolling waits for each batch by design,
and Rolling is the default), the second wave's persist overwrites the first
wave's remainder; those deferred tasks are never created — silent work
loss, no error or warning, repos left unreconciled against the superseded
spec change.

Reproduction (temporary test on the exact candidate, output at
/tmp/stage/review-evidence/task-157-probe-overlapping-waves.txt):
4 repos, rolling batch 1; wave A for `backend-developer` v4 → 1 task + 3
deferred; wave B for `code-reviewer` v2 before wave A completes; drive both
rollouts to completion → persona tasks = **1/4**, reviewer tasks = 4/4.
Three repos never received the persona reconciliation task.

Fix direction: merge the new remainder into any existing `PendingRollout`
on persist (dedup by title/repo, as the advance path already does), or key
pending state per wave.

## Observations (not findings, no action required for this task)

- The spec-reject path (api/specs.rs:1043) cancels in-flight tasks by
  spec_path without calling the terminal hook. Pre-existing at base (same
  gap for §11 completion, task-156 scope) and practically unreachable for
  reconciliation tasks (their spec_path is a meta-spec *name*, a different
  namespace from spec-ledger paths).
- Crash-window stall: if the process dies between a task's terminal
  `tasks.update` and the hook, the persisted remainder survives (restart
  safe) but no advance fires until another reconciliation task goes
  terminal. Inherent edge; the remainder is not lost.

— Reviewer, 2026-11-06
