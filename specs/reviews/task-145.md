# Review — task-145 (Spec enforcement validation tests for attack/drift prevention)

Spec: `specs/system/agent-gates.md` §Part 2 "What This Prevents" (coverage row 23).
Candidate: `006175449891d7d33f664e715cd76a8b900058f0` (base `349a32ac`).
Commits under review: `f4b7f151` (tests), `0de2f4e4` (race fix), `b2d9556e`/`00617544` (docs/process).
Verdict: **approved** — no findings.

## Probes run (independent, this round)

- `cargo check -p gyre-server --lib --tests` on the exact candidate tree →
  **EXIT=0, clean** (cold, 8m57s; evidence
  `/tmp/stage/review-evidence/cargo-check-server.txt`). The lib and all test
  targets were checked — confirmed via check artifacts
  (`deps/libspec_enforcement-*.rmeta` produced during the run) and a follow-up
  `cargo check -p gyre-server --test spec_enforcement -v` reporting `Fresh
  gyre-server`. This matters because the candidate merges base `349a32ac`,
  which brought a search-indexer refactor into `api/merge_requests.rs` and
  `merge_processor.rs`; the merged tree compiles including
  `tests/spec_enforcement.rs`.
- `bash scripts/check-task-commit-attribution.sh` → **OK, exit 0** at the
  candidate (evidence: `check-task-commit-attribution-pass.txt`). The first run
  failed only because the session's working tree carried the known executor
  attribution-clobber (unstaged revert of `specs/tasks/task-145.md` to base —
  the mechanism documented in task-208 review rounds 121-123). The candidate
  commit `00617544` records both task-labeled commits
  (`0de2f4e4`, `f4b7f151`) in its frontmatter; the task file was restored to
  the candidate's committed content (`git checkout -- specs/tasks/task-145.md`;
  the review guard excludes the task file from its hash set by design) and the
  gate re-run passes.
- Static trace of every production surface the tests exercise (see
  `/tmp/stage/review-evidence/review-record.md` for line-level citations):
  policy enforcement block in `merge_processor.rs:1934-2050`,
  `verify_spec_ref`/`verify_signed_spec_ref` per-(path,sha) ledger queries,
  `git_refs::resolve_spec_blob_sha` `specs/`-prefix fallback, the full
  agent_review gate chain (enqueue → `trigger_gates_for_mr` → pinned-SHA spec
  delivery via `GYRE_SPEC_FILE` → scoped-token review submission →
  `check_review_verdict` → queue-entry failure on a required failed gate), and
  the `builtin:require-human-spec-approval` ABAC deny path including the
  `abac_denied` audit detail and the API-key human-approver control.

## Independent test-pass evidence

- Durable verification finding (source `9b9954f1`, same base): full
  `cargo test --all` = **1974/1975 passed**; the single failure
  (`git_http::tests::push_persona_change_updates_entity_resets_approval_and_notifies_owner`,
  "owner must be notified") is the test race this candidate fixes in
  `0de2f4e4`. `tests/spec_enforcement.rs` is byte-identical between `9b9954f1`
  and the candidate, so all 7 integration tests and both new unit tests passed
  in that run.
- The race fix is legitimate, not masking: post-receive persona processing
  runs in a spawned task (`git_http.rs:995`) that creates the approval task
  (step 3) before notifying the owner (step 4); the old test checked the inbox
  exactly once inside that window. `wait_for_notification` polls with the same
  bounded 30s deadline as the pre-existing `wait_for_task_title` and still
  asserts failure if the notification never appears; content assertions are
  unchanged.
- Deferred to the independent verifier (review-stage wrapper rejects
  gyre-server test codegen under the 4 GiB budget):
  - `cargo test -p gyre-server --test spec_enforcement`
  - `cargo test -p gyre-server --lib merge_processor`
  - `cargo test -p gyre-server --lib git_http::tests::push_persona_change_updates_entity_resets_approval_and_notifies_owner`
  - `cargo test --all`

## Acceptance criteria

- **7 attack/drift integration tests** — one per spec-table row, each
  documented with its row in a header comment: `code_without_spec_is_rejected`,
  `nonexistent_spec_reference_is_rejected`, `unapproved_spec_is_rejected_until_approved`,
  `stale_spec_warns_or_blocks_by_policy`,
  `gate_agent_reviews_pinned_sha_and_catches_wrong_version`,
  `retroactive_spec_modification_requires_new_approval`,
  `unauthorized_approver_is_denied_by_abac`. ✓
- **5 forge-policy unit tests** — `require_spec_ref` and
  `require_approved_spec` added at merge level in `merge_processor.rs` (tests
  mod); `warn_stale_spec`, `require_current_spec`, `require_signed_approval`
  already had merge-level unit tests at base (lines 9164/9307/9453). All five
  covered; duplicating the three existing ones would be test inflation. ✓
- **Self-contained** — every test starts its own server on an ephemeral
  loopback port with its own state, seeds its own workspace/repo/policies/MRs,
  and tears down via TempDirs. No shared fixtures. ✓
- **Clear error messages** — every rejection assert checks the reason names
  the cause (missing `spec_ref`, `no active approval` + spec path/SHA,
  `requires current spec` + both SHAs); author notification asserted in the
  unit tests. ✓
- **Tests pass** — see evidence above; verifier runs recorded. ✓

## Bug-kill analysis (no self-confirming tests)

Each test fails if its enforcement is disabled: removing the policy block
merges the attack MRs (Failed asserts break); a path-only (SHA-blind) ledger
query would let the v1 approval satisfy SHA-2 in the retroactive test; a gate
that reviewed HEAD instead of the pinned SHA would approve the wrong-version
MR and merge it; removing the ABAC policy yields 201 for the rogue agent. The
warn path asserts the advisory MR comment, not a log line. Positive controls
in rows 1, 3, 5, 7 guard against always-fail/false-positive readings.

## Deviations noted (non-blocking, documented in the tests)

- Row 2 ("SHA not found"): production has no standalone merge-time
  SHA-existence rejection — the only merge-time checks are the five per-repo
  policies. The test configures `require_approved_spec` (a SHA never in the
  repo can never have a ledger approval), asserts the rejection names the
  spec path, and additionally proves the pinned SHA is unresolvable (MR spec
  endpoint 404s). Faithful to the actual enforcement surface.
- Row 4 plan step says "Approve spec at SHA-1" first; the test omits the
  approval because the staleness checks (`warn_stale_spec` /
  `require_current_spec`) are approval-independent in production.
- `0de2f4e4` touches a test outside task-145's own surface (git_http persona
  push test) — justified by the durable verification failure on that exact
  test; test-only, bounded poll, assertions unchanged.

## Scope check

No production code changes: both `merge_processor.rs` (+232) and `git_http.rs`
(+47/-12) diffs are entirely inside `#[cfg(test)] mod tests`. No new routes,
migrations, or scripts. Coverage row 23 marked `implemented` with an honest
"independent verification pending" note naming the deferred probes.
