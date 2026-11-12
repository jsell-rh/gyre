# Review — task-135 (Implement gate chain configuration and gate agent lifecycle)

Spec: `specs/system/agent-gates.md` §Gate Chains (Composable Policies), §Gate Agent Lifecycle.
Base `4a203a066a75707001607bbe17dd8e6e616a9fa9` → candidate `44d159317bd0594ab4ce34f21d9ce6a9498469a8`.
Product commits: `583c4a07d` (implementation), `d71555220` (review-repair f0f41ed), `6ccbdff17` (merge reconciliation with task-118 base), `2999bb5e7` (verification-repair dbfc3754 — test-only: scoped-reviewer JWT minted with `&state.base_url` instead of a hardcoded `localhost:0` issuer; verified single-line diff).
Verdict: **approved** — real implementations for both coverage sections; no blocking findings.

## Probes run (evidence: /tmp/stage/review-evidence/)

- Mechanical invariant scripts, all exit 0: check-arch, check-abac-route-registry, check-abac-exempt-handlers, check-api-auth, check-migration-versions, check-migration-sql-portability, check-mem-port-contracts, check-fabricated-scope-defaults, check-scope-literal-defaults, check-in-memory-state-stores, check-inert-enforcement, check-lossy-secret-conversion, check-byte-slice-truncation, check-relative-path-defaults, check-forwarded-header-trust, check-unbounded-external-http, check-fail-open-ref-resolution, check-forged-scope-fields, check-dead-message-kinds.
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0 (all new test code compiles).
- `/tmp/stage/bin/cargo test -p gyre-domain --lib` → 466 passed, 0 failed.
- `/tmp/stage/bin/cargo test -p gyre-adapters --lib sqlite::` → 316 passed, 0 failed, including the new chain-ordering (`list_by_repo_orders_by_chain_position`, tie-break test), gate-upsert position round-trip, and review-attribution persistence tests.
- gyre-server test *execution* is blocked by the build wrapper's 4 GiB budget (exit 64, recorded) — infrastructure restriction, not a code defect. Required verifier probes listed below.

## Verified working

**§Gate Chains:**
- **Ordered chains, all backends.** `quality_gates.position` (migration `2026-11-12-000061`, next unused sequence after 000060, portable standard SQL); SQLite and Postgres `ORDER BY position, created_at`; mem adapter sorts identically (`check-mem-port-contracts` passes); `create_gate` appends the next free position; `GET /repos/:id/gates` and `GET /repos/:id/gate-chain` return chain order (the port's `list_by_repo_id_and_phase` default filters the ordered list, so ordering survives phase filtering).
- **TOML chain configuration.** `PUT/GET /repos/:id/gate-chain` accepts and returns the spec's exact `[[gates]]` document (mixed gate types, `required`, `min_approvals`; PascalCase or snake_case type names). PUT is atomic — validation-before-mutation, 400 leaves the stored chain untouched (test asserts stored gate ids identical after duplicate-name, missing-command, missing-min_approvals, unknown-type, malformed-TOML, and JSON→415 rejections); same-named gates keep ids (prior GateResults stay linked); absent pre-merge gates deleted; `position` = array index; post-merge recovery gates neither deleted nor re-phased, name collisions rejected (regression test `gate_chain_put_preserves_post_merge_gates`). Route registered in `api/mod.rs` and ABAC-mapped (`gate` resource) — registry and exempt-handler scripts pass.
- **Parallel execution / wait / block / notify.** Enqueue triggers `trigger_gates_for_mr` (one tokio task per gate). `check_gates_for_mr` decides via the pure two-pass `decide_gate_readiness`: required failures → `Err` (merge processor fails the queue entry; `notify_gate_failure` delivers failure details to the author — pre-existing path, fires on gate failure regardless of selection order) taking precedence over waiting; required Pending/Running → `Ok(false)` requeue. Unit test pins both orderings of the waiting/failed masking case.
- **Optional gates never block.** `required = false` gates are neither waited on nor blocking in any state — including RequiredApprovals gates short of their minimum (regression tests `advisory_approvals_gate_must_not_block_merge`, `decide_gate_readiness_failure_precedes_waiting_in_any_order`). Advisory failures attach an MR comment attributed to `gate-agent:{gate_id}`; required failures post no advisory comment.
- **RequiredApprovals really counts.** Distinct reviewers with Approved reviews (duplicate approvals by one reviewer do not satisfy min-2 — test); live re-evaluation each processor cycle promotes a waiting gate to Passed, persisted via `update_status`.

**§Gate Agent Lifecycle:**
- Ephemeral/read-only/identity-scoped shipped in task-134 (scoped `review:submit` JWT, push denied, token revoked on exit incl. crash/timeout) — unchanged and re-confirmed.
- **Attribution.** `Review.reviewer_persona` + `Review.gate_id` (migration 000061, both SQL adapters + mem round-trip tests) derived server-side in `submit_review` from the signature-verified scoped token's `task_id` claim (auth.rs attaches `jwt_claims` only after `agent_signing_key.validate` succeeds; `review:submit` is minted only by `mint_scoped`). The test posts a forged `reviewer_agent_id` in the body and asserts the server-side binding overrides it; unscoped reviews carry no attribution.
- **Context windows captured for audit.** `capture_gate_agent_context` stores the exact bundle the agent sees (persona system prompt, MR diff, spec at the pinned SHA, MR title, task description) zstd-compressed through the port-backed conversation repository *before* the subprocess runs, plus a `gate_agent_context` audit event with the conversation SHA; retrievable at `GET /api/v1/conversations/:sha` (endpoint exists, lib.rs:848). Validators are captured too. Unresolvable tenant scope → capture skipped and logged, never a fabricated "default" identity (fabricated-scope scripts pass). The test decompresses the stored blob and asserts the pinned-sha spec content ("not the v2 tip"), the diff, the persona prompt, and the task description.

## Test-inflation check

None found. New tests drive the real router (`oneshot` on `api_router`), the real gate executor (including a real `true` subprocess and the fail-closed exit-0-no-verdict path), or the pure decision function with both orderings pinned; each fails if the behavior it guards regresses.

## Required verifier probes (gyre-server execution blocked by the 4 GiB wrapper)

- `cargo test -p gyre-server --lib api::merge_requests::tests::scoped_gate_review_records_persona_attribution` (the dbfc3754 repair; fails 401-vs-201 before `2999bb5e`, passes after)
- `cargo test -p gyre-server --lib api::merge_requests::tests::unscoped_review_has_no_gate_attribution`
- `cargo test -p gyre-server --lib gate_chain`
- `cargo test -p gyre-server --lib required_approvals`
- `cargo test -p gyre-server --lib check_gates`
- `cargo test -p gyre-server --lib advisory`
- `cargo test -p gyre-server --lib agent_review`
- `cargo test -p gyre-server --lib decide_gate_readiness`
- `cargo test -p gyre-server --lib ralph_loop::tests::gate_failure_respawns_agent_with_fresh_context`
- Task acceptance criterion: `cargo test --all`

## Non-blocking observations (recorded for completeness)

- Deleting/replacing gates while MRs are in flight leaves orphan gate results; `check_gates_for_mr` defaults unknown gates to `required = true`, so a removed gate's Pending/Running result requeues its queue entry indefinitely. The semantics is pre-existing (single-gate `DELETE /repos/:id/gates/:gate_id` + base `unwrap_or(true)` default); the chain PUT mirrors the same per-name delete. A future task could fail-close orphaned results on gate deletion.
- `notify_gate_failure` fires for advisory gate failures too (pre-existing path; non-blocking, and the new advisory MR comment adds visibility).
- Gate-agent approvals count toward RequiredApprovals distinct-reviewer minimums; the spec does not define reviewer eligibility, and distinct-identity counting is a defensible reading.
- No web/ source changes in this candidate; `web/dist/` regeneration is owned by the verifier per checkpoint policy.
