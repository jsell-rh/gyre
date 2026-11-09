# Review — task-134 (Define AgentReview and AgentValidation gate types with review protocol)

Spec: `specs/system/agent-gates.md` §Gate Types (Extended) + §AgentReview Gate (coverage rows 4–5).
Candidate: `ca5c86654` (base `4118553f1`), branch `pipeline/task-134/57dea822618044e4abdad4df46d47607-1`. Scoping per `commits:` frontmatter: `31c35ddb2`, `b3678e276` (product code recovered across interrupted checkpoint runs; merge/bookkeeping commits `94029b2ff`/`f9725aa46`/`ca5c86654` verified to add no unreviewed product surface — the full base..candidate diff was inspected file-by-file).
Verdict: **complete** (approved).

## Test runs (this checkout, at `ca5c86654`, tree clean)

- `cargo test -p gyre-server --test task134_review_probe` → **7/7 passed** (reviewer identity binding, allow-list denial, push denial, revocation, validator cannot submit review, validator reads MR context, lookalike-scope exact-match).
- `cargo test -p gyre-server --lib gate_executor` → **37/37 passed**, including both e2e tests. Re-ran `agent_review_end_to_end_*` with `--nocapture`: no SKIP message — this sandbox permits loopback accept, so both e2e tests ran the real path (live axum server + real bare git repo + `review_agent_driver.sh` child process submitting through the live Review API with a real scoped JWT).
- `cargo test -p gyre-server --lib api::gates` → **7/7 passed** (`create_agent_review_gate`, `create_agent_validation_gate` incl. `validation_type` round-trip through list).
- `cargo test -p gyre-adapters read_file_at_commit` → **3/3 passed** (pinned-SHA content, missing file → None, unknown SHA → Err).
- `cargo test -p gyre-adapters save_upsert_updates_all_config_columns` → **1/1 passed**.

## Mutation probes (each reverted; `git status --short` clean after)

- **TTL**: `gate_token_ttl_secs` → fixed 360 (the old bug) → `review_token_ttl_covers_configured_gate_timeout` and `validation_token_ttl_covers_configured_gate_timeout` both **FAIL**. Both regression tests are anchored to the production mint path.
- **Identity binding**: `submit_review` uses `req.reviewer_agent_id` unconditionally → `scoped_review_token_reviewer_identity_is_bound` **FAILS** (forged id would be stored).
- **ABAC allow-list**: scoped-token check removed from `abac_middleware` → `scoped_review_token_denied_outside_allowlist` and `validation_scoped_token_cannot_submit_review` both **FAIL** (a validator could submit an approval). Containment is genuinely enforced, not incidental.

## Mechanical checks

PASS: `check-arch`, `check-migration-versions`, `check-fabricated-scope-defaults`, `check-abac-exempt-handlers`, `check-abac-route-registry`, `check-mem-port-contracts`, `check-fail-open-ref-resolution`, `check-lossy-secret-conversion`, `check-forged-scope-fields`, `check-forwarded-header-trust`, `check-in-memory-state-stores`, `check-unbounded-external-http`, `check-byte-slice-truncation`, `check-relative-path-defaults`, `check-inert-enforcement`, `check-migration-sql-portability`, `check-task-commit-attribution`.

Four checks fail but **reproduce identically at the base commit `4118553f1`** in a clean worktree (awk dialect error inside `check-crypto-verify` itself; `check-assertionless-tests`/`check-dead-test-code` exit 2 with no findings; `check-conditional-test-guards` flags pre-existing `web/src/__tests__/WorkspaceSettings.test.js` guards in files this candidate never touched) — pre-existing environment/base issues, not candidate defects.

Exemption-freeze growth probe: appending a third entry to `fabricated-scope-defaults-exemptions.txt` now **FAILS** `check-fabricated-scope-defaults.sh` (exit 1, "grew to 3 entries (baseline: 2)") — the 31c35ddb repair genuinely closed the two-slot slack.

## Verified against the acceptance criteria

- **Gate type variants**: `GateType::AgentReview`/`AgentValidation` in `gyre-common/src/gate.rs`; `QualityGate.validation_type` added in domain, both adapters, schema, and migration `2026-10-09-000056_gate_validation_type`. Diesel version = date+sequence, so the version is unique (`20261009000056` ≠ `20261008000056` ≠ `20261010000056`); `check-migration-versions.sh` passes and the migration runs — no silently-dropped-DDL hazard. (Note: the sequence number 000056 was not the numerically-next unused 000058, but the repo already contains date-distinguished 000056/000057 pairs at base and the enforced invariant is version uniqueness, which holds.)
- **Gate CRUD**: `POST/GET /api/v1/repos/:id/gates` accept/return both types plus `validation_type`; round-trip test passes through the real SQLite adapter (upsert test additionally proves `.set()` updates every mutable config column, including `required`).
- **Spawn protocol**: `build_review_agent_context` resolves the persona nearest-wins (repo → workspace → tenant; unresolvable workspace skips the tenant scope with a warn — no fabricated "default"), and fails the gate **before** minting or spawning on missing MR/repo/persona or unresolvable `spec_ref` (unknown pinned SHA is an error via `read_file_at_commit`, not a silent None). MR context is real: full diff via `git_ops.diff`, spec content at the pinned SHA (e2e proves the reviewer sees v1, not the branch-tip v2), MR title, and task description via MR → author agent → task.
- **Scoped token**: `AgentSigningKey::mint_scoped` mints an EdDSA JWT carrying exactly `review:submit` (reviewers) or `validation:report` (validators). Enforcement is layered and independently mutation-verified: `git_http` push denial (403, checked before repo resolution), ABAC middleware route allow-list (reviewers: MR detail/reviews/diff/comments/gates/version; validators: MR detail/diff/version only), and `submit_review` binds the reviewer identity to the token subject so a forged `reviewer_agent_id` in the body is ignored (e2e + probe both prove it). Capability comparisons are exact-match — the lookalike-scope probe (`review:submit:admin`, `xreview:submit`) proves no substring inheritance.
- **Verdict mapping**: `check_review_verdict` — only the gate's own agent's review counts; Approved → Passed, ChangesRequested → Failed with the review body surfaced; exit-0-without-review fails (the old auto-approving stub is deleted — fail-closed, "cannot determine state" ≠ "state is fine").
- **Teardown**: `revoke_gate_token` (kv_remove) runs on every exit path (verdict, timeout, spawn failure, empty command); `revoked_scoped_token_is_rejected` proves the revoked JWT 401s; temp spec/diff files removed. No per-agent ABAC policies are ever created for gate agents, so the plan's "remove ABAC policies" step has nothing to clean — the allow-list is claim-based, and revocation removes the identity.
- **Token TTL**: derived from `gate.timeout_secs + 60` on both mint sites; both regression tests fail against the fixed-360 mutation.

## Notes (non-blocking, no action required for this task)

- `gyre-ports/src/git_ops.rs`: the `revert_commit` doc comment now carries a duplicated paragraph (the task-134 edit re-inserted the "Reverting a non-tip merge…" block). Cosmetic only; no behavior impact.
- A scoped gate agent authenticates with the platform-wide agent-role posture (tenant `default`, no workspace claim), so within its allow-listed routes it can read other MRs — same exposure class as all existing spawned-agent JWTs and strictly narrower than before (allow-list + push denial are new containment); cross-MR/MR-route scoping is platform model, not this task's contract.

Findings:

- (none)
