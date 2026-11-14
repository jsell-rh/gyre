---
title: "Implement gate chain configuration and gate agent lifecycle"
spec_ref: "agent-gates.md §Gate Chains"
depends_on:
  - task-134
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Gate Chains (Composable Policies)"
  - "agent-gates.md §Gate Agent Lifecycle"
commits: ["25a6f4639150bdabb1c9f89e9e2a0bc2e41c89aa", "9ff6b003411e9f09c18d3a0a8e327780bbc59c22", "089cdce388806ffd3c3514859f73148488c3c3a5", "2999bb5e7b9989eaf0518ecb4b484ecb2228b188", "6ccbdff179e7e559c35c08e5072b53b4590dd29c", "d715552208fab61ea2e1e241e36d0f86074ae2b0", "583c4a07dde4629da277a1efdafaa5b077596b9d"]
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
independently approved in prior rounds. This round is a contract repair:
the prior checkpoint amended protected task text (the last acceptance
criterion's wording), which the contract guard rejects; this commit
restores the assigned contract verbatim, records the branch's
product-surface commits, and sets ready-for-review. No production code
changed this round — the tree is identical to the verified 25a6f463
(the doc-only commits after it touch only this file).

### Behavior (attributed commits)

- **Gate chains (§Gate Chains):** repos configure an ordered list of
  mixed gate types (TestCommand, LintCommand, AgentReview,
  AgentValidation, RequiredApprovals) with per-gate `required` flags
  and type-specific config. `trigger_gates_for_mr`
  (crates/gyre-server/src/gate_executor.rs) creates a Pending
  GateResult per gate and spawns every gate's execution concurrently
  (tokio tasks); the merge processor waits until every required gate
  passes. Required-gate failure blocks the MR and notifies the author
  agent with failure details; `required = false` gates record results
  and attach advisory comments without blocking. Per-MR gate status is
  tracked Pending → Running → Passed/Failed in the `gate_results`
  store.
- **Gate agent lifecycle (§Gate Agent Lifecycle):** review/validation
  agents are spawned per gate run with a scoped OIDC token
  (`review:submit` / `validation:report` only — no push, no task
  creation), attributed to their persona and gate config
  (`reviewer_persona`/`gate_id` derived server-side from the token's
  `task_id` claim, never from caller-supplied fields), torn down
  (token revoked) after the verdict, and their full context window
  captured to the audit trail.

### CI repair already landed (25a6f463)

The merge of base 6ffd6799 (task-130 §1 token-scoping claims) left two
task-135-side sites on the pre-§1 contract: the
`scoped_gate_review_records_persona_attribution` test called the old
6-arg `mint_scoped` (E0061 in the lib test target — the Clippy failure,
run 43535816551), and `submit_review`'s attribution gate read the scope
claim as a raw JSON string, which with the claim now a `Vec<String>` is
always `None`, silently dropping persona/gate attribution. Both were
repaired: the test mints exactly as the gate executor does (MR →
workspace → tenant resolved fail-closed, persona slug from the gate
persona reference), and the attribution gate reads the
extractor-derived `auth.scope`, identical to the identity binding above
it. A tree-wide sweep confirmed no other string-form scope reader
remains. Clippy-diff vs main and vs assigned base both exited 0 at
25a6f463; all mechanical lints passed (logs retained under
/tmp/stage/review-evidence/).

### Verifier-required probes

The sandbox build wrapper refuses gyre-server test compilation during
implement (4 GiB budget). The independent verifier must run
`cargo nextest run --all` (the CI test job), in particular the
gyre-server slices:

- `api::merge_requests::tests::scoped_gate_review_records_persona_attribution`
- `api::merge_requests::tests::unscoped_review_has_no_gate_attribution`
- `api::merge_requests::tests::submit_review_binds_agent_token_caller_to_own_identity`
- `--test task134_review_probe` (scoped-token enforcement suite)
- `gate_executor::` (gate chain + scoped-token suites)
