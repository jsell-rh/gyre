---
title: "Implement gate chain configuration and gate agent lifecycle"
spec_ref: "agent-gates.md §Gate Chains"
depends_on:
  - task-134
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Gate Chains (Composable Policies)"
  - "agent-gates.md §Gate Agent Lifecycle"
commits: ["2999bb5e7b9989eaf0518ecb4b484ecb2228b188", "6ccbdff179e7e559c35c08e5072b53b4590dd29c", "d715552208fab61ea2e1e241e36d0f86074ae2b0", "583c4a07dde4629da277a1efdafaa5b077596b9d"]
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

Both coverage sections are implemented with real production behavior, and
this round reconciled the branch with the new base `f98b179` (rebase
repair `2b78af82`). Product commits: `583c4a07d` (implementation),
`d71555220` (review-repair round f0f41ed: advisory gates never block,
failures precede waiting), `6ccbdff1` (merge reconciliation with the
task-118 base), `2999bb5e` (verification-repair round dbfc3754: scoped
reviewer token minted with the state's `base_url` issuer), and the rebase
merge `62c7de659` (this round).

### Gate chains (§Gate Chains)

- **Ordered chains**: `quality_gates.position` (migration
  `2026-11-12-000062_gate_chain_config` — renumbered from 000061 this
  round because the new base took `2026-11-12-000061_repo_access_overrides`
  and both mapped to the same Diesel version `20261112000061`, which
  would have silently dropped one migration on fresh databases);
  SQLite/Postgres adapters `ORDER BY position, created_at`, mem adapter
  sorts identically; `create_gate` appends; `GET /repos/:id/gates`
  returns chain order.
- **TOML chain configuration**: `PUT/GET /api/v1/repos/:id/gate-chain`
  accepting/returning the spec's `[[gates]]` format. PUT replaces the
  chain atomically (validation-before-mutation; 400 leaves the chain
  untouched; JSON body → 415); same-named gates keep ids; absent pre-merge
  gates deleted; `position` = array index. ABAC-mapped as resource `gate`.
- **Post-merge gates preserved**: the chain manages only PreMerge gates —
  post-merge recovery gates (same table, configured via
  `PUT /repos/:id/post-merge-gates`) are neither deleted nor re-phased by
  a chain PUT; `GET /gate-chain` emits only the pre-merge chain; a chain
  entry reusing a post-merge gate's name is a 400.
- **RequiredApprovals really counts**: distinct reviewers with Approved
  reviews; live re-evaluation each merge-processor cycle promotes a
  waiting gate to Passed the moment the Nth distinct approval lands.
- **Parallel execution / wait / block / notify**: enqueue triggers
  `trigger_gates_for_mr` (one tokio task per gate); `check_gates_for_mr`
  decides via a pure, order-independent pass (`decide_gate_readiness`):
  required failures → Err (merge processor fails the queue entry and
  notifies the author — failures take precedence over waiting), required
  Pending/Running → Ok(false) requeue, else Ok(true).
- **Optional gates never block**: `required = false` gates are never waited
  on and their failures never block — including RequiredApprovals gates
  short of their minimum. Advisory failures attach an MR comment
  attributed to `gate-agent:{gate_id}`.

### Gate agent lifecycle (§Gate Agent Lifecycle)

- **Ephemeral, read-only, identity-scoped**: scoped JWT
  `review:submit`/`validation:report` (task-134 protocol), push denied in
  git_http, token revoked on process exit.
- **Attributed**: `Review.reviewer_persona` + `Review.gate_id` derived
  server-side from the scoped reviewer token's `task_id` claim; surfaced
  in `ReviewResponse` and `GET /merge-requests/:id/reviews`.
- **Context windows captured for audit**: the exact bundle the agent sees
  (persona system prompt, MR diff, spec at pinned SHA, task description)
  zstd-compressed into the conversation repository before the subprocess
  runs, replayable at `GET /api/v1/conversations/:sha`, referenced by a
  `gate_agent_context` audit event; capture skipped and logged when the
  tenant scope is unresolvable — never fabricated.

### Rebase round (2b78af82, merge `62c7de659`)

Resolved the single conflict (docs/api-reference.md: kept both the
gate-chain row and the base's access-override rows), renumbered the gate
chain migration as above, and audited the auto-merge semantically:
`gate_executor.rs` and `api/gates.rs` are byte-identical to the candidate;
the base's additions to `ralph_loop.rs`, `merge_requests.rs`, `mem.rs`,
`spawn.rs`, `api/mod.rs`, and `abac_middleware.rs` touch no gate-related
code (search scoping, repo access overrides, task-194 lifecycle work).

### Test evidence

Regression tests shipped in earlier rounds (mem-backed, plus pure unit
tests): `decide_gate_readiness_failure_precedes_waiting_in_any_order`,
`advisory_approvals_gate_must_not_block_merge`,
`waiting_approvals_gate_must_not_block_failed_required_gate`,
`gate_chain_put_parses_spec_toml_and_orders_chain`,
`gate_chain_put_preserves_post_merge_gates`, plus the gate-agent
attribution and context-capture tests in `merge_requests.rs` and
`gate_executor.rs`.

This sandbox: `cargo check -p gyre-server --all-targets` exit 0;
`check-migration-versions.sh` and all other mechanical check scripts exit
0 on the merged tree. Adapter-level tests executed here against real
SQLite (exit 0): `sqlite::quality_gate` (chain ordering persists and
roundtrips via the renumbered migration), `sqlite::review` (persona/gate
attribution roundtrips), `sqlite::merge_queue` (8 status-transition
tests), and the `no_duplicate_migration_version_prefixes` guard. gyre-server
test *execution* is blocked by the build wrapper's 4 GiB budget
(infrastructure restriction, not a code defect) and must run in the
independent verifier. Required probes:
- `cargo test -p gyre-server --lib scoped_gate_review_records_persona_attribution`
- `cargo test -p gyre-server --lib gate_chain`
- `cargo test -p gyre-server --lib required_approvals`
- `cargo test -p gyre-server --lib check_gates`
- `cargo test -p gyre-server --lib advisory`
- `cargo test -p gyre-server --lib agent_review`
- `cargo test -p gyre-server --lib decide_gate_readiness`
- `cargo test -p gyre-server --lib ralph_loop::tests::gate_failure_respawns_agent_with_fresh_context`
- task acceptance criterion `cargo test --all`
