# Review — task-134 (Define AgentReview and AgentValidation gate types with review protocol)

Spec: `agent-gates.md` §Gate Types (Extended), §AgentReview Gate.
Candidate: `72eafe1c4` on base `fb1f82e31` (branch `pipeline/task-134/54ed103eb24a4a5dbd58745557f790ee-1`).
Verdict: **approved** (round 5; covers the `459f51b9` repair round's migration renumber and the merge onto base `fb1f82e31`).

## Evidence (all on the exact candidate tree, `/tmp/stage/bin/cargo`)

- `cargo test -p gyre-server --lib gate_executor` → **37 passed, 0 failed**, including both real-server e2e tests (`agent_review_end_to_end_approved_verdict_passes_gate`, `agent_review_end_to_end_changes_requested_fails_gate` — this sandbox permits loopback accept, so they ran, not skipped: the approved-path test asserts the live Review API returned 201 and the review row is bound to the token subject) and both TTL regression tests.
- `cargo test -p gyre-server --test task134_review_probe` → **7 passed, 0 failed** (scoped-token allow-list, push denial, identity binding, validator cannot submit review, revoked-token rejection, lookalike scope).
- `cargo test -p gyre-server --lib git_http` → **49 passed**; `api::merge_requests` → **33 passed**; `api::gates` → **12 passed** (incl. `create_agent_review_gate`, `create_agent_validation_gate` with validation_type round-trip through list).
- `cargo test -p gyre-adapters --lib -- save_upsert no_duplicate_migration migration_000058` → **3 passed** (the three tests the `459f51b9` F1 finding named; duplicate-version collision is gone — `check-migration-versions.sh` rc=0, migration is `2026-10-09-000059_gate_validation_type`, no version collision with the base's `2026-10-10-000056_user_tenant_id`).
- Mechanical checks all rc=0: `check-migration-versions`, `check-migration-sql-portability`, `check-fabricated-scope-defaults`, `check-abac-route-registry`, `check-abac-exempt-handlers`, `check-arch`, `check-mem-port-contracts`, `check-byte-slice-truncation`, `check-inert-enforcement`, `check-forwarded-header-trust`, `check-scope-literal-defaults`, `check-task-commit-attribution`.
- Freeze probe: appending a temporary entry to `fabricated-scope-defaults-exemptions.txt` fails `check-fabricated-scope-defaults.sh` (rc=1); restored baseline passes (file restored verbatim, tree clean after).

## Mutation probes (tests proven discriminating; source restored after each)

- **A — fixed TTL:** `gate_token_ttl_secs` → constant 360: `review_token_ttl_covers_configured_gate_timeout` and `validation_token_ttl_covers_configured_gate_timeout` both FAIL. The TTL-follows-gate-window behavior is real.
- **B — allow-list disabled:** `is_reviewer`/`is_validator` forced false in `abac_middleware.rs`: `scoped_review_token_denied_outside_allowlist` and `validation_scoped_token_can_read_mr_context`'s sibling `validation_scoped_token_cannot_submit_review` FAIL. (`scoped_review_token_cannot_push` still passes under this mutation — correct: push denial is an independent layer in `git_http.rs`, defense in depth.)
- **C — identity binding removed:** `submit_review` falls back to body `reviewer_agent_id`: `scoped_review_token_reviewer_identity_is_bound` FAILS.

## Findings from prior rounds — resolved

- `459f51b9` F1 (duplicate Diesel version `20261009000056`): fixed by renumbering to `2026-10-09-000059`; version check and all three named tests pass on this tree.
- `459f51b9` F2 (stale evidence claim): Shipped section now cites the re-run suite; I re-ran the named tests independently and they pass.
- `31c35ddb` findings (freeze slack, non-discriminating allow-list test): freeze enforces count 1 with growth failing (probe above); `scoped_review_token_denied_outside_allowlist` now seeds builtin policies and fails under mutation B.

## Contract verification

- **Gate type variants / CRUD:** `GateType::AgentReview`/`AgentValidation` with `QualityGate.validation_type` (nullable column, migration 000059, both SQLite and Postgres adapters); `POST/GET /api/v1/repos/:id/gates` accept and return both types with `persona`/`validation_type`/`required` (round-trip asserted through list).
- **Spawn protocol:** persona resolved nearest-wins repo → workspace → tenant (workspace lookup failure skips the tenant scope, logs — no fabricated "default"); unresolvable persona, missing MR/repo, or unresolvable `spec_ref` (unknown SHA or absent path) fails the gate before any process or token mint (asserted: no `gate-*` tokens in the store after context-gathering failure). MR context is real: full diff via `git_ops.diff`, spec content at the pinned SHA via the new `read_file_at_commit` port (e2e asserts the agent received Spec v1 from the pre-tip SHA, not tip's v2), MR title, and task description via MR → author agent → task.
- **Scoped token:** `mint_scoped` issues `review:submit` (reviewers) / `validation:report` (validators) with `jti`, registered in `agent_tokens` (auth extractor rejects unregistered/removed tokens — `revoked_scoped_token_is_rejected`). Enforcement layered and exact-match: `git_http` 403s push for both scopes before repo resolution; ABAC middleware allow-lists only each capability's routes (validators get no review-submission route); `submit_review` binds the reviewer identity to the token subject (forged body id ignored, proven e2e). TTL = configured `timeout_secs` + 60s grace on both mint sites (mutation A).
- **Verdict mapping:** Approved → Passed, ChangesRequested → Failed with the review body surfaced in gate output; only the gate's own agent's verdict counts; exit-0-without-review fails. No-command gates fail closed (the old auto-approve/auto-pass stubs are gone).
- **Teardown:** token revoked (`kv_remove`) on every exit path (verdict, timeout, spawn failure — asserted by `gate_token_revoked_after_review_completes`/`_validation_completes`); temp spec/diff files removed.
- **`cargo test --all`:** not run here (owned by verification); every focused suite claimed and relevant to the diff passes independently.

## Non-blocking nit (no action required)

- `crates/gyre-ports/src/git_ops.rs`: the `revert_commit` doc comment carries a duplicated paragraph (merge artifact from the rebase; the base's paragraph was re-added alongside the identical existing one). Documentation only, zero behavior — noted for the next toucher to drop the duplicate.

## Notes for verification

- The two e2e tests skip in sandboxes that deny loopback `accept()`; here loopback works and both ran for real. If verification runs in a restricted sandbox, the always-run guard `e2e_review_submission_is_abac_allowed_with_seeded_policies` (oneshot, no listener) plus the probe suite cover the same regression class.
