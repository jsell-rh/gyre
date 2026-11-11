---
title: "Implement gate chain configuration and gate agent lifecycle"
spec_ref: "agent-gates.md §Gate Chains"
depends_on:
  - task-134
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Gate Chains (Composable Policies)"
  - "agent-gates.md §Gate Agent Lifecycle"
commits: ["583c4a07d9c1d0f2789cf3748a02d25152d3435d"]
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

Both coverage sections are implemented with real production behavior
(product commit `583c4a07d9c1d0f2789cf3748a02d25152d3435d`).

### Gate chains (§Gate Chains)

- **Ordered chains**: `quality_gates.position` (migration
  `2026-11-12-000061`, next free sequence after 000060); SQLite and
  Postgres adapters `ORDER BY position, created_at` (creation order
  tiebreaks pre-column rows), mem adapter sorts identically;
  `create_gate` appends at the end of the chain; `GET /repos/:id/gates`
  returns chain order; `GateResponse` exposes `position`.
- **TOML chain configuration**: `PUT/GET /api/v1/repos/:id/gate-chain`
  accepting/returning the spec's exact `[[gates]]` format (`name`,
  `type` in PascalCase or snake_case, `command`, `persona`,
  `min_approvals`, `validation_type`, `required`, `timeout_secs`).
  PUT replaces the chain atomically: all validation happens before the
  first mutation (duplicate names / missing per-type fields / unknown
  types / malformed TOML → 400 with the current chain untouched; a JSON
  body → 415); same-named gates keep their ids so existing GateResults
  stay linked; gates absent from the chain are deleted; `position` =
  array index. Route is ABAC-mapped as resource `gate`
  (`check-abac-route-registry` passes).
- **RequiredApprovals really counts now**: the previous code returned
  `Passed` claiming "approval check delegated to merge processor" — no
  delegate existed anywhere. Now `run_required_approvals_gate` counts
  DISTINCT reviewers with Approved reviews; short of the minimum the
  gate is `Running`, and `check_gates_for_mr` re-evaluates live every
  merge-processor cycle, promoting the persisted result to `Passed` the
  moment the Nth distinct approval lands (no re-enqueue). One reviewer
  approving twice counts once.
- **Parallel execution / wait / block / notify** (pre-existing, verified
  intact): enqueue triggers `trigger_gates_for_mr` which spawns one
  tokio task per gate; `check_gates_for_mr` returns not-ready (requeue)
  while required gates are Pending/Running and fails the queue entry +
  notifies the author when a required gate fails.
- **Optional gates**: `required = false` failures don't block and now
  attach an advisory comment on the MR attributed to
  `gate-agent:{gate_id}` (results were already recorded in
  `gate_results`).

### Gate agent lifecycle (§Gate Agent Lifecycle)

- **Ephemeral, read-only, identity-scoped**: shipped with task-134
  (scoped JWT `review:submit`/`validation:report`, exact-match scope
  enforcement, push denied in git_http, token revoked on process exit
  including crash/timeout) — unchanged, covered by task-134's tests.
- **Attributed** (new): `Review.reviewer_persona` + `Review.gate_id`
  (migration `2026-11-12-000061`; SQLite + Postgres + mem round-trips).
  `submit_review` derives both server-side from the scoped reviewer
  token's `task_id` claim (the gate that minted it) — request bodies
  cannot forge or suppress attribution. `ReviewResponse` and
  `GET /merge-requests/:id/reviews` surface both fields.
- **Context windows captured for audit** (new): before the
  review/validation subprocess runs, the exact bundle the agent will see
  (persona system prompt, full MR diff, spec content at the pinned
  `spec_ref` SHA, MR title, task description/acceptance criteria) is
  zstd-compressed and stored through the conversation repository —
  the same provenance store regular agents use — replayable at
  `GET /api/v1/conversations/:sha`. A `gate_agent_context` audit event
  records the conversation SHA, gate, persona, and MR. Capture happens
  before the process runs, so timed-out or crashed reviewers still
  leave an auditable context. When the workspace's tenant scope cannot
  be resolved, capture is skipped and logged — never stored under a
  fabricated tenant identity.

### Test evidence

Server test compilation is deferred to the independent verifier (the
build wrapper blocks gyre-server test builds in this sandbox to fit the
4 GiB budget; `cargo check -p gyre-server --tests` passes). The verifier
should run:

- `cargo test -p gyre-adapters --lib -- sqlite::review sqlite::quality_gate`
  (already executed here: 14/14 pass, including the new
  `review_attribution_persists_and_roundtrips`,
  `review_without_attribution_stays_null`,
  `list_by_repo_orders_by_chain_position`,
  `list_by_repo_position_tie_falls_back_to_created_at`, and the extended
  `save_upsert_updates_all_config_columns`; the fresh-DB run also proves
  migration 000061 applies cleanly).
- `cargo test -p gyre-domain --lib review` (executed here: 8/8 pass).
- `cargo test -p gyre-server --lib` — new tests:
  `gate_chain_put_parses_spec_toml_and_orders_chain`,
  `gate_chain_replace_reorders_keeps_ids_deletes_removed`,
  `gate_chain_invalid_entries_rejected_and_chain_unchanged`,
  `gate_chain_unknown_repo_is_404`,
  `advisory_gate_failure_attaches_mr_comment`,
  `required_gate_failure_attaches_no_advisory_comment`,
  `required_approvals_gate_passes_when_count_met`,
  `required_approvals_gate_waits_until_approvals_arrive`,
  `required_approvals_counts_distinct_reviewers`,
  `agent_review_captures_context_window_for_audit`,
  `agent_review_context_capture_skips_without_workspace_tenant`,
  `scoped_gate_review_records_persona_attribution`,
  `unscoped_review_has_no_gate_attribution`; plus the full existing
  gate_executor / merge_processor / api suites for regressions
  (notably `check_gates_*`, `selection_skips_gates_pending_tries_next_candidate`,
  `selection_fails_gates_failed_entry_tries_next`, and the task-134
  e2e review tests, which exercise the code paths this task extends).
- `cargo test --all` (acceptance criterion).
- Mechanical guards executed here and passing: check-arch,
  check-abac-route-registry, check-migration-versions,
  check-migration-sql-portability, check-byte-slice-truncation,
  check-fabricated-scope-defaults, check-inert-enforcement,
  check-lossy-secret-conversion, check-mem-port-contracts,
  check-relative-path-defaults, check-scope-literal-defaults,
  check-forged-scope-fields, check-in-memory-state-stores,
  check-dead-message-kinds, check-forwarded-header-trust,
  check-no-em-dash, check-conditional-test-guards. All other check
  scripts match baseline exit codes exactly (no new findings).
