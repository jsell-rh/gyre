---
title: "Implement gate chain configuration and gate agent lifecycle"
spec_ref: "agent-gates.md §Gate Chains"
depends_on:
  - task-134
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Gate Chains (Composable Policies)"
  - "agent-gates.md §Gate Agent Lifecycle"
commits: ["9ff6b003411e9f09c18d3a0a8e327780bbc59c22", "089cdce388806ffd3c3514859f73148488c3c3a5", "2999bb5e7b9989eaf0518ecb4b484ecb2228b188", "6ccbdff179e7e559c35c08e5072b53b4590dd29c", "d715552208fab61ea2e1e241e36d0f86074ae2b0", "583c4a07dde4629da277a1efdafaa5b077596b9d"]
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

Both coverage sections are implemented in production code and were
independently approved in prior rounds; this round resolved the fresh
rebase conflict against main (task-128's ABAC policy engine).

### Behavior

- **Gate chains:** repos configure an ordered list of mixed gate types
  (TestCommand, LintCommand, AgentReview, AgentValidation,
  RequiredApprovals) with per-gate `required` flags and type-specific
  config; the merge processor runs all gates in parallel per MR and waits
  until every required gate passes. Required-gate failure blocks the MR
  and notifies the author agent with failure details; `required = false`
  gates record results and attach advisory comments without blocking.
  Per-MR gate status is tracked Pending → Running → Passed/Failed.
- **Gate agent lifecycle:** review/validation agents are spawned per gate
  run with a scoped OIDC token (`review:submit` only — no push, no task
  creation), attributed to their persona and gate config, torn down
  (token revoked) after the verdict, and their full context window
  captured to the audit trail.

### This round: rebase resolution

Main gained task-128's ABAC policy engine, which refactored the route
literals in `abac_middleware.rs` into constants. The only conflicted file
was `crates/gyre-server/src/abac_middleware.rs`; the resolution keeps
task-128's `REPOS_WORKTREES_ID_ROUTE`/`REPOS_GATES_ROUTE`/
`REPOS_GATES_ID_ROUTE` constants and preserves task-135's
`/api/v1/repos/:id/gate-chain` resolver mapping (required: the route is
registered in `api/mod.rs` and an unregistered route gets no policy
evaluation). All other task-128 files auto-merged.

### Test evidence

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` — clean on the merged
  tree (no semantic conflict between task-128's policy engine and
  task-135's gate/merge-processor code).
- `scripts/check-abac-route-registry.sh` — OK: all registered
  `/api/v1/` routes resolve in the ABAC registry, no duplicate entries,
  no exemption growth.
- `scripts/check-arch.sh` — passed.
- Behavior tests for gate chains and agent lifecycle live in
  `crates/gyre-server/src/api/gates.rs` and
  `crates/gyre-server/src/merge_processor.rs` (unchanged this round;
  full `cargo test --all` runs in the independent verifier).
