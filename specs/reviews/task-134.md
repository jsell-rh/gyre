# Review: TASK-134 — AgentReview / AgentValidation gate types with review protocol

**Reviewer:** Independent review agent
**Date:** 2026-04-11
**Candidate:** `838a65fe452ed2d8c106fa5954e1ffbdb74abbce` (base `c5deca801a331e049f35b181f42d898df24e7e2b`)
**Verdict:** needs-revision (2 minor findings; core contract verified)

---

## Scope reviewed

Diff base..candidate: 24 files, +2267/−163. `gate_executor.rs` (AgentReview /
AgentValidation execution), `auth.rs` (`mint_scoped`), `abac_middleware.rs`
(scoped-token route allow-list), `git_http.rs` (push denial), `api/gates.rs` /
`api/merge_requests.rs` (CRUD + reviewer identity binding), adapters
(`read_file_at_commit` port, `validation_type` persistence, migration
`2026-10-09-000056`), `tests/task134_review_probe.rs`, driver fixture, docs,
exemption-file bookkeeping.

## What was verified (independently, with evidence)

Evidence and command outputs under `/tmp/stage/review-evidence/`.

- **Gate types & CRUD**: `AgentReview`/`AgentValidation` variants present in
  `gyre-common`; `QualityGate.validation_type` persisted by SQLite + Postgres
  (upsert round-trip test green); CRUD accepts/returns `validation_type`
  (api::gates 6/6, including list round-trip).
- **Spawn protocol** (`gate_executor.rs`): persona resolved nearest-wins
  (repo → workspace → tenant, no fabricated scope); unresolvable persona /
  spec_ref / MR fails the gate before any process spawns or token is minted
  (tests assert no `gate-*` token leaks on the fail path). Spec content read
  at the pinned SHA via `read_file_at_commit` (unknown SHA is `Err`, missing
  file is `Ok(None)` → gate fails with "does not resolve"); pinned-SHA test
  proves v1-at-SHA is read, not the branch-tip v2. MR title and task
  description (acceptance-criteria carrier) resolved via MR → author agent →
  task.
- **Scoped token**: `mint_scoped` mints `review:submit` (reviewers) /
  `validation:report` (validators); TTL = `timeout_secs + 60` via the shared
  `gate_token_ttl_secs` helper (both TTL regression tests green). Enforcement
  is layered and exact-match: `git_http` denies push 403 before repo
  resolution; ABAC middleware allow-lists each capability's routes only;
  `submit_review` binds the reviewer identity to the token subject.
  **Mutation-verified**: disabling the identity binding makes
  `scoped_review_token_reviewer_identity_is_bound` FAIL; disabling the ABAC
  allow-list makes `validation_scoped_token_cannot_submit_review` FAIL
  (validator submits a review, 201). Both mutations restored; tree clean.
- **Verdict mapping**: Approved → Passed, ChangesRequested → Failed with the
  review body surfaced; only the gate's own agent's verdict counts;
  exit-0-without-review fails. Unit tests + two real-server e2e tests (real
  axum server, real bare git repo, `review_agent_driver.sh` driving the live
  Review API over loopback HTTP) — both e2e tests ran (no skips) in this
  sandbox: 37/37 gate_executor tests green.
- **Teardown**: token revoked (`kv_remove`) on every exit path (verdict,
  timeout, spawn failure, empty command); temp spec/diff files removed;
  `revoked_scoped_token_is_rejected` proves the revoked token 401s.
- **Mechanical checks** all pass on the merged tree: fabricated-scope-defaults,
  migration-versions, arch, migration-sql-portability, abac-route-registry,
  scope-literal-defaults, mem-port-contracts, mcp-write-tools,
  inert-enforcement, dead-message-kinds, task-commit-attribution.
- **Focused suites**: gate_executor 37/37, task134_review_probe 7/7,
  api::gates 6/6, api::merge_requests 32/32, git2_ops read_file_at_commit 3/3,
  sqlite quality_gate upsert 1/1. (`cargo test --all` owned by verification.)

The functional task contract is genuinely implemented; no fakery found in the
gate execution, token scoping, or verdict paths.

## Findings

- [ ] **F1 (process): `FROZEN_EXEMPTION_COUNT` not lowered as claimed.** The
  shipped notes state "FROZEN_EXEMPTION_COUNT lowered 4→2 to match the
  remaining entries" and the exemption file header now says "frozen at 2
  entries", but `scripts/check-fabricated-scope-defaults.sh:35` still reads
  `FROZEN_EXEMPTION_COUNT=4` — no commit in base..candidate touches the
  script. The freeze therefore retains two slots of silent-growth slack,
  contradicting the any-growth-fails freeze principle the file's own new
  comment cites, and the shipped claim is false as written. Fix: lower the
  constant to 2.
- [ ] **F2 (test): `scoped_review_token_denied_outside_allowlist` is
  non-discriminating.** The test does not seed builtin ABAC policies, so its
  403 comes from the empty policy store's default-deny rather than the
  allow-list; verified by mutation (it passes with the allow-list disabled
  while the sibling `validation_scoped_token_cannot_submit_review` correctly
  fails). Its comment's premise ("the agent role would otherwise be
  permitted") is not established in the test state. Fix: seed builtin
  policies so the denial is attributable to the allow-list. The allow-list
  enforcement itself IS covered by the discriminating sibling test, so this
  is a test-quality repair, not an enforcement gap.

## Notes for the verifier

- Both e2e tests and the full task134_review_probe suite ran without skips in
  this sandbox (loopback HTTP permitted); no transport restrictions applied.
- The `GateType` variants themselves pre-date this candidate (present in
  base); the candidate's contribution is the execution protocol, scoping, and
  persistence, which are reviewed above.
- `cargo test --all` and the complete mechanical-check suite remain owned by
  the verification stage.
