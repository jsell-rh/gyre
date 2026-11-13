---
title: "Implement gate chain configuration and gate agent lifecycle"
spec_ref: "agent-gates.md §Gate Chains"
depends_on:
  - task-134
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Gate Chains (Composable Policies)"
  - "agent-gates.md §Gate Agent Lifecycle"
commits: ["089cdce388806ffd3c3514859f73148488c3c3a5", "2999bb5e7b9989eaf0518ecb4b484ecb2228b188", "6ccbdff179e7e559c35c08e5072b53b4590dd29c", "d715552208fab61ea2e1e241e36d0f86074ae2b0", "583c4a07dde4629da277a1efdafaa5b077596b9d"]
---

## Spec Excerpt

From `agent-gates.md` §Gate Chains (Composable Policies):

Repos configure gate chains as ordered policies:

```toml
[[gates]]
name = "tests"
type = "TestCommand"
command = "cargo test"
required = true

[[gates]]
name = "security-review"
type = "AgentReview"
persona = "personas/security.md"
required = true

[[gates]]
name = "peer-approval"
type = "RequiredApprovals"
min_approvals = 1
required = true
```

**Evaluation order:**
- All gates run in parallel
- Merge processor waits until all required gates pass
- If any required gate fails, MR is blocked and author agent is notified with failure details

**Optional gates:**
- Marked `required = false` for advisory-only checks
- Results recorded but don't block merging

From §Gate Agent Lifecycle:
- **Ephemeral and single-purpose** — spawned for one review, torn down after
- **Read-only access** — can't push, only review
- **Identity-scoped** — OIDC token only permits review submission, not code changes
- **Attributed** — review shows which gate persona produced it
- **Context windows captured** for audit

## Implementation Plan

1. **Gate chain configuration model:**
   - Support ordered list of gates per repo (existing `gates` table)
   - Each gate has: name, type, required flag, type-specific config
   - TOML/JSON gate chain configuration on repo settings

2. **Parallel gate execution in merge processor:**
   - When MR enters merge queue, trigger all configured gates simultaneously
   - TestCommand/LintCommand: spawn shell commands
   - AgentReview: spawn review agents (per task-134)
   - RequiredApprovals: check review count
   - Track gate status per MR: Pending → Running → Passed/Failed

3. **Gate status tracking:**
   - Per-MR gate status stored in database
   - Merge processor polls status each cycle
   - All required gates Passed → proceed with merge
   - Any required gate Failed → block, notify author
   - Any Pending/Running → requeue, check next cycle

4. **Optional gate handling:**
   - `required = false` gates run but don't block
   - Results recorded in gate status for visibility
   - Advisory comments attached to MR

5. **Gate agent lifecycle enforcement:**
   - Review agents are ephemeral: created per-gate, destroyed after verdict
   - OIDC scope limited to `review:submit` (no push, no task creation)
   - Agent attribution recorded: which persona, which gate config
   - Context window captured in audit trail

## Acceptance Criteria

- [ ] Gate chains support ordered list of mixed gate types
- [ ] All gates execute in parallel when MR enters merge queue
- [ ] Merge processor tracks per-MR gate status (Pending/Running/Passed/Failed)
- [ ] All required gates must pass before merge proceeds
- [ ] Failed required gate blocks merge and notifies author
- [ ] Optional gates (`required = false`) don't block merging
- [ ] Gate agent lifecycle: ephemeral, read-only, scoped, attributed
- [ ] Gate agent context windows captured for audit
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-gates.md` §Gate Chains and §Gate Agent Lifecycle. Existing merge processor: grep for `merge_processor\|merge_queue` in `gyre-server/src/`. Gate storage: `gyre-server/src/api/gates.rs`. Merge queue: `gyre-server/src/api/merge_queue.rs`. Agent spawn: `gyre-server/src/api/spawn.rs`. The key integration point is the merge processor's gate evaluation loop — find where it currently checks TestCommand/LintCommand gates and extend it to handle AgentReview gates. Check migration numbering: currently at 000049.

## Shipped

Both coverage sections were implemented in prior rounds and verified; this
round (repair `2f69d822`, category rebase) resolved the DIRTY merge of the
published candidate `d35ada4ea` against current main `3b734b1af` (task-168
"Repo lifecycle — archive push rejection, admin repos tab, gate config UI"),
preserving the retained gate-chain implementation and reconciling it with
main's parallel gate-position work.

### Merge resolution (commit `a2ba501ac`)

- **Duplicate `quality_gates.position` column**: both branches added it via
  differently-versioned migrations (main's published
  `2026-10-08-000062_gate_position`, version `20261008000062`, runs first;
  task-135's `2026-11-12-000062_gate_chain_config`, version
  `20261112000062`). Diesel runs both on fresh databases, so the second
  `ADD COLUMN` would fail there while single-branch test DBs stayed green.
  Resolution: the published task-168 migration owns the column;
  task-135's migration now adds only the `reviews.reviewer_persona` /
  `reviews.gate_id` attribution columns (its down.sql drops only those).
- **Auto-merge artifacts**: duplicate `position` entries in `schema.rs`,
  the sqlite/postgres `QualityGateRow` structs, insert literals, and 12
  test-fixture sites (recovery, merge_processor, gate_executor,
  mr_timeline, ralph_loop) — all deduplicated; kept the negative-clamping
  row conversion and `(position, created_at)` ordering everywhere
  (sqlite, postgres, mem adapters, `GET /repos/:id/gates`).
- **`create_gate` position semantics unified**: an explicit `position` in
  the request is honored (task-168 drag-to-reorder); otherwise the gate
  appends to the end of the chain (task-135 ordered chains).
- **Both endpoint sets kept**: task-135's `PUT/GET /repos/:id/gate-chain`
  TOML chain configuration and task-168's per-gate
  `PUT /repos/:id/gates/:gate_id` (`update_gate`); routes and ABAC
  mappings cover both.
- **New guard (commit `089cdce38`)**: `update_gate` renames now enforce
  per-repo name uniqueness (409 on collision) — the chain endpoint's
  by-name replacement semantics make that invariant load-bearing once both
  surfaces exist. Regression test
  `update_gate_rename_to_sibling_name_rejected` covers it.

### Prior-round behavior (unchanged, retained)

- Ordered gate chains: `quality_gates.position` with `(position,
  created_at)` read ordering across all three adapters; `create_gate`
  appends; `GET /repos/:id/gates` returns chain order.
- TOML chain configuration: `PUT/GET /api/v1/repos/:id/gate-chain` in the
  spec's `[[gates]]` format; atomic replace (validation-before-mutation,
  JSON → 415); same-named gates keep ids; absent pre-merge gates deleted;
  post-merge recovery gates preserved and name-collision rejected.
- Parallel gate execution: enqueue triggers one tokio task per gate;
  `decide_gate_readiness` is a pure order-independent pass — all required
  gates must pass before merge, any required failure blocks the MR and
  notifies the author with details, advisory (`required = false`) gates
  record results without blocking; RequiredApprovals counts distinct
  approved reviewers with live re-evaluation per cycle.
- Gate agent lifecycle: ephemeral single-purpose review agents torn down
  after verdict; read-only repo access; OIDC-scoped tokens limited to
  `review:submit` (ABAC allow-list in `abac_middleware`); reviews carry
  `reviewer_persona` + `gate_id` attribution; context windows captured in
  the audit trail.

### Test evidence

In-sandbox (via the build wrapper; cgroup memory limit is 4 GiB, so
gyre-server test compilation is deferred to the verifier):

- `cargo check --workspace --all-targets` — clean, no warnings.
- `cargo test -p gyre-adapters quality_gate` — 3 passed (chain ordering,
  position tiebreak, upsert persists position).
- `cargo test -p gyre-adapters migration` — 8 passed, including
  `migration_is_idempotent` / `migrations_create_tables`, proving both
  000062 migrations apply cleanly on a fresh database (the duplicate-ADD
  hazard is gone).
- `cd web && npm test` — 110 files, 2218 passed, 41 skipped.
- `cargo fmt --check`, `check-arch.sh`, `check-migration-versions.sh`,
  `check-migration-sql-portability.sh`, `check-abac-route-registry.sh`,
  `check-mem-port-contracts.sh` — all pass.

Verifier must run (see /tmp/stage/review-evidence/task-135-merge-resolution.md):
`cargo test --all`; focused triage: `cargo test -p gyre-server --lib
api::gates`, `--lib merge_processor`, `--lib gate_executor`, `--test
git_integration`, `cargo test -p gyre-adapters`.
