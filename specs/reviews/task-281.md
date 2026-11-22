# Review — task-281 (Impact analysis: cascade test results and per-dependent test status served by the API and shown in the UI)

Spec: `specs/system/dependency-graph.md` §Impact Analysis (gap `spec-cab97078efffe0fbfb7339be` / `impact-cascade-results-ui`).
Candidate under review: `8be33dfaba8dfcdd16faf2e557f19f5fb1f99366` (branch `pipeline/task-281/75d39a60dd124f2cae90834db56e70d8-2`, base `82aed3dbddb5a5be8a195b8ffb789bcb2030c644`; product commits `71bda29a` + `82d862ac`, both attributed in the task frontmatter; the five `process` commits touch only `specs/tasks/task-281.md`).
Verdict: **approved** — no findings. The working tree at review time carried only the executor's re-checkout of `specs/tasks/task-281.md` (a documented checkpoint artifact, not part of the product diff); no source was modified during review.

## Probes run (evidence under `/tmp/stage/review-evidence/`)

- `scripts/check-abac-route-registry.sh` → exit 0 (`abac-route-registry.txt`; both new routes resolve in the registry, exemption file untouched — the diff touches no `scripts/` file).
- `scripts/check-abac-exempt-handlers.sh` → exit 0, 86 handlers (`abac-exempt-handlers.txt`).
- `scripts/check-arch.sh` → exit 0 (`check-arch.txt`).
- `/tmp/stage/bin/cargo check -p gyre-server` → exit 0, 7m38s (`cargo-check-gyre-server.txt`).
- `/tmp/stage/bin/cargo check -p gyre-server --tests` → exit 0, 3m59s (`cargo-check-gyre-server-tests.txt`) — the prior round's failure class (test-cfg `expect_err` on non-`Debug` types, fixed by deriving `Debug` on the two response structs) is closed; the ten new tests compile.
- `/tmp/stage/bin/cargo fmt --check -p gyre-server` → exit 0 (`cargo-fmt.txt`).
- `web`: `npm ci` (locked deps) then `npx vitest run src/__tests__/ImpactAnalysisModal.test.js` → **23/23 passed** (`npm-ci.txt`, `vitest-impact-modal.txt`).
- Full read of the base→candidate diff (10 files); grep sweeps: no optional-chained `cascadeTestResults?.`/`triggerCascadeTests?.` anywhere in `web/src`; `cascade` in `web/src/lib/api.js` finds the two real one-line implementations against `/repos/{id}/cascade-test-results` (GET) and `/repos/{id}/cascade-tests/trigger` (POST), matching the registered axum routes and ABAC mappings (`repo` resource, `write` action on the trigger); no route-pattern conflicts (all `/api/v1/repos/:id/...` siblings are distinct static literals, so no router-build panic).

## Verified against the acceptance criteria

- **AC1 (blast radius carries test status + results).** `BlastRadiusResponse` now carries `dependents: [{repo_id, direct, test_status}]` and `cascade_test_results: [CascadeTestResultResponse]` (dependencies.rs). `test_status` is derived from the dependent's most recent durable `CascadeTestResult` **against this repo** — `list_by_merged_repo` is newest-first in every adapter (port doc + mem/SQLite/PG impls verified), so the first match per dependent is its latest result; results for other merged repos correctly report `unknown` (dedicated test `blast_radius_test_status_ignores_results_for_other_merged_repos`). The BFS core (direct/transitive/dedup/total) is unchanged. Spec's example lines ("tests passing"/"tests unknown", "Cascade test results: not yet run") now have a real server-side source; the modal renders "Cascade test results: not yet run" on the empty store.
- **AC2 (durable GET + gated POST trigger).** `GET /api/v1/repos/{id}/cascade-test-results` reads the task-280 `CascadeTestResultRepository` port (durable under the SQLite/PG adapters wired via `store!`), 404 on unknown repo. `POST /api/v1/repos/{id}/cascade-tests/trigger` reuses the **same production path as the merge** — `merge_processor::trigger_cascade_tests`, now returning `CascadeTriggerOutcome {created_task_ids, skipped_dependents}` (merge-path caller unchanged, ignores the return). Policy gating mirrors the merge path: merged-repo workspace `require_cascade_tests = false` → 409 refusal before anything is created; each dependent's own workspace policy respected (opted-out dependents skipped and counted, tested). Per-handler authorization loads the repo, resolves its workspace, fails closed on a dangling workspace reference, and compares the workspace's tenant against the caller's (403 on mismatch, tested with no tasks created). SHA handling is fail-closed: explicit `merge_commit_sha` or default-branch HEAD via `resolve_ref`; unresolvable HEAD → 400, never a fabricated empty SHA. Ten server tests cover all of these paths with real state (type-checked here; execution recorded for the verifier below per the 4 GiB budget rule).
- **AC3 (web client + modal).** `cascadeTestResults`/`triggerCascadeTests` are real methods in `web/src/lib/api.js`; the modal's fetch is a real call (no `?.` no-op), the test-status column populates from `blastRadius.dependents[].test_status` with a same-store fallback derivation from the results records and defaults to `unknown` — the old hardcoded `'--'` and the dead `cascadeResult?.status` lookup are gone; the cascade-results section renders per-result dependent, short SHA, failure details, timestamp, and Pass/Fail; the trigger button performs the real call, re-fetches on success, and surfaces refusals (e.g. 409) in a `cascade-error` element instead of swallowing them. Vitest suite (23 tests) passed in this sandbox; the column-population test fails if the population logic is removed (asserts passing/failing/unknown and `not.toContain('--')`), and the trigger tests assert the real call, the re-fetch, and the surfaced refusal — mutation-sensitive, not self-confirming. The mocks are shape-faithful to the verified serde response (`id, dependent_repo_id, merged_repo_id, merge_commit_sha, cascade_task_id, passed, failure_details, recorded_at`).
- **AC4 (mechanical checks).** Both ABAC scripts exit 0 with the new endpoints; no exemption-file growth (no `scripts/` changes in the diff).

Test-inflation check: none found. Server tests drive the real handlers with real mem-backed state, real dependency edges, and real persisted `CascadeTestResult` records, asserting concrete statuses, task counts/types/workspace/SHA-pinning, and error variants (`NotFound`/`Conflict`/`Forbidden`/`InvalidInput` map to 404/409/403/400 in `error.rs`, verified). Web tests are mock-based (necessarily) but shape-faithful and behavior-killing.

## Minor observations (non-blocking, recorded for completeness)

- The hand-maintained `**Coverage:** 12/16 (3 n/a)` header line in `specs/coverage/system/dependency-graph.md` is now stale: rows count 13 (4 implemented + 9 verified) after row 17's promotion. Row statuses themselves are accurate, and the verifier regenerates `SUMMARY.md` by recounting rows, so nothing propagates; PM may refresh the count line.
- The trigger's per-handler authorization compares tenant only, not workspace — appropriate: the trigger mints tasks in *dependent* workspaces (which may differ from the merged repo's workspace), tenant containment is the correct boundary for that effect, callers may be tenant-scoped (`workspace_id: None`), and the route is registered in the ABAC resolver (middleware policy evaluation applies on top). The enforced invariant only mandates per-handler authorization for resolver-exempt routes.
- `Option<Json<TriggerCascadeTestsRequest>>` maps any body-rejection (including malformed JSON) to `None` — the body is optional by design and the fallback is a real HEAD resolution, so this is robustness-neutral.
- The web tests exercise the fallback derivation path (`MOCK_BLAST_RADIUS` lacks `dependents`); the primary `dependents[].test_status` path is server-tested and trivial in the modal.

## Verifier must run (gyre-server test execution exceeds this sandbox's budget; type-checked only)

In `crates/gyre-server` (api/dependencies.rs tests module):

- `cargo test -p gyre-server blast_radius_reports_dependent_test_status_and_results`
- `cargo test -p gyre-server blast_radius_test_status_uses_most_recent_result`
- `cargo test -p gyre-server blast_radius_test_status_ignores_results_for_other_merged_repos`
- `cargo test -p gyre-server cascade_test_results_endpoint_returns_durable_records`
- `cargo test -p gyre-server cascade_test_results_endpoint_unknown_repo_is_404`
- `cargo test -p gyre-server trigger_endpoint_creates_cascade_tasks_for_dependents`
- `cargo test -p gyre-server trigger_endpoint_refused_when_policy_disables_cascade_tests`
- `cargo test -p gyre-server trigger_endpoint_skips_dependent_whose_workspace_opted_out`
- `cargo test -p gyre-server trigger_endpoint_denied_across_tenants`
- `cargo test -p gyre-server trigger_endpoint_without_sha_fails_closed_on_unresolvable_head`

Plus the pre-existing `dependencies.rs` and `merge_processor.rs` suites (the `trigger_cascade_tests` signature change is call-compatible; the merge-path caller ignores the outcome), full workspace suite, web `npm test`, and Clippy per standard verification.
