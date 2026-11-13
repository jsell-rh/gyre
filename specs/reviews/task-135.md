# Review — task-135 (Gate chain configuration and gate agent lifecycle)

Spec: `specs/system/agent-gates.md` §Gate Chains (Composable Policies), §Gate Agent Lifecycle.
Base `3b734b1af` → candidate `daf79bc87` (rebase/merge round over the retained
gate-chain implementation; product commits `583c4a07`, `d7155522`, `6ccbdff1`,
`2999bb5e`, `089cdce3`).
Verdict: **approved** — both coverage sections are implemented by real production
code; no blocking findings. Gyre-server test *execution* deferred to the verifier
(sandbox memory budget); type-checked here.

## Probes run (this round, candidate checkout)

- `cargo test -p gyre-adapters` (via `/tmp/stage/bin/cargo`) → **473 passed,
  0 failed** (269s), including the new `list_by_repo_orders_by_chain_position`,
  `list_by_repo_position_tie_falls_back_to_created_at`,
  `review_attribution_persists_and_roundtrips`,
  `review_without_attribution_stays_null`, and all 8 migration tests
  (`migration_is_idempotent`, `migrations_create_tables`,
  `no_duplicate_migration_version_prefixes`) — proving the
  `2026-11-12-000062_gate_chain_config` migration applies cleanly alongside
  base's `2026-10-08-000062_gate_position` on a fresh database (the
  duplicate-ADD-column hazard from the DIRTY merge is gone).
- `cargo check -p gyre-server` → clean (5m45s). `cargo check -p gyre-server
  --tests` → clean (3m42s) — the new gates.rs / gate_executor.rs /
  merge_requests.rs test targets compile without warnings.
- Mechanical invariants, all exit 0: `check-arch`, `check-migration-versions`,
  `check-migration-sql-portability`, `check-abac-route-registry`,
  `check-abac-exempt-handlers`, `check-mcp-write-tools`,
  `check-dead-message-kinds`, `check-byte-slice-truncation`,
  `check-relative-path-defaults`, `check-fail-open-ref-resolution`,
  `check-fabricated-scope-defaults`, `check-lossy-secret-conversion`,
  `check-scope-literal-defaults`, `check-inert-enforcement`,
  `check-forwarded-header-trust`, `check-in-memory-state-stores`,
  `check-unbounded-external-http`.
- `check-task-commit-attribution.sh`: fails against the *stage-reset working
  tree* (task file reverted to `commits: []`), **passes against the candidate
  content** (`git show daf79bc87:specs/tasks/task-135.md` — all five product
  commits listed). Stage artifact, not a candidate defect.

## Verified working

- **Ordered chains of mixed gate types**: `quality_gates.position` (column owned
  by base's task-168 migration; candidate's migration adds only
  `reviews.reviewer_persona`/`reviews.gate_id` and documents the ownership
  split). Read ordering `(position, created_at)` consistent across SQLite,
  PostgreSQL, and mem adapters and `GET /repos/:id/gates` (adapters tests prove
  ordering and tiebreak on real SQLite).
- **Chain configuration**: `PUT/GET /repos/:id/gate-chain` parses the spec's
  exact `[[gates]]` TOML (PascalCase or snake_case types, `command`, `persona`,
  `min_approvals`, `required`, `timeout_secs`); full validation before the first
  mutation (duplicate/empty names, missing command/min_approvals, unknown type,
  malformed TOML, non-TOML content-type → 400/415 with chain untouched — pinned
  by `gate_chain_invalid_entries_rejected_and_chain_unchanged`); same-named
  gates keep ids, absent pre-merge gates deleted, post-merge recovery gates
  preserved and name-collisions rejected; GET round-trips. `update_gate` renames
  now enforce per-repo name uniqueness (409), which the chain's by-name
  replacement semantics make load-bearing.
- **Parallel execution**: `enqueue` → `trigger_gates_for_mr` creates one
  Pending GateResult per gate and `tokio::spawn`s one task per gate (code
  verified at `gate_executor.rs:34-68`); merge processor polls
  `check_gates_for_mr` each cycle (merge_processor.rs:788, 1732) and requeues on
  `Ok(false)`.
- **Decision semantics**: `decide_gate_readiness` is a pure two-pass,
  order-independent decision — required failures block (Err) even while other
  required gates are Pending/Running; required Pending/Running waits; advisory
  gates never block in any state (including advisory RequiredApprovals gates
  short of their minimum — prior review F1/F2 repairs, present with regression
  tests `decide_gate_readiness_failure_precedes_waiting_in_any_order`,
  `advisory_approvals_gate_must_not_block_merge`,
  `waiting_approvals_gate_must_not_mask_failed_required_gate`).
- **RequiredApprovals is real**: was a hollow "delegated to merge processor"
  pass-through; now counts distinct approved reviewers live (`run_required_approvals_gate`
  → Running while short; `check_gates_for_mr` promotes to Passed and persists
  the promotion when the Nth distinct approval lands). Distinctness pinned by
  `required_approvals_counts_distinct_reviewers`.
- **Failed required gate blocks + notifies**: merge processor fails the queue
  entry with the reason; `run_gate` emits a `GateFailure` event carrying the
  output and calls `notify_gate_failure` (HSI §2). Advisory failures attach a
  `gate-agent:{gate_id}`-attributed MR comment instead of blocking.
- **Gate agent lifecycle**: ephemeral scoped identities (`gate-review-<uuid>`)
  with `review:submit` tokens minted with the state's `base_url` issuer (fix
  `2999bb5e`), revoked on completion/crash/timeout (task-134 machinery,
  retained). **Attribution** is new and real: `reviews.reviewer_persona` +
  `reviews.gate_id` derived server-side in `submit_review` from the scoped
  token's `task_id` claim (gate lookup), never from body fields — the forge test
  (`scoped_gate_review_records_persona_attribution`) submits a body with a
  forged `reviewer_agent_id` and asserts the server-derived identity and
  attribution. **Context windows** are captured before the subprocess runs
  (persona prompt, diff, pinned-sha spec content, task description), zstd'd
  into the conversation repository, retrievable at `GET /conversations/:sha`,
  referenced by a `gate_agent_context` audit event; capture skips (logged)
  when the tenant scope is unresolvable rather than fabricating one —
  consistent with the fabricated-scope invariants (scripts pass).
- **Schema consistency**: Row struct field order matches the reordered
  `schema.rs` (`position` before `created_at`) in both SQLite and PG adapters —
  proven by the passing real-SQLite roundtrip tests, not just compilation.
- Docs (`api-reference.md`, `agent-protocol.md`) and coverage matrix updated to
  match actual authorization and behavior (prior F5 repair retained).

## Deferred to verifier (sandbox budget — must run)

- `cargo test --all`
- Focused: `cargo test -p gyre-server --lib api::gates`,
  `cargo test -p gyre-server --lib gate_executor`,
  `cargo test -p gyre-server --lib merge_processor`,
  `cargo test -p gyre-server --test git_integration`.
  (All type-check clean here; test bodies reviewed — they drive the real
  router/mem stores and assert persisted state, not mocks.)

## Non-blocking observations (recorded, no action required)

- `set_gate_chain` applies per-entry saves without a wrapping DB transaction;
  a *runtime storage error* mid-loop could leave a partially applied chain.
  Spec-invalid chains are fully rejected before mutation (tested); the failure
  direction of a mid-loop DB error is conservative. Not spec-required.
- Advisory gate failures also emit `GateFailure` events/notifications in
  addition to the advisory comment — informational, not blocking; spec requires
  notification for required failures and does not forbid it for advisory ones.
- The `notify_gate_failure` payload carries gate name/MR but not the gate
  output text; failure details live in the GateResult and the `GateFailure`
  message event. Full inbox delivery of failure details is task-137's scope.
- Migration sequence `000062` is date-prefixed-duplicated with base's
  `2026-10-08-000062_gate_position` (unique Diesel versions; up.sql documents
  the column ownership). Consistent with existing base practice
  (`000060`/`000061` also date-duplicated) and `check-migration-versions.sh`
  passes.
- Generated `web/dist/`: no web source changes in this candidate, so not
  applicable; bundle regeneration is verifier-owned regardless.

Test-inflation check: none found. New tests drive real routers, real SQLite
databases, real token minting/verification, and assert persisted state; several
(advisory-never-blocks, failure-precedes-waiting, forge-attribution,
distinct-approvals) kill specific bugs found in earlier review rounds.
