# Review — task-145 (Spec enforcement validation tests for attack/drift prevention)

Spec: `specs/system/agent-gates.md` §Part 2 "What This Prevents" (7 attack/drift rows).
Assigned candidate: `9b9954f18b0cdbd7bf804578206d4f88ccf2a91a` (base `b9487b1b`).
Product commit under review: `f4b7f151` (test suite + merge_processor unit tests; carried
byte-identical through `e2a13024` and the base merge `d78d004a`).
Verdict: **approved** — see `/tmp/stage/verdict.json`. No blocking findings.

## Scope of the diff (base → candidate)

`crates/gyre-server/tests/spec_enforcement.rs` (new, 1099 lines),
`crates/gyre-server/src/merge_processor.rs` (+232 lines, entirely inside `mod tests`),
`specs/coverage/system/agent-gates.md` (row 23 → implemented),
`specs/tasks/task-145.md` (shipped/test-evidence sections). No production-code changes.

## What was verified

### 1. Every driven surface is real production code (static trace, candidate tree)

- **Policy gate ordering** — `merge_processor.rs` ~1927-2070: the five forge policies are
  evaluated *before* `git_ops.merge_branches` is invoked; a violation fails the queue entry
  (`MergeQueueEntryStatus::Failed` + reason) and notifies the author
  (`report_spec_policy_violation`). `require_spec_ref` fires on `mr.spec_ref.is_none()`;
  `require_approved_spec`/`require_signed_approval` go through
  `api::gates::verify_spec_ref`/`verify_signed_spec_ref`, which query
  `spec_approvals.find_by_spec_sha(path, sha)` per exact (path, sha) version and require
  `is_active()` (Pending/Revoked/Rejected never authorize) — so Row 2 (nonexistent SHA),
  Row 3 (pending ledger row), and Row 6 (SHA-2 after retroactive edit) all reduce to real
  per-SHA ledger lookups that genuinely reject.
- **Stale-spec handling** — `resolve_spec_blob_sha` against the real repo path; warn path
  writes a deduped MR comment from `forge:spec-policy` whose body contains
  "Stale spec reference"; require_current path fails the entry with
  "requires current spec". Matches Row 4's assertions.
- **Row 5 (wrong spec version)** — `gate_executor.rs` `run_agent_review_gate` →
  `build_review_agent_context` resolves the spec content at the **pinned** blob SHA via
  `git_ops.read_file_at_commit` (fail-closed when unresolvable), writes it to
  `GYRE_SPEC_FILE`, the MR patch to `GYRE_DIFF_FILE`, mints a scoped `review:submit` JWT
  (`gate-review-{uuid}`), and maps the submitted verdict
  (`check_review_verdict`: Approved→Passed, ChangesRequested→Failed). The merge
  processor's candidate selection fails entries whose required gates failed. The test's
  reviewer script (bash+curl) therefore judges MR1 (implements v2 content, pins v1)
  against v1 and returns `changes_requested`; if the gate read HEAD instead of the pinned
  SHA, MR1 would be approved and the test would fail — the test is genuinely
  discriminating on the pinned-SHA contract.
- **Row 7 (unauthorized approver)** — route `POST /api/v1/specs/:path/approve` is in the
  ABAC resolver (`"spec"`, `"approve"`); the immutable
  `builtin:require-human-spec-approval` Deny (priority 999, `subject.type != user`)
  beats `admin-all-operations` (900); denial returns 403 and records an `AbacDenied`
  audit event with `policy_id = builtin-require-human-spec-approval` (policy **id**,
  hyphenated — matches the test's assertion). The human control path is real: API-key
  auth sets `agent_id = user.display_name`, the handler requires the ledger's
  `current_sha` to match, and writes an active `SpecApproval` row with
  `approver_id = "user:{display_name}"`.
- **Row 2's 404** — `GET /merge-requests/:id/spec` does a real `git_refs::read_blob` and
  404s for an unreachable SHA.
- **Real merges** — `build_state` wires the real git2 `GitOpsPort`; `merge_branches`
  performs a true three-way tree merge and ref update (works on the non-bare fixtures).

### 2. Test quality (no inflation)

Each of the 7 integration tests would fail if its enforcement were disabled (blocked
cases assert `Failed` + MR stays open + feature commit absent from `main`; every test
also carries a positive control that merges). Self-contained: own server on an ephemeral
loopback port, own in-memory state via `build_state`, own TempDir git repos, own
policies — no shared fixtures. Rejection reasons are asserted to name the missing
field/spec path/SHA (clear-and-actionable AC). The 5 forge policies have merge-level unit
tests: 2 new (`require_spec_ref_blocks_mr_without_spec_ref`,
`require_approved_spec_blocks_unapproved_spec`) + 3 pre-existing
(`require_current_spec_blocks_stale_spec_ref`,
`require_signed_approval_blocks_unsigned_spec_approval`, `warn_stale_spec` advisory) —
not duplicating the existing three is correct per the goal's "no new tests for
already-working behavior".

### 3. Probes run (evidence in `/tmp/stage/review-evidence/`)

- `cargo check -p gyre-server --tests` (via `/tmp/stage/bin/cargo`, stage=review) →
  **exit 0** on the exact candidate tree (`cargo-check-gyre-server-tests.log`,
  `-run2.log`; fresh `spec_enforcement` check fingerprints written at run completion —
  the suite type-checks against the merged head, including the task-182 base-merge
  changes). Server test *execution* is refused by the build wrapper (4 GiB budget) —
  deferred to the verifier, see below.
- `bash scripts/check-task-commit-attribution.sh` → **exit 0, OK**
  (`attribution-gate.log`; `e2a13024` task-labeled product-surface merge commit is
  covered by the frontmatter list).
- `bash scripts/check-arch.sh` → **exit 0** (`arch lint passed`).
- GitHub API (reachable from this sandbox): check-runs for head `e2a13024` —
  **Test (`cargo nextest run --all`) = success**, Clippy/Format/Architecture lint/Web
  build/e2e/agent-runner all success; only "Development controller" failed, at step 6
  (`python3 -m unittest discover -s scripts -p 'test_dev*.py'`; go step 5 passed) —
  job-log download requires admin rights (403), so the exact failing test inside the
  step is not extractable locally.
- CI-repair reproduction: `python3 -m unittest discover -s scripts -p 'test_dev*.py'`
  at the candidate → **61 tests, OK, exit 0** in a scrubbed environment
  (`dev-unittest-run3-scrubbed-nolock.log`). Intermediate runs isolated the two earlier
  local errors as sandbox artifacts: `test_dev_build_command` fails only with
  `GYRE_PIPELINE_STAGE=review` exported (the script-under-test's own stage gate exits 64
  — run 1), and `test_dev_cargo_clean` times out only while a concurrent cargo probe
  holds the shared `/tmp/stage/build.lock` (run 2; passes in isolation and in run 3).
- `python3 -m unittest discover -s scripts -p 'test_pipeline*.py'` → 2 failures
  (`test_real_timeout_kills_check_and_records_infrastructure_code`,
  `test_crash_cleanup_stops_durable_agent_before_capturing_source`) — **both reproduce
  byte-identically on the base commit `b9487b1b`** (verified in a throwaway worktree),
  i.e. pre-existing/environmental, not candidate defects; that CI step was *skipped* in
  run 43419972221 (it never ran on this head), and the candidate diff does not touch
  `scripts/`, `tools/`, or `.github/`.

## Deferred checks for the independent verifier

1. `cargo test -p gyre-server --test spec_enforcement` (needs a loopback listener —
   supported in this sandbox class; confirm in the verifier environment).
2. `cargo test -p gyre-server --lib -- merge_processor::tests::` (at minimum the two new
   policy tests).
3. A green CI run on the published head, specifically the "Development controller" job
   (`test_dev*.py` + `test_pipeline*.py` steps) — the run-43419972221 failure is not
   reproducible on candidate content locally, and its surface is untouched by the diff,
   so a green re-run closes it as a runner-side flake.

## Non-blocking observations

- **Row 2 mechanism note.** The spec table's mechanism cell says "SHA not found"; the
  merge-time rejection reason the test asserts (and production emits) is the per-SHA
  approval-ledger check ("no active approval … naming the spec path"), because a SHA
  that never existed can have no ledger approval. The test documents this mapping and
  additionally proves unreachability via the MR spec endpoint's real `read_blob` 404.
  The observable contract of the row (MR referencing a nonexistent spec is rejected) is
  enforced and tested; the "SHA not found" wording also exists verbatim on the
  gate-context path (`gate_executor.rs`: "does not resolve") and the spec endpoint 404
  body. Not a defect; recorded so a future auditor sees the mapping was examined, not
  missed.
- **Unit-test git backend.** The two new `merge_processor.rs` unit tests use
  `mem::test_state()` (NoopGitOps). This is fine and consistent with the three existing
  policy unit tests: the policy gate fires before any git operation, so the Failed
  assertions prove real enforcement; the Noop merge only backs the positive control.
  Real end-to-end merges are covered by the integration suite.
- **CI head vs candidate tree.** CI ran on `e2a13024`; the candidate adds the base merge
  (`d78d004a`, task-182 concept-view changes in `api/graph.rs`, `api/mod.rs`,
  `abac_middleware.rs`, `llm_defaults.rs`, `view_query*`) plus process/docs commits. The
  task-145 test files are byte-identical between the two, and `cargo check --tests`
  passes on the exact merged tree; the deferred test execution closes the residual gap.

## Verdict

**Approved.** All acceptance criteria are met with real, discriminating, self-contained
tests; the enforcement surfaces they drive are production code (verified by direct source
trace); type-check, attribution, and architecture gates pass on the exact candidate; and
the candidate's own test surface is green in CI on a tree byte-identical in the tested
files. Server test-binary execution is deferred to the verifier per the build-budget
split (commands listed above).
