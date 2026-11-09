# Review — task-134 (Define AgentReview and AgentValidation gate types with review protocol)

Spec: `specs/system/agent-gates.md` §Gate Types (Extended) + §AgentReview Gate (coverage rows 4-5).
Candidate: `91b8d11ce2fb2ea6182675b099aca47f18f9564a` on `pipeline/task-134/41d4e31263b94ca5b5f67183f3858c75-2`, base `fefde2946`.
Product commits in scope: `b3678e27` (checkpoint carrying the gate-executor/auth/ABAC/adapter work), `31c35ddb` (review-repair round), `531434a68` (jti fix after rebase); `05c99b4e` is the rebase merge, `6e66ae7b6`/`91b8d11ce` touch only `specs/tasks/task-134.md`. Attribution check passes (`check-task-commit-attribution.sh`).

Verdict: **complete** (approved).

## Evidence (all at HEAD = candidate; logs in `/tmp/stage/review-evidence/`)

Focused test runs (`SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test`, shared target dir; cold build 10m21s):

- `--test task134_review_probe`: **7/7** — reviewer identity binding, allow-list denial, push denial (403 + message), revoked-token 401, validator cannot submit review, validator can read MR context, lookalike-scope exact-match.
- `--lib gate_executor`: **37/37** — including both real-server e2e tests (`agent_review_end_to_end_approved_verdict_passes_gate`, `..._changes_requested_fails_gate`; loopback is permitted in this sandbox, so they **ran**, not skipped: real axum server, real bare git repo with spec pinned at a pre-tip SHA, `review_agent_driver.sh` submitting through the live Review API with the scoped JWT), both TTL regression tests, both token-revocation tests, fail-closed no-command/missing-MR/unresolvable-persona tests, verdict-mapping tests, and the no-loopback ABAC regression guard.
- `--lib api::gates`: 7/7 (create_agent_review_gate, create_agent_validation_gate with validation_type round-trip through list).
- `--lib submit_` (merge_requests): 6/6; `--lib auth`: 61/61.
- `cargo test -p gyre-adapters quality_gate` 1/1 (`save_upsert_updates_all_config_columns` — upsert persists validation_type AND required), `read_file_at_commit` 3/3 (pinned content at pre-tip SHA, missing file → None, unknown SHA → Err).
- `cargo check -p gyre-cli --tests`, `-p gyre-domain --tests`: clean (the `QualityGate.validation_type` field has no other construction sites).

Mutation probes (tree restored and verified clean after each):

1. **ABAC scoped-token allow-list disabled** (`abac_middleware.rs:994` `if !allowed` → `if false`): `scoped_review_token_denied_outside_allowlist` and `validation_scoped_token_cannot_submit_review` **FAIL** (validator got 201). Discriminating.
2. **Reviewer identity binding disabled** (`merge_requests.rs:1029` condition → `if false`): `scoped_review_token_reviewer_identity_is_bound` and `e2e_review_submission_is_abac_allowed_with_seeded_policies` **FAIL**. Discriminating.

Mechanical checks at candidate: arch, abac-route-registry, abac-exempt-handlers, mcp-write-tools, dead-message-kinds, byte-slice-truncation, relative-path-defaults, fail-open-ref-resolution, lossy-secret-conversion, scope-literal-defaults, inert-enforcement, migration-sql-portability, forged-scope-fields, forwarded-header-trust, in-memory-state-stores, unbounded-external-http, mem-port-contracts, migration-versions, task-commit-attribution — **all PASS**.
Freeze probe on `check-fabricated-scope-defaults.sh`: baseline (1 entry) passes; a temporarily appended second entry fails rc=1; file restored byte-identical, baseline re-passes — the count=1 freeze has no slack.

## Contract verification

- **Gate types**: `GateType::AgentReview`/`AgentValidation` in `gyre-common` (present since `d7940e857`, carried by this branch); `QualityGate.validation_type` added with migration `2026-10-09-000056` (next free 000056 slot at this branch point; numbering check passes; portable `ALTER TABLE ... ADD COLUMN`), persisted by both SQLite and Postgres adapters with a discriminating upsert round-trip test.
- **Gate CRUD**: `POST/GET /api/v1/repos/:id/gates` accepts/returns persona, required, validation_type for both new types (tests assert round-trip).
- **Spawn protocol** (`gate_executor.rs`): persona resolved nearest-wins repo → workspace → tenant (no fabricated tenant scope; unresolvable workspace skips tenant scope with a log); unresolvable persona or `spec_ref` fails the gate **before** any process or token is minted (test asserts zero `gate-*` tokens in the store). MR context is real: full diff via `git_ops.diff`, spec content at the pinned SHA via the new `read_file_at_commit` port (unknown SHA is an Err, not silent None — port test proves it), MR title, and task description via MR → author agent → task.
- **Scoped token**: `mint_scoped` emits `review:submit` (reviewers) / `validation:report` (validators) with `jti` and TTL = `timeout_secs + 60` (both mint sites; regression tests assert `exp − iat ≥ 700 + 60` for a configured 700s window). Enforcement is layered and exact-match: `git_http` push denial (403 before repo resolution), ABAC middleware route allow-list (validators get read-only MR routes, no review submission), and `submit_review` binds the reviewer identity to the token subject (forged `reviewer_agent_id` ignored — asserted by oneshot probe and the live e2e driver).
- **Verdict mapping**: only the gate's own agent's review counts; Approved → Passed, ChangesRequested → Failed with the review body surfaced; exit-0-without-review fails. Both e2e tests drive the real chain including `http_code=201` through the live API.
- **Teardown**: token revoked (`kv_remove`) on every exit path — verdict, timeout, spawn failure, empty command (asserted by tests); temp spec/diff files removed. No agent record or ABAC policy is created for the gate agent, so nothing else remains to clean up.
- **Merge integration**: `merge_queue.rs:89` triggers gates on enqueue; `check_gates_for_mr` honors required/advisory semantics for the new types.

## Minor (not blocking, recorded for completeness)

- On gate timeout the spawned agent process is not killed (`tokio::process` default `kill_on_drop=false`); the token is revoked on that path, so no capability survives — it is a process-leak nit, not an enforcement gap.
- `GYRE_SPEC_CONTENT` still carries the full spec text as an env var alongside `GYRE_SPEC_FILE`, so a spec larger than the Linux per-string exec limit (~128 KiB) would fail the spawn with E2BIG (fail-closed, confusing error). The diff, by contrast, is file-only. Trimming the env copy to a stub (file is the carrier) would remove the ceiling.
- The reviewer allow-list includes `POST /api/v1/merge-requests/:id/comments` — defensible as review-feedback surface, and still gated by normal ABAC policy evaluation; noting it as a conscious scope choice.
