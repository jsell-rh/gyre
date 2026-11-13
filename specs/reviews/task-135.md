# Review — task-135 (Implement gate chain configuration and gate agent lifecycle)

Spec: `specs/system/agent-gates.md` §Gate Chains (Composable Policies), §Gate Agent Lifecycle.
Base `48b3d47e` → candidate `50cd52b3` (six product commits: `583c4a07`, `d7155522`,
`6ccbdff1`, `2999bb5e`, `089cdce3`, `9ff6b003`, plus process/merge commits recorded in
`specs/tasks/task-135.md` frontmatter at HEAD).
Verdict: **approved** — both coverage sections are implemented with real production
behavior; no fakery, no test inflation found. Server-side test execution is delegated
to the independent verifier (sandbox 4 GiB build limit); the required commands are
recorded below and in `/tmp/stage/review-evidence/`.

## Probes run (in-sandbox, candidate checked out at `50cd52b3`)

- `/tmp/stage/bin/cargo check -p gyre-server --lib --tests` → **clean** (10m; all new
  endpoint/executor/attribution test code compiles).
- `/tmp/stage/bin/cargo test -p gyre-adapters --lib sqlite::review` → **11 passed**
  (incl. `review_attribution_persists_and_roundtrips`,
  `review_without_attribution_stays_null`).
- `/tmp/stage/bin/cargo test -p gyre-adapters --lib sqlite::quality_gate` → **3 passed**
  (incl. `list_by_repo_orders_by_chain_position`,
  `list_by_repo_position_tie_falls_back_to_created_at`).
- `/tmp/stage/bin/cargo test -p gyre-domain --lib` → **474 passed**.
- `scripts/check-arch.sh`, `check-abac-route-registry.sh`,
  `check-migration-sql-portability.sh`, `check-mem-port-contracts.sh`,
  `check-migration-versions.sh` → all OK.
- `/tmp/stage/bin/cargo test -p gyre-server --lib ...` → **blocked by the executor's
  4 GiB wrapper** (infrastructure limit, not a code defect). Verifier must run:
  `cargo test -p gyre-server --lib update_gate`, `--lib api::gates`,
  `--lib gate_executor`, `--lib merge_requests`, then full `cargo test --all`.

## Verified against the acceptance criteria

- **Ordered chains of mixed gate types — real.** `position` column (from the
  pre-existing task-168 migration `2026-10-08-000062_gate_position`, present in the
  base; the new migration correctly does NOT re-add it), adapters order by
  `(position, created_at)` in SQLite, Postgres, and mem; `create_gate` appends.
  `PUT/GET /repos/:id/gate-chain` accepts/returns the spec's exact `[[gates]]` TOML
  (PascalCase and snake_case type names), with per-entry validation (duplicate names,
  missing `command`, missing `min_approvals`, unknown types, malformed TOML → 400
  **before any mutation**), same-named gates keep ids, absent pre-merge gates deleted,
  post-merge gates preserved (name collision with a post-merge gate → 400). JSON
  bodies → 415. Route registered in the ABAC resolver (`gate` resource).
- **Parallel execution — real.** `merge_queue.rs:133` calls `trigger_gates_for_mr` on
  enqueue; it creates one Pending `GateResult` per configured gate and spawns one
  tokio task per gate (`gate_executor.rs:34-71`).
- **Per-MR gate status tracking — real.** `GateResult` Pending → Running →
  Passed/Failed persisted through `GateResultRepository::update_status` (verified in
  SQLite adapter and mem store).
- **Required gates must pass; failure blocks + notifies.** `check_gates_for_mr` now
  evaluates all results into `(status, is_required, gate_id)` and decides in a second,
  order-independent pass (`decide_gate_readiness`): required failures scanned across
  ALL results before any wait decision, so a Pending/Running required gate can never
  mask a Failed required gate (repair `d7155522`, regression tests
  `decide_gate_readiness_failure_precedes_waiting_in_any_order`,
  `waiting_approvals_gate_must_not_mask_failed_required_gate`). The merge processor
  (step 4b, `merge_processor.rs:1732`) fails the queue entry on `Err`, and
  `run_gate` emits `GateFailure` + `notify_gate_failure` to the author on failure.
- **Optional gates never block — real.** Advisory gates are excluded from both the
  failure and the wait pass in any state (including RequiredApprovals gates short of
  their minimum — repair `d7155522`, regression
  `advisory_approvals_gate_must_not_block_merge`); advisory failures are recorded and
  attach an MR comment attributed to `gate-agent:{gate_id}` (regression tests assert
  exactly one comment, correct author, and that required failures post none).
- **RequiredApprovals really counts.** Was a hollow "delegated to merge processor"
  Passed stub at base; now `count_distinct_approvals` counts **distinct** approving
  reviewers live (re-approval by the same reviewer doesn't satisfy min-2), a short
  gate waits as Running, and the merge processor's live re-check promotes it to
  Passed the moment the Nth distinct approval lands (promotion persisted).
- **Gate agent lifecycle.** Ephemeral/read-only/identity-scoped shipped in task-134
  (base: scoped `review:submit` JWT, token revoked on exit incl. crash/timeout, push
  denied, exact-match scope checks — verified present in base `48b3d47e`).
  **Attribution is new and real:** `Review.reviewer_persona` + `Review.gate_id`
  (migration `2026-11-12-000062`, SQLite + Postgres + mem), derived **server-side**
  in `submit_review` from the scoped token's `task_id` claim — the request body
  cannot forge or suppress it (test submits `reviewer_agent_id: "forged-reviewer"`
  and asserts the token subject + persona win). Surfaced in `ReviewResponse`.
  **Context windows captured for audit — real:** `capture_gate_agent_context`
  zstd-compresses the exact bundle the agent sees (persona system prompt, MR diff,
  spec at the **pinned** SHA, MR title, task description) into the conversation
  repository *before* the subprocess runs, and records a `gate_agent_context` audit
  event with the conversation SHA. The test fixture uses a real bare git repo
  (`Git2OpsAdapter`) with the spec advanced past the pinned SHA and asserts the
  captured content is v1-at-SHA, not the tip — a meaningful, non-self-confirming
  test. Validators are captured too. Capture is skipped+logged when the workspace's
  tenant cannot be resolved (never fabricated) — regression-tested.
- **This round's repair (`9ff6b003`) is correct.** The 409 rename-conflict body now
  returns the unchanged `GateResponse` at the top level alongside `error`, still
  before any mutation or save; the previously failing
  `update_gate_rename_to_sibling_name_rejected` asserts status 409 and
  `json["name"] == "lint"`, which the new body satisfies. Verified in source; the
  test itself is verifier-run (compile-verified here).
- **No fakery / no test inflation.** No new `todo!`/`bail!`/stubs in the diff. New
  tests drive the real router or real repositories with real git fixtures and real
  minted JWTs; they assert behavior, not mirrored logic. `check-mem-port-contracts`
  and migration portability/duplication checks pass (the two `000062` dirs have
  distinct full Diesel versions `20261008000062` / `20261112000062`).

## Non-blocking observations (recorded, no repair required)

- The coverage-matrix notes in `specs/coverage/system/agent-gates.md` cite migration
  `2026-11-12-000061` for the `position`/attribution columns; the actual migrations
  are `2026-10-08-000062_gate_position` (task-168, pre-existing) and
  `2026-11-12-000062_gate_chain_config`. Cosmetic citation error only — the
  migrations themselves are correct, non-duplicating, and portable.
- `set_gate_chain` applies saves/deletes sequentially with no repo-level
  transaction; a mid-apply infrastructure (DB) error could leave a partially applied
  chain. Validation errors are all-or-nothing as documented. The port offers no
  transactional replace, and this matches the codebase's prevailing multi-write
  handler style.
- `notify_gate_failure(..., "default")` tenant literal in `gate_executor.rs` is
  pre-existing baseline (present verbatim at base `48b3d47e`, covered by the frozen
  exemption), not introduced by this candidate.
- `scripts/check-task-commit-attribution.sh` fails **only** against the staged
  working tree, where the executor reset `specs/tasks/task-135.md` to the pristine
  assignment state; at the committed candidate HEAD the frontmatter lists all six
  product commits. No real attribution gap.

## Generated-output note (verifier-owned)

`web/dist/` was not touched by this candidate; per the stage contract, bundle
regeneration and freshness are owned by the deterministic verifier.
