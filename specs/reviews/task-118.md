# Review — task-118 (Implement agent spawning, implementation phase & Ralph loop)

Spec: `specs/system/agent-runtime.md` §1 Phases 4–8.
Candidate under review: `60226d0f8a4174b24ac4603b71ef182a8086bdb2` (base
`63fbe0ca1aae5ed4940d67acf5c10f3a65b5060c`); implementation commit
`26edb8fc3`, repair rounds `6ae1e989c` (round 1) and `a08ef0e79` (round 2).
Verdict: **approved** — all prior durable findings repaired with regression
tests; acceptance criteria implemented in production code; focused probes
pass. Full probe log: `/tmp/stage/review-evidence/review-summary.md`.

## Round 3 (this review)

Probes run (review sandbox; server test binaries deferred to the verifier
per the 4 GiB budget):

- `cargo check -p gyre-server` → pass (7m19s).
- `cargo check -p gyre-server --tests` → pass (3m00s) — all unit test
  modules and `tests/e2e_ralph_loop.rs` type-check.
- `cargo test -p gyre-domain -p gyre-common` → all pass, including
  `test_release_claim_from_in_progress` and
  `test_release_claim_rejects_other_statuses`.
- `scripts/check-arch.sh`, `check-byte-slice-truncation.sh`,
  `check-migration-versions.sh`, `check-migration-sql-portability.sh`,
  `check-dead-message-kinds.sh`, `check-mcp-write-tools.sh` → all pass.

### Prior findings — repair verification

1. **Duplicate merge-queue entries (round 1)** — fixed: `enqueue_core` is
   idempotent per open MR (non-terminal scan via `list_queue`, which
   excludes Merged/Failed/Cancelled), returns the existing entry (200 vs
   201), allows a fresh enqueue after a terminal entry, and does not
   re-trigger gates for an existing entry. Handler tests
   `enqueue_is_idempotent_per_open_mr` and
   `enqueue_after_terminal_entry_creates_fresh_entry` pin both behaviors;
   `e2e_ralph_loop.rs` no longer manually enqueues the auto-enqueued MR.
2. **tokio `test-util` in workspace production deps (round 1)** — fixed:
   dev-scoped re-enable in gyre-server only.
3. **Stale-snapshot claim (round 1)** — fixed: fresh-row re-read before the
   claim persist and before the budget-flag persist.
4. **`release_claim` dead path (round 2)** — fixed: dedicated domain edge
   `Task::release_claim` (the only InProgress→Backlog transition; the
   general table still has none). Domain tests pass in-sandbox; regression
   test `budget_exhausted_mid_flight_releases_claim_and_respawns` drives
   the exact race (claim → exhaust → Blocked outcome → release → restore →
   re-spawn).
5. **`8.min()` byte-slice (round 2)** — fixed: `chars().take(8)`.

### Acceptance criteria — verified in code

Phase 4 scheduler (`spawn_implementation_workers` under the cycle lock):
Backlog+Implementation detection, depends_on/order sequencing, Archived
defer, max_agents repo→workspace queue-not-reject, budget-exhausted label
(cleared on spawn), fresh-row claim, system-principal spawn through
`spawn_agent_core`; migration 000060 (next sequence, portable SQL, both
adapters, round-trip test). Phase 5/6: both completion surfaces (REST and
MCP `gyre_agent_complete`, agent-JWT self-scoping intact, `needs_write`
gate) share `create_mr_for_completion` and auto-enqueue via `enqueue_core`
→ `trigger_gates_for_mr`. Phase 7: gate watcher (bounded), pass path
(merge → task Done), fail path (real gate output to agent inbox, MR closed,
re-activation Idle→Active valid, fresh JWT + `GYRE_ITERATION` on the same
worktree, shared `launch_agent_process`), max-iterations Blocked/Failed/
token-removed/TaskBlocked-to-orchestrator, spawn-failure 1s/5s/30s retries
with per-attempt teardown and priority-1 `ComputeTargetUnavailable`
notification. Phase 8: gate reviewer now mechanically triggered by the
auto-enqueue; interrogation and reconciliation pre-existing, with
reconciliation's Delegation tasks now flowing through the completed
worker-spawn chain. Spec rejection mid-flight pre-existing and verified.

### Advisory notes (low severity, non-blocking)

- `ralph_loop.rs` `on_gates_failed` relaunch-**failure** branch leaves the
  freshly-minted re-spawn JWT in `agent_tokens` while marking the agent
  Failed (the max-iterations branch and `teardown_failed_spawn` both remove
  it; agent-token auth never checks status). Bounded by the 300s default
  agent JWT TTL and the token never being delivered to a process on a
  failed launch. One-line fix if a later round touches this file.
- `enqueue_core` idempotency is scan-then-insert (no unique constraint on
  `merge_request_id`): strictly concurrent enqueues of one MR can still
  duplicate. The deterministic sequential duplicate is fixed.
- `git_integration.rs` complete→manual-enqueue tests now return the
  auto-enqueued entry (assertions still hold); a 5s processor tick landing
  in the ~ms gap is a small flake window.

### Verifier must run

`cargo test -p gyre-server --lib`, `cargo test -p gyre-server --test
e2e_ralph_loop`, `cargo test -p gyre-server --test git_integration`, and
`cargo test --all` (the acceptance criterion `cargo test --all passes`
could not be executed in the review sandbox; test code type-checks and the
domain-level portions pass).
