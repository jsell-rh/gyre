# Review — task-157 (Implement meta-spec rollout policies)

Spec: `specs/system/meta-spec-reconciliation.md` §7 Rollout Policy. Candidate
`fe7beea54561b7170f9a39abacbeba58a6759402` on base
`124a852bedb4e5759bf143b6c2683df6eeb03849` (branch
`pipeline/task-157/c9dc5a11ff704bd7bf88effc376843d5-1`). Scoping per the
`commits:` frontmatter: `a1ff51f2c`, `e67c46719`, `aa73b69a5`, `de208c147`,
`9caa8566f` (all task-labeled product commits on the branch are listed;
the rest are process/merge commits).

Verdict: **needs revision** (1 finding).

## Spec fidelity (resolved in the candidate's favor — verified)

The task excerpt describes immediate/rolling/manual strategies with batch
delays and repo-count auto-selection; the LIVE spec §7 (read directly; NOT
modified in this diff) defines `RolloutPolicy { strategy, max_concurrent_agents,
priority, auto_merge }` with Immediate/Rolling/Background, default
Rolling/Normal/auto_merge false, and no repo-count auto-selection. Following
the live spec over the stale excerpt is correct per the goal rules ("specs are
the contract"). Domain types, defaults, and wire names match the live spec
exactly (`gyre-domain/src/meta_spec.rs`).

## Probes (evidence: /tmp/stage/review-evidence/task-157-review-fe7beea54.txt)

- `cargo test -p gyre-server --lib reconciliation::tests` → 32 passed.
- `cargo test -p gyre-server --lib api::meta_specs` → 25 passed.
- `cargo test -p gyre-domain --lib` → 430 passed (rollout-policy subset
  round-trips wire names, spec default, priority mapping).
- `cargo test -p gyre-adapters rollout_policy` → 5 passed (real SQLite on a
  temp file incl. domain-policy JSON round-trip).
- All 17 mechanical check scripts PASS (arch, ABAC route registry, mem port
  contracts, migration versions — Diesel version `20261106000057` includes the
  date prefix so it is unique — dead message kinds, in-memory state stores,
  MCP write tools, exempt handlers, byte-slice, relative paths, fail-open
  refs, fabricated/scope defaults, inert enforcement, lossy secrets,
  forwarded headers, forged scope fields).
- `check-rustfmt-diff.py 124a852be` → changed lines clean (20 files).
  `check-clippy-diff.py` defers to verification (workspace-wide build); a
  focused clippy over gyre-domain/-ports/-adapters shows zero diagnostics in
  the new rollout-policy files.

## Verified working (no findings)

- **Types & persistence**: `RolloutPolicy`/`RolloutStrategy`/
  `ReconciliationPriority` in `gyre-domain` with the spec default; new
  `RolloutPolicyRepository` port with SQLite + Postgres + mem adapters,
  portable migration `2026-11-06-000057_rollout_policies`, wired via the
  `store!` pattern (restart-safe, not an in-memory state-store violation).
- **Immediate/Background dispatch** in `run_reconciliation`: Immediate creates
  all tasks with the Delegation signal; Background creates all tasks WITHOUT
  `task_type` — verified `spawn_agent_core` (api/spawn.rs:366-386) rejects
  tasks without a task_type, so Background tasks are genuinely inert.
- **Rolling batches**: first batch of `max_concurrent_agents`, remainder
  kv-persisted; the shared `on_reconciliation_task_terminal` hook (wired into
  all three terminal surfaces: REST task transition, REST repo archive, MCP
  update_task) advances before the §11 completion check, so
  `ReconciliationCompleted` fires exactly once at rollout end. Overlapping-wave
  merge (`merge_pending_rollout`) is lossless and fail-closed on read failure —
  both regression tests pass and the semantics hold.
- **auto_merge**: enqueues reconciliation MRs at agent completion into the
  merge queue with gate execution triggered; gate passage still enforced by
  the merge processor. Default false leaves manual enqueue. Non-reconciliation
  MRs ignored.
- **API**: GET/PUT `/api/v1/workspaces/:id/meta-specs/rollout-policy`
  registered in the ABAC resolver (`meta_spec` resource), PUT is Admin-only
  with validation (unknown strategy/priority → 400, `max_concurrent_agents < 1`
  → 400), partial updates merge with current values, `rollout_policy_updated`
  event emitted, 404 on unknown workspace. Developer-role 403 test present.
  `docs/api-reference.md` updated.
- **Priority mapping**: High→High, Normal→Medium, Low→Low onto `TaskPriority`,
  which the repo-orchestrator surface (RepoMode) orders by.

## Finding F1 — Rolling rollout stalls behind inert Background tasks

**File**: `crates/gyre-server/src/reconciliation.rs`
(`maybe_advance_rolling_rollout`, the `open_reconciliation` check).

The next-batch gate counts ANY non-terminal reconciliation-labeled task as
"batch in flight". §7 Background tasks are created without `task_type` and sit
in Backlog **by design** until the repo orchestrator engages them —
potentially indefinitely. Once a workspace has parked Background
reconciliation tasks, every subsequent (or switched-to) Rolling rollout stalls
after its first batch: the deferred remainder stays in kv pending state and is
never created.

Reproduced on the candidate with a temporary probe test (source restored;
tree verified clean; evidence:
`/tmp/stage/review-evidence/task-157-background-stall-probe.txt`):
Background wave parks 3 tasks → policy switched to Rolling → new wave creates
batch 1 (1 task + 2 deferred) → batch 1 driven to Done →
`on_reconciliation_task_terminal` runs → **no next batch created**
(`reviewer_open=0`, both deferred tasks remain pending).

Why this is a defect, not a design choice:

- Contradicts §7 Rolling: "Wait for each batch to complete before starting the
  next" — the rollout's own batch completed, but the wait condition is "no
  reconciliation-labeled task in the workspace is non-terminal", which parked
  inert tasks satisfy forever.
- Internally inconsistent with `run_reconciliation`, which starts a new wave's
  first batch with NO in-flight check — so "a workspace still reconciling is
  not ready for more" is not the shipped trigger-time semantics either.
- No escape hatch: the mid-rollout strategy-switch flush also lives inside the
  never-running advance, and the daily §10 conformance sweep creates
  drift-review tasks, not rolling batches. Only terminal-ing the parked
  Background tasks resumes the rollout.
- Reachable via the public API with no races: PUT rollout-policy
  (background) → approval wave A → PUT rollout-policy (rolling) → approval
  wave B → wave B's batch 1 completes → stalled.

Repair direction: gate the advance on the rollout's own in-flight work (e.g.
non-terminal reconciliation tasks carrying a `task_type` — i.e. actually
dispatched for agent work), not on inert Backlog entries, plus a regression
test covering Background-history + Rolling rollout proceeding after its own
batch completes.

## Notes for verification (deferred)

- Workspace-wide suite, all-target Clippy, and full `check-clippy-diff.py`
  belong to verification (the diff-scoped clippy gate refuses to run in the
  review sandbox).
- `specs/coverage/system/meta-spec-reconciliation.md` §7 is marked
  `implemented`; with F1 open, the Rolling strategy does not proceed reliably
  in a workspace with Background history — the section should not be
  considered closed until F1 is repaired.
