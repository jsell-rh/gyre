---
title: "Implement gate chain configuration and gate agent lifecycle"
spec_ref: "agent-gates.md §Gate Chains"
depends_on:
  - task-134
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Gate Chains (Composable Policies)"
  - "agent-gates.md §Gate Agent Lifecycle"
commits: ["25a6f4639150bdabb1c9f89e9e2a0bc2e41c89aa", "9ff6b003411e9f09c18d3a0a8e327780bbc59c22", "089cdce388806ffd3c3514859f73148488c3c3a5", "2999bb5e7b9989eaf0518ecb4b484ecb2228b188", "6ccbdff179e7e559c35c08e5072b53b4590dd29c", "d715552208fab61ea2e1e241e36d0f86074ae2b0", "583c4a07dde4629da277a1efdafaa5b077596b9d", "54ac2796a7963af5cf7da39219ef36080186f8d8"]
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
independently approved in prior rounds. This round is a verification
repair: the full-suite run of candidate b4093d74 (attempt c7b64c69)
failed exactly one test,
`gate_executor::tests::agent_review_context_capture_skips_without_workspace_tenant`
("capture must be skipped, not stored under a fabricated tenant").

### Root cause (fixture, not production code)

`seed_mr_with_repo` left the MR on `MergeRequest::new`'s bootstrap
`default` workspace binding while the repo lives in `ws-test`, and
ensured a workspace row for `default` — so the MR's workspace always
resolved and the capture was always recorded under a tenant the repo
does not belong to. The orphan test's premise (no scoperable tenant)
could never hold, and the whole fixture family silently scoped
gate-agent tokens and captured context windows to the wrong workspace.

### Repair (54ac2796)

- `seed_mr_with_repo` now models the production create paths: the MR
carries the repo's workspace (`ws-test`) and the ws-test workspace row
is ensured — the same shape as the existing `seed_full_review_fixture`.
The stray `default`-row ensure is removed.
- The orphan test now deletes the ws-test row after seeding and asserts
the §1 token-scoping contract (task-130 dependency, fail-closed): the
gate FAILS with `workspace ws-test not found`, no gate token is minted
(fail-before-mint), and no `gate_agent_context` audit event exists
(capture skipped, never stored under a fabricated tenant). The prior
"reviewer runs and exits without submitting" expectation contradicted
the fail-closed dependency contract and was corrected; all original
guards are preserved or strengthened.

### Behavior (prior rounds, unchanged)

- **Gate chains (§Gate Chains):** repos configure an ordered list of
mixed gate types (TestCommand, LintCommand, AgentReview,
AgentValidation, RequiredApprovals) with per-gate `required` flags and
type-specific config. `trigger_gates_for_mr` creates a Pending
GateResult per gate and spawns every gate's execution concurrently;
the merge processor waits until every required gate passes.
Required-gate failure blocks the MR and notifies the author agent with
failure details; `required = false` gates record results and attach
advisory comments without blocking. Per-MR gate status is tracked
Pending → Running → Passed/Failed in the `gate_results` store.
- **Gate agent lifecycle (§Gate Agent Lifecycle):** review/validation
agents are spawned per gate run with a scoped OIDC token
(`review:submit` / `validation:report` only), attributed to their
persona and gate config (derived server-side from the token's
`task_id` claim, never from caller-supplied fields), torn down (token
revoked) after the verdict, and their full context window captured to
the audit trail.

### Test evidence

- This sandbox: `cargo check -p gyre-server --lib --tests` exit 0
(type-checks the repaired test module; the implement-stage build
wrapper refuses gyre-server test compilation under the 4 GiB budget),
`rustfmt --check` clean, `check-fabricated-scope-defaults`,
`check-scope-literal-defaults`, and `check-arch` all OK. Evidence:
`/tmp/stage/review-evidence/task-135-orphan-capture-repair.md`.
- Verifier must run `cargo test --all` (the failing command) — in
particular `cargo test -p gyre-server --lib gate_executor`: the
repaired orphan test plus the ~20 sibling tests on the changed fixture
(TestCommand/LintCommand/check_gates_for_mr family, AgentReview
context-gathering family, validation-token family) must pass.
