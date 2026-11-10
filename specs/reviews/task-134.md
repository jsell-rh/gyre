# Review — task-134 (agent-gates.md §Gate Types (Extended) + §AgentReview Gate)

Spec: `specs/system/agent-gates.md` Part 1 §Gate Types through §AgentReview Gate.
Candidate under review: `18ccb3e756fe855a9d5865bf3dc3d1d6643507e6` (merge `3cbf86679` onto base
`dbf9edaeae5c8a5441bd9ff50c9dff3cec99172a`, plus verification-repair round `33a60bc0f`
and process commits `069cbcc32`/`18ccb3e75`). This review continues a long chain of
prior rounds (findings `31c35ddb`, `459f51b9`, TTL repair `72c5bb03`, merges onto
`fb1f82e31`/`b5a872087`/`dbf9edaea`); none of the prior approvals is carried forward —
every claim below was re-probed independently on the exact candidate tree.

Verdict: **approved** (no findings).

## Evidence (all on the clean candidate tree, HEAD = `18ccb3e75`)

Focused probes (via `/tmp/stage/bin/cargo`; output retained in
`/tmp/stage/review-evidence/`):

- `cargo test -p gyre-server --lib gate_executor` → **37 passed, 0 failed**, including
  both real end-to-end tests (`agent_review_end_to_end_approved_verdict_passes_gate`,
  `agent_review_end_to_end_changes_requested_fails_gate`) — no SKIP: this sandbox
  permits loopback accept (confirmed against `/tmp/stage/capabilities.json`), so the
  tests bound a real axum server with the full require_auth + ABAC stack, backed a real
  bare git repo with the spec pinned at a pre-tip SHA, and drove the fixture
  `review_agent_driver.sh` through the live Review API with the scoped JWT. The
  assertions prove the whole §AgentReview Gate chain: persona prompt, MR title, task
  description (acceptance criteria), spec content at the pinned SHA, spec_ref, full
  diff via `GYRE_DIFF_FILE`, `http_code=201` submission, reviewer identity bound to the
  token subject (forged `reviewer_agent_id` in the body ignored), verdict→status
  mapping both directions, and token teardown (no `gate-*` tokens left in
  `agent_tokens`).
- `cargo test -p gyre-server --test task134_review_probe` → **7 passed, 0 failed**.
- `cargo test -p gyre-server --lib api::gates` → 12/12 (AgentReview/AgentValidation
  CRUD incl. `validation_type` round-trip through list).
- `cargo test -p gyre-server --lib api::merge_requests` → 33/33; `--lib abac` → 38/38.
- `cargo test -p gyre-adapters --lib quality_gate` → upsert round-trip
  (`save_upsert_updates_all_config_columns`, which also guards the `required` column
  drift class) passes; `--lib read_file_at_commit` → 3/3 (pinned-content-at-old-SHA,
  missing-file→None, unknown-SHA→Err — the fail-closed ref-resolution contract).

Discriminating-power probes (temporary mutations, tree restored to clean and
re-verified with `git status`):

- **Allow-list mutation** (`is_reviewer`/`is_validator` forced `false` in
  `abac_middleware.rs`): `scoped_review_token_denied_outside_allowlist` and
  `validation_scoped_token_cannot_submit_review` both **FAIL** — the 403s are
  attributable to the scoped-token allow-list, not an empty policy store.
- **Identity-binding mutation** (`if false && scope == REVIEW_SUBMIT_SCOPE` in
  `submit_review`): `scoped_review_token_reviewer_identity_is_bound` and
  `e2e_review_submission_is_abac_allowed_with_seeded_policies` both **FAIL** — the
  reviewer-identity binding is real enforcement, not a mirrored assertion.

Mechanical checks (all exit 0 on the candidate tree): `check-arch.sh`,
`check-abac-route-registry.sh` (duplicate literals at the frozen 17 baseline after the
`33a60bc0f` const dedupe — registry and allow-list now share the exact same
`MR_*_ROUTE` consts, so they cannot drift), `check-migration-versions.sh` (the
renumbered `2026-10-09-000059_gate_validation_type` holds),
`check-migration-sql-portability.sh`, `check-fabricated-scope-defaults.sh`
(freeze at 1 entry; an independently-run growth probe — temporary extra entry —
fails with rc=1, then restored and baseline re-passes),
`check-mem-port-contracts.sh`, `check-byte-slice-truncation.sh`,
`check-inert-enforcement.sh`, `check-task-commit-attribution.sh`, and
`check-rustfmt-diff.py dbf9edaea` (changed lines clean, 17 files).
Focused `cargo clippy -p gyre-server --lib` / `-p gyre-adapters --lib` compile with no
warnings in any changed file; the workspace-wide clippy script is rejected by the
review-stage cargo wrapper (infrastructure, not a code defect) and is deferred to
verification, as is `cargo test --all`.

## Acceptance-criteria cross-check

- **Variants defined**: `GateType::AgentReview`/`AgentValidation` in `gyre-common`;
  `QualityGate.validation_type` persisted by SQLite and Postgres adapters
  (migration 000059, portable SQL) with an upsert round-trip test.
- **CRUD accepts/stores config**: `api/gates.rs` accepts and returns `persona`,
  `required`, `validation_type`; round-trip asserted through the list endpoint.
- **Spawn protocol**: persona resolved nearest-wins (repo → workspace → tenant, no
  fabricated scope; unresolvable persona or spec_ref fails the gate before any
  process spawns); MR context gathered for real (full diff via `git_ops.diff`,
  spec content at the pinned SHA via the new `read_file_at_commit` port where an
  unknown SHA is an `Err`, MR title, task description via MR → author agent → task);
  scoped JWT minted with a unique `jti` and registered in `agent_tokens`.
- **Agent receives context**: proven by the live e2e assertions listed above.
- **Token scoped to `review:submit` only**: exact-match capability comparison in
  three layers — ABAC route allow-list (reviewer: MR detail/reviews/diff/comments/
  gates + version; validator: read-only MR detail/diff + version),
  `git_receive_pack` 403 read-only push denial (checked before repo resolution),
  and `submit_review` binding reviewer identity to the token subject. Validators
  (`validation:report`) cannot reach the review route at all. Lookalike scopes
  (`review:submit:admin`, `xreview:submit`) do not inherit any capability.
- **Verdict mapping**: Approved → Passed, ChangesRequested → Failed with the review
  body surfaced; only the gate's own agent's verdict counts
  (`review_verdict_ignores_reviews_from_other_agents`); exit-0-without-review fails
  (fail-closed, replacing the old no-command auto-approve fallback).
- **Teardown**: token revoked (`kv_remove`) on every exit path — verdict, timeout,
  spawn failure, empty command — asserted by `gate_token_revoked_after_review_completes`
  / `gate_token_revoked_after_validation_completes` and the e2e teardown assertion;
  temp spec/diff files removed.
- **Token TTL**: both mint sites derive TTL from the configured gate window via the
  shared `gate_token_ttl_secs` helper (timeout + 60s grace); regression tests on both
  paths assert `exp − iat` covers a configured 700s window.
- **`cargo test --all`**: owned by verification; every focused suite touching the
  diff is green (see above).

## Non-blocking observations (recorded for completeness, no action required)

- On the timeout path the spawned agent process is not explicitly killed
  (`kill_on_drop` is not set), so an orphaned process could outlive the gate window;
  its token is already revoked and push is denied for the scope, so it retains no
  capability — operational hygiene only.
- Coverage rows 4/5 in `specs/coverage/system/agent-gates.md` remain `task-assigned`;
  this matches the repo convention (rows flip at task completion, cf. task-199/
  task-201 also ready-for-review with unflipped rows), and the shipped-notes claims
  were independently corroborated by the probes above rather than trusted.
