# Review — task-135 (Implement gate chain configuration and gate agent lifecycle)

Spec: `specs/system/agent-gates.md` §Gate Chains (Composable Policies), §Gate Agent Lifecycle.
Candidate: `b4093d74e910b2cd86e949e1048f750031587640` (base `6ffd6799`).
Product commits: `583c4a07`, `6ccbdff1`, `d7155522`, `2999bb5e`, `089cdce3`, `9ff6b003`, `25a6f463` (all recorded in frontmatter).
Verdict: **approved** — real implementations for both coverage sections; no fakery found.

## Probes run (review sandbox; evidence in `/tmp/stage/review-evidence/`)

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server` → exit 0 (7m58s, cold). Full log retained.
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-adapters` → **518 passed / 0 failed** (6 ignored), including:
  - `sqlite::quality_gate::tests::list_by_repo_orders_by_chain_position ... ok`
  - `sqlite::quality_gate::tests::list_by_repo_position_tie_falls_back_to_created_at ... ok`
  - `sqlite::review::tests::review_attribution_persists_and_roundtrips ... ok` (proves migration `2026-11-12-000062_gate_chain_config` actually runs on SQLite — the attribution columns it adds round-trip)
  - `sqlite::review::tests::review_without_attribution_stays_null ... ok`
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-domain` → 507+8 passed / 0 failed.
- Mechanical invariants all exit 0: `check-arch`, `check-abac-route-registry`, `check-migration-versions`, `check-migration-sql-portability`, `check-mem-port-contracts`, `check-inert-enforcement`, `check-scope-literal-defaults`, `check-fabricated-scope-defaults`, `check-forged-scope-fields`, `check-in-memory-state-stores`, `check-abac-exempt-handlers`, `check-byte-slice-truncation`, `check-relative-path-defaults`, `check-lossy-secret-conversion`, `check-dead-message-kinds`, `check-forwarded-header-trust`, `check-unbounded-external-http`, `check-fail-open-ref-resolution`.
- Commit attribution: all 7 task-135-labeled product-surface commits on the branch are in the `commits:` frontmatter.
- gyre-server test execution is blocked by the build wrapper in this stage (`cargo test -p gyre-server` → exit 64, "gyre-server code generation plus the model exceeds 4 GiB"); see "Verifier-required probes" below. This is an infrastructure restriction, not a code defect.

## Verified working (full diff inspection base→candidate)

- **Ordered gate chains (§Gate Chains).** `position` column (added earlier by task-168's `2026-10-08-000062_gate_position`; this migration correctly does NOT re-add it) now backed by: adapters `ORDER BY position, created_at` (SQLite + PG, plus an equivalent `sort_by_key` in mem), `create_gate` append-or-explicit position, and a new `GET/PUT /repos/:id/gate-chain` endpoint that accepts/returns the spec's exact `[[gates]]` TOML (PascalCase or snake_case type names, `min_approvals` per spec). PUT semantics verified by tests: replace-in-order (position = index), same-named gates keep ids (GateResults stay linked), absent pre-merge gates deleted, post-merge gates preserved and name-collisions rejected, validation entirely before first mutation (atomic; invalid chain → 400/415 with chain untouched). Route registered in `api/mod.rs` with an ABAC `RouteResourceMapping` entry (registry check passes).
- **Mixed types + parallel execution.** `trigger_gates_for_mr` (pre-existing, invoked from `merge_queue.rs:133` on enqueue) creates a Pending GateResult per gate and `tokio::spawn`s each — genuinely parallel, all configured types.
- **RequiredApprovals is now real.** Was a hollow `("Passed", "delegated to merge processor")` stub at base with no delegate; now `run_required_approvals_gate` counts distinct approving reviewers live, stays `Running` until N distinct approvals (duplicate re-approvals by one reviewer don't count — regression-tested), and `check_gates_for_mr` re-evaluates and persists the promotion to Passed on the next processor cycle.
- **Merge decision is order-independent and correct.** New two-pass `decide_gate_readiness`: required failures block (Err) even while other required gates are Pending/Running (masking bug from earlier rounds fixed, regression-tested in both orderings); required Pending/Running waits (Ok(false) → merge processor skips/requeues, confirmed at merge_processor.rs:1814 and :854); advisory gates never block in any state — including advisory RequiredApprovals gates short of their minimum. Required-gate failure path notifies the author (`notify_gate_failure` + GateFailure event in `run_gate`).
- **Optional gates recorded, non-blocking.** Advisory failures persist their result and attach an MR comment attributed to `gate-agent:{gate_id}`; required failures correctly do not.
- **Gate agent lifecycle (§Gate Agent Lifecycle).** Ephemeral/read-only/identity-scoped shipped in task-134 (scoped EdDSA JWT, exact-match scope, push denial, token revocation on completion/crash/timeout — in base). NEW here and real: **attribution** — `Review.reviewer_persona`/`gate_id` columns (migration 000062, both SQL adapters + mem), derived server-side in `submit_review` from the scoped token's `task_id` claim read via the extractor-derived `auth.scope` (array form — the fail-open string-read bug from the §1 claims merge was repaired in `25a6f463`); body fields cannot forge it, and the pre-existing reviewer-identity binding forces scoped tokens to submit under their own subject. **Context windows captured** — `capture_gate_agent_context` zstd-compresses the exact bundle the reviewer will see (persona system prompt, MR diff, spec at the pinned SHA, MR title, task description) into the conversation repository BEFORE the process runs, with a `gate_agent_context` audit event recording SHA/gate/persona/MR; validators captured too; capture is skipped (logged) when the workspace tenant is unresolvable rather than stored under a fabricated identity. Test retrieves the blob and asserts the pinned (not tip) spec content is what was captured.

## Test-inflation check

None found. New tests drive real paths: HTTP `oneshot` against the real router with real EdDSA-scoped JWTs, `run_gate` through persisted GateResults, conversation-store retrieval + JSON parse of the captured blob, and SQLite round-trips for the new columns. Each kills a real bug class (rename collisions, masking orderings, duplicate approvals, fabricated scope capture).

## Non-blocking observations (recorded for completeness)

- `specs/coverage/system/agent-gates.md` rows 6–7 cite "migration `2026-11-12-000061`"; the actual migration is `2026-11-12-000062_gate_chain_config` (000061 is `repo_access_overrides`). Comment-level typo in a coverage note; the substance of the note is accurate. Not blocking.
- Deleting a gate from a chain while an MR holds results for it leaves those results orphaned; `check_gates_for_mr` defaults unknown gates to required, so a Pending orphan blocks that MR until closed. Same semantics as the pre-existing `DELETE /gates/:gate_id` endpoint; not introduced by this task.

## Verifier-required probes (gyre-server test compilation is deferred)

Run `cargo nextest run --all` (CI test job). At minimum these gyre-server lib tests:

- `api::gates::tests::gate_chain_put_parses_spec_toml_and_orders_chain`
- `api::gates::tests::gate_chain_replace_reorders_keeps_ids_deletes_removed`
- `api::gates::tests::gate_chain_put_preserves_post_merge_gates`
- `api::gates::tests::gate_chain_invalid_entries_rejected_and_chain_unchanged`
- `api::gates::tests::gate_chain_unknown_repo_is_404`
- `api::gates::tests::update_gate_rename_to_sibling_name_rejected`
- `api::gates::tests::list_gates_orders_by_position`
- `gate_executor::tests::advisory_gate_failure_attaches_mr_comment`
- `gate_executor::tests::required_gate_failure_attaches_no_advisory_comment`
- `gate_executor::tests::required_approvals_gate_passes_when_count_met`
- `gate_executor::tests::required_approvals_gate_waits_until_approvals_arrive`
- `gate_executor::tests::required_approvals_counts_distinct_reviewers`
- `gate_executor::tests::advisory_approvals_gate_must_not_block_merge`
- `gate_executor::tests::decide_gate_readiness_failure_precedes_waiting_in_any_order`
- `gate_executor::tests::waiting_approvals_gate_must_not_mask_failed_required_gate`
- `gate_executor::tests::agent_review_captures_context_window_for_audit`
- `gate_executor::tests::agent_review_context_capture_skips_without_workspace_tenant`
- `api::merge_requests::tests::scoped_gate_review_records_persona_attribution`
- `api::merge_requests::tests::unscoped_review_has_no_gate_attribution`

Plus the PostgreSQL storage path (migration `2026-11-12-000062_gate_chain_config` runs on PG — plain `ADD COLUMN`/`DROP COLUMN`, portable by inspection and by `check-migration-sql-portability.sh`).
