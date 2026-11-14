# Review — task-145 (agent-gates §Part 2 "What This Prevents" — spec enforcement validation tests)

Spec: `specs/system/agent-gates.md` §Part 2 "What This Prevents" — validation
tests for all 7 attack/drift rows of the spec-to-code binding table, plus
merge-level unit coverage for the 5 forge enforcement policies.

Candidate: `446fc28ecfc7b8dfdc90e0a7b98f032a6c265f44` (base `b6892403`).
Product-surface commits: `f4b7f151` (the test suite), `6128a39c` (mint-arity
repair after the base merge brought task-130 token scoping), `0c052944`
(notification-listing arity adaptation), `0de2f4e4` (notification-race poll
fix in an unrelated touched test). All four are listed in the task's
`commits:` frontmatter (attribution verified manually; the script itself
times out iterating full history in this sandbox). Verdict: **complete**.

## What was reviewed

Diff base→candidate touches only `crates/gyre-server/tests/spec_enforcement.rs`
(new, 1112 lines), `crates/gyre-server/src/merge_processor.rs` (two new unit
tests inside the existing `mod tests`), `crates/gyre-server/src/git_http.rs`
(test-module-only poll helper), and specs/coverage + task-file docs. No
production behavior was changed — correct for a validation-test task.

## Acceptance criteria

- **7 integration tests, one per attack/drift row** — ✓
  `tests/spec_enforcement.rs`, each test documented with its spec-table row,
  each self-contained (own server on an ephemeral loopback port, own state,
  own real on-disk git repos, own policies — no shared fixtures). They drive
  real surfaces end to end: REST policy config (`PUT /repos/:id/spec-policy`),
  REST MR creation + queue enqueue, the real merge-processor cycle
  (`merge_processor::run_once` — the same `process_next` the spawned loop
  runs), real git blob SHAs from real commits, real merges through the git2
  adapter, the real spec-approval ledger, a real `agent_review` gate whose
  reviewer is a real bash process comparing the MR diff against the spec
  content at the PINNED SHA and submitting its verdict through the review API
  with a scoped token, and the real ABAC middleware over a live HTTP listener
  for the unauthorized-approver row (403 + audit trail naming
  `builtin-require-human-spec-approval` + no ledger row + a human-Admin
  control that succeeds, proving the denial is the human-only policy and not
  default-deny).
- **Unit tests for the 5 forge policies** — ✓ The candidate adds the two
  that lacked merge-level coverage (`require_spec_ref`,
  `require_approved_spec`), each with a rejection case (terminal `Failed`
  entry + reason naming the missing artifact + author notification) and a
  positive control that merges. `warn_stale_spec`, `require_current_spec`,
  and `require_signed_approval` already had merge-level unit tests at the
  base (task-139); per the goal ("new tests for already-working behavior —
  not needed, not wanted") not duplicating them is correct. All 5 policies
  are covered at merge level.
- **Self-contained** — ✓ (see above).
- **Clear error messages** — ✓ the tests assert the production reason
  strings name the missing thing ("spec_ref", "no active approval" + spec
  path/SHA, "requires current spec"), so a vague reason fails the test.
- **Tests pass** — compile-verified on the exact candidate (see probes).
  Execution evidence: the full suite ran green on tree `9b9954f1`
  (1974/1975, the single failure an unrelated notification race fixed by
  `0de2f4e4`, included in this candidate). The only deltas in the
  task-145 test files since that run are mechanical arity adaptations
  required by merged main (`mint` scope args, `list_for_user` filter arg) —
  verified by direct diff and traced through the production signatures;
  none changes test semantics (the Row 7 denial is the immutable
  `builtin:require-human-spec-approval` Deny on `subject.type != "user"`
  only; hierarchy claims do not widen or narrow it, and
  `builtin-agent-repo-scope` is disabled by default).

## Probes run (evidence in /tmp/stage/review-evidence/)

- `cargo check -p gyre-server --test spec_enforcement` → **exit 0** (7m23s;
  includes the web-asset build). This is the exact probe that exposed the
  prior round's E0061; it is resolved on the candidate.
- `cargo check -p gyre-server --tests` (all test targets, incl. the new
  merge_processor unit tests and the git_http test change) → **exit 0**.
- `cargo test -p gyre-server --test spec_enforcement` → **blocked by the
  executor's build wrapper** (gyre-server test-binary codegen exceeds the
  review build budget — infrastructure restriction, not a code defect).
  Verifier must execute: `cargo test -p gyre-server --test spec_enforcement`,
  `cargo test -p gyre-server --lib merge_processor`,
  `cargo test -p gyre-server --lib git_http`, then the full `cargo test --all`.
- Mechanical gates on the candidate tree: check-arch, byte-slice-truncation,
  in-memory-state-stores, inert-enforcement, relative-path-defaults,
  lossy-secret-conversion, scope-literal-defaults,
  fabricated-scope-defaults — all OK.

## Mutation sensitivity (static analysis)

The assertions bind to production outputs, not mirrored logic: terminal
queue-entry statuses (`Failed`/`Merged` — both terminal, so an
absence-style false pass is impossible), production-generated error
strings, MR status via REST, on-disk git state (`main:feature.txt` must not
resolve after a blocked merge), gate verdicts recorded by a real reviewer
process, HTTP status codes through the real router/ABAC stack. Removing any
enforcement branch the tests cover flips a `Failed`→`Merged` (or vice
versa) outcome and fails the corresponding assertion.

## Observations (non-blocking, recorded for completeness)

- Row 2 ("nonexistent spec: SHA not found") is enforced through the
  approval-ledger path (`require_approved_spec` → "no active approval") —
  that is the merge-time enforcement surface task-139 shipped (in the base,
  out of task-145's scope); the test documents this mapping and additionally
  proves the pinned SHA genuinely resolves nowhere (MR spec endpoint 404).
  The spec-row behavior — MR rejected — is validated.
- Row 4 does not pre-approve SHA-1 (the plan sketch mentioned approval),
  but no approval-requiring policy is configured in that repo; the behavior
  under test (warn-and-proceed vs block) is fully exercised with a
  production-written advisory MR comment asserted.
- Row 7's rogue token is minted with a literal `"default"` tenant scope
  arg — an intentional self-consistent rogue scope in test code; the
  scope-fabrication gates (which target production lookup fallbacks) pass.
- Working-tree note (not a candidate defect): the review sandbox's
  `specs/tasks/task-145.md` was reset to the base template before this
  review started (controller stage artifact, present in the
  before-snapshot); the candidate commit's own task-file content (progress,
  `commits:`, shipped notes) is intact at `446fc28e`.
- Coverage row 23 (`What This Prevents`) moved to `implemented` with an
  honest "independent verification pending" note and the verifier probes
  recorded — consistent with the goal's no-fake-completion rule.
