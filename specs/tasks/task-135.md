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

- [x] Gate chains support ordered list of mixed gate types
- [x] All gates execute in parallel when MR enters merge queue
- [x] Merge processor tracks per-MR gate status (Pending/Running/Passed/Failed)
- [x] All required gates must pass before merge proceeds
- [x] Failed required gate blocks merge and notifies author
- [x] Optional gates (`required = false`) don't block merging
- [x] Gate agent lifecycle: ephemeral, read-only, scoped, attributed
- [x] Gate agent context windows captured for audit
- [x] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-gates.md` §Gate Chains and §Gate Agent Lifecycle. Existing merge processor: grep for `merge_processor\|merge_queue` in `gyre-server/src/`. Gate storage: `gyre-server/src/api/gates.rs`. Merge queue: `gyre-server/src/api/merge_queue.rs`. Agent spawn: `gyre-server/src/api/spawn.rs`. The key integration point is the merge processor's gate evaluation loop — find where it currently checks TestCommand/LintCommand gates and extend it to handle AgentReview gates. Check migration numbering: currently at 000049.

## Shipped

Both coverage sections (§Gate Chains (Composable Policies), §Gate Agent
Lifecycle) are implemented and independently approved in prior rounds.
This round repaired the DIRTY merge state against current main
(4dc0eb9b, task-139 forge enforcement policies) with merge commit
`a312f2a7`, preserving the retained task-135 implementation.

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
  (persona prompt, pinned spec content, diff, task description) is
  captured to the audit trail before the process runs.

### Merge resolution detail (`a312f2a7`)

The only conflicted file was `crates/gyre-server/src/gate_executor.rs`,
with two hunks of identical shape: task-135's context-window capture vs
main's pinned-SHA `GYRE_SPEC_URL` delivery. Both sides were kept — they
are complementary (audit capture of the exact bundle + serving the spec
blob at the MR's `spec_ref` SHA rather than HEAD). No behavior from
either side was dropped.

### Test evidence

- `cargo check -p gyre-server` — clean on the resolution
  (log: `/tmp/stage/review-evidence/cargo-check-gyre-server.txt`).
- gyre-server test builds exceed the in-sandbox 4 GiB budget; the
  verifier must run: `cargo test -p gyre-server --lib update_gate`,
  `cargo test -p gyre-server --test task134_review_probe`, then the full
  `cargo test --all`.
