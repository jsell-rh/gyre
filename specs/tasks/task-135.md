---
title: "Implement gate chain configuration and gate agent lifecycle"
spec_ref: "agent-gates.md §Gate Chains"
depends_on:
  - task-134
progress: complete
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
independently approved in prior rounds. This round reconciled the branch
with the new base (f764bc00, task-187's conversational exploration) and
re-verified the failing "Development controller" CI check from run
43465259045.

### CI failure triage (run 43465259045)

The failed check runs only `tools/fullsend-dispatcher` go tests, the
`scripts/test_dev*.py` / `scripts/test_pipeline*.py` python suites, and
the `scripts/**.test.mjs` node suites. Between the repair base (d9c419a0,
whose CI passed) and the failed candidate (bb7af039) the only change
outside `crates/`, `docs/`, `specs/`, and `web/dist/` is `Cargo.lock` —
every input the failed job executes is byte-identical to the passing
baseline, so no task-135 change is reachable from that job and the
failure was transient. Evidence and probe logs are saved under
`/tmp/stage/review-evidence/` (controller artifact path, not a repo
path).

### Re-verification on the merged tree (f19438f2)

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server` — clean.
- All 16 mechanical `scripts/check-*.sh` lints (arch, ABAC route
  registry + exempt handlers, MCP write gates, migration numbering and
  portability, attribution, scope/lint classes) — pass.
- `cargo test -p gyre-domain` (515) and `cargo test -p gyre-adapters`
  (516) — pass.
- Node script suites (25 tests) — pass.
- `env -u GYRE_PIPELINE_STAGE python3 -m unittest discover -s scripts
  -p 'test_dev*.py'` — 61 pass; the `test_pipeline*.py` suite passes
  except two tests that exercise `os.killpg`, which this sandbox denies
  even on the test's own `start_new_session` child (minimal repro in
  `/tmp/stage/review-evidence/killpg-restriction-repro.txt`). That is
  an environment restriction for remote verification, not a code
  defect; the same script sources passed CI on the baseline. Running
  with the sandbox's exported `GYRE_PIPELINE_STAGE=implement` also
  (correctly) turns the build-admission wrapper's refusals into three
  `test_dev` failures that pass with the variable unset.
- `go` is not installed here; `tools/fullsend-dispatcher` is unchanged
  from the passing baseline.

### Behavior (unchanged this round, from the attributed commits)

- **Gate chains:** repos configure an ordered list of mixed gate types
  (TestCommand, LintCommand, AgentReview, AgentValidation,
  RequiredApprovals) with per-gate `required` flags and type-specific
  config; `trigger_gates_for_mr` creates a Pending GateResult per gate
  and spawns every gate's execution concurrently (tokio tasks), and the
  merge processor waits until every required gate passes. Required-gate
  failure blocks the MR and notifies the author agent with failure
  details; `required = false` gates record results and attach advisory
  comments without blocking. Per-MR gate status is tracked
  Pending → Running → Passed/Failed in the `gate_results` store, and
  `check_gates_for_mr` decides order-independently so a pending gate
  can never mask a failed required gate.
- **Gate agent lifecycle:** review/validation agents are spawned per
  gate run with a scoped OIDC token (`review:submit`/validation scope
  only — no push, no task creation), attributed to their persona and
  gate config (`reviewer_persona`/`gate_id` set from the token, never
  from caller-supplied fields), torn down (token revoked) after the
  verdict, and their full context window captured to the audit trail.

### Verifier-required probes

The sandbox build wrapper refuses gyre-server test compilation during
implement (4 GiB budget). The independent verifier must run:

- `cargo test -p gyre-server gate`
- `cargo test -p gyre-server merge`
- `cargo test --all`
