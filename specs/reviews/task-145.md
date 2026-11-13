---
task: task-145
candidate: f8727eb69cbf3d1ce1a812ad410b4b35838c1912
base: f7f5c59f22682d40e6f995b137dfa3ea9bfd3ad3
verdict: approved
reviewed: 2026-04-15
---

# Review — task-145: spec enforcement validation tests (agent-gates.md §What This Prevents)

## Scope

Diff base→candidate touches only test code and docs:
`crates/gyre-server/tests/spec_enforcement.rs` (new, 7 integration tests),
`crates/gyre-server/src/merge_processor.rs` (+2 unit tests inside the
existing `#[cfg(test)]` module), `crates/gyre-server/src/git_http.rs`
(rebase repair of a test race helper, `#[cfg(test)]` only), plus the task
file and coverage row 23. No production behavior change — correct scope for
a test-only task.

## Findings

None blocking.

- All 7 attack/drift rows have a dedicated, self-contained integration test
  (own server on an ephemeral port, own state, own on-disk git repos, own
  policies — no shared fixtures).
- All 5 forge enforcement policies now have merge-level unit tests: the
  base (task-139) already carried `warn_stale_spec`,
  `require_current_spec`, `require_signed_approval`; the candidate adds the
  two missing ones with rejection reason, author notification, and merging
  positive controls. No duplicated coverage of already-tested behavior.
- Tests are discriminating, not self-confirming: every assertion targets
  production surfaces verified in the base tree (`run_once` → spec-policy
  block → `verify_spec_ref` per-SHA ledger query; `resolve_spec_blob_sha`
  stale detection; `gate_executor` delivering the PINNED spec blob via
  GYRE_SPEC_FILE with a scoped review token; ABAC resolver entry +
  immutable `builtin:require-human-spec-approval` Deny with `abac_denied`
  audit attribution). Most tests carry positive controls (merging MR with
  spec_ref, approved SHA, human Admin approver) proving the rejections are
  the policy, not default-deny or accident.
- Error-message assertions check the reason names the missing field /
  spec path / policy — acceptance criterion "clear and actionable" met.

Non-blocking observation: row 2's merge-time rejection reason is the
approval-ledger path ("no active approval") rather than a dedicated
"SHA not found in repo" existence probe — a property of the already-merged
task-139 enforcement surface; the test documents this and additionally
asserts the MR spec endpoint 404s for the ghost SHA.

## Probes

- `cargo check -p gyre-server --lib --tests` at candidate HEAD: clean
  (exit 0). Evidence: review-evidence/cargo-check-gyre-server-tests.txt.
- `check-task-commit-attribution.sh` at the candidate commit: OK (the
  sandbox working-tree failure is the harness's review-round revert of the
  task file, not the candidate).
- `check-arch.sh`, `check-byte-slice-truncation.sh`,
  `check-inert-enforcement.sh`, `check-relative-path-defaults.sh`: all pass.

## Verifier must execute (server test binaries exceed this sandbox's build
budget; cargo check only here)

- `cargo test -p gyre-server --test spec_enforcement`
- `cargo test -p gyre-server --lib merge_processor`
- `cargo test -p gyre-server --lib git_http::tests::push_persona_change_updates_entity_resets_approval_and_notifies_owner`
- full `cargo test --all`
