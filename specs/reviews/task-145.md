# Review: task-145 — Spec enforcement validation tests for attack/drift prevention

Comparison base: 5984177c2c6fcdab43e5f0071675f18245a1a8b5 → candidate
fcf4cb12f67cbf318f02e69245d0c59a65069880 (branch
`pipeline/task-145/eb2322dc8ea74a2399350a8b84944c28-1`).
Diff: 7 files, +1479/−19 — new `crates/gyre-server/tests/spec_enforcement.rs`
(1112 lines), +233 lines of policy unit tests in
`crates/gyre-server/src/merge_processor.rs` (test module), two test-module
deflakes (`notifications.rs`, `git_http.rs`), coverage-matrix + task-file
updates. No production behavior changes outside `#[cfg(test)]` modules; no
script, exemption, or verifier changes.

## Verdict

**Approved.** No material findings. The task contract — validation tests for
every attack/drift row of agent-gates.md §Part 2 "What This Prevents" — is
satisfied by tests that drive the real enforcement surfaces and fail if the
enforcement is disabled or weakened.

## R1 Findings

None blocking. Observations recorded below (O1–O3) and in
`/tmp/stage/review-evidence/probe-notes.md`.

## Evidence reviewed

### Acceptance criterion: one integration test per attack/drift row (7/7)

`tests/spec_enforcement.rs`, one test per row, each self-contained (own
`build_state` server on an ephemeral loopback port, own on-disk git repo in a
TempDir, own policies/ledger/MR state; no shared fixtures):

| Row | Test | Discriminating power |
|---|---|---|
| 1 code without a spec | `code_without_spec_is_rejected` | `require_spec_ref` set via the REST policy surface; MR without `spec_ref` → queue entry `Failed` with a reason naming `spec_ref`, MR stays open, `git show main:feature.txt` proves the commit never landed; control MR with a `spec_ref` merges. If the policy check were removed the entry would merge → test fails. |
| 2 nonexistent spec | `nonexistent_spec_reference_is_rejected` | well-formed 40-hex SHA resolving to nothing → `Failed` with "no active approval" naming the spec path; MR spec endpoint (gate agent's channel) 404s. |
| 3 unapproved spec | `unapproved_spec_is_rejected_until_approved` | real committed spec file with its real blob SHA; a Pending ledger row authorizes nothing (`is_active()` false); approving that exact (path, sha) releases the block and a fresh MR merges — proves the rejection is the approval state, not something else. |
| 4 spec changed after code written | `stale_spec_warns_or_blocks_by_policy` | same repo, spec v1→v2: under `warn_stale_spec` merge proceeds and the advisory lands as an MR comment from `forge:spec-policy` (body "Stale spec reference"); under `require_current_spec` the stale ref is `Failed` with "requires current spec". Both halves asserted in one test. |
| 5 wrong spec version | `gate_agent_reviews_pinned_sha_and_catches_wrong_version` | real reviewer script (registered `agent_review` gate, scoped token, real process) judges the MR against `GYRE_SPEC_FILE`; the wrong-version MR pins v1 (answer 42) but implements the HEAD spec (43), so if the gate ever reviewed against HEAD instead of the pinned SHA the verdict would flip to approved and the test fails — the test kills the pinned-vs-HEAD bug by construction. Correct pairing merges. |
| 6 retroactive modification | `retroactive_spec_modification_requires_new_approval` | approves SHA-1, asserts SHA-1 approval stays active while SHA-2 has none in the ledger; MR referencing SHA-2 under `require_approved_spec` → `Failed` naming the unapproved SHA. |
| 7 unauthorized approver | `unauthorized_approver_is_denied_by_abac` | registered agent JWT (real `mint`, self-consistent rogue scope) → 403, with the denial recorded in the audit trail against `builtin-require-human-spec-approval`; a human Admin control succeeds (201) and the ledger records the approver identity — proving the 403 is the human-only policy, not default-deny. |

### Acceptance criterion: unit tests for each of the 5 forge policies

Present at the candidate: `require_spec_ref_blocks_mr_without_spec_ref` and
`require_approved_spec_blocks_unapproved_spec` (added by this candidate,
merge_processor.rs test module) plus the three from task-139
(`warn_stale_spec_adds_advisory_comment_without_blocking`,
`require_current_spec_blocks_stale_spec_ref`,
`require_signed_approval_blocks_unsigned_spec_approval`). The unit tests
exercise the real policy-decision code against real in-memory state
(`spec_policies`, `spec_approvals`); the merge call itself is `NoopGitOps`
(the established `test_state()` pattern) — this does not weaken the policy
assertions, since a disabled policy lets the entry proceed to `Merged` and
the tests assert `Failed` with a specific reason plus author notification.

### Production surfaces verified real (source reads)

- `merge_processor.rs:2075-2175` — the merge-time spec-policy block
  (pre-existing task-139 code, unchanged by this candidate): blocking checks
  via `api::gates::verify_spec_ref` / `verify_signed_spec_ref` (real
  per-(path,sha) ledger queries) and staleness via
  `git_refs::resolve_spec_blob_sha`.
- `gate_executor.rs:1329-1345, 1505-1570` — agent_review gate reads the spec
  at the MR's pinned SHA through `git_ops.read_file_at_commit` (fail-closed),
  hands it to the reviewer process via `GYRE_SPEC_FILE`, mints and revokes a
  scoped review token.
- ABAC: `builtin:require-human-spec-approval` is an immutable priority-999
  Deny (gyre-domain/src/policy.rs:314-337), seeded by
  `seed_builtin_policies`, route-mapped for `/api/v1/specs/:path/approve` →
  ("spec", "approve") (abac_middleware.rs:373).
- All REST endpoints/field names used by the tests exist and match
  (`repos/:id/spec-policy`, `repos/:id/gates`, `merge-requests` with
  `spec_ref` validation, `merge-queue/enqueue` → `trigger_gates_for_mr`,
  `merge-requests/:id/{spec,comments,reviews,gates}`); `mint()` arity in
  Row 7 matches `auth.rs:267`.

### Acceptance criterion: tests pass

- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` — clean at the
  candidate (7m23s, exit 0; log in review-evidence).
- Focused test execution is blocked in this sandbox by the executor's 4 GiB
  build wrapper (exit 64, recorded) — an infrastructure restriction, not a
  code defect. Verifier must run:
  `cargo nextest run -p gyre-server --test spec_enforcement`, the
  `notifications::tests::notify_rich_falls_back_to_workspace_home_link`
  deflake, and the five merge_processor policy unit tests (exact commands in
  `/tmp/stage/review-evidence/probe-notes.md`).
- Prior CI context: run 43585665655 (ancestor head 39388628) failed only on
  the `notify_rich` flake; the candidate's de74a59f root-causes it
  (whole-second `created_at` + `created_at`-desc sort reordering across a
  second boundary) and fixes the selection to be identity-based — the
  discriminating assertion (unresolvable workspace ⇒ no `action_url`) is
  preserved, so this is a deflake, not a weakening.

### Commit attribution

All five product-surface task-145 commits (f4b7f151, 0de2f4e4, 0c052944,
6128a39c, de74a59f) are in task-145.md `commits:` frontmatter; 6128a39c is
additionally attributed in task-130.md (it supplies task-130 scope args to
the Row 7 token mint). docs/process-only commits touch no product files.

## Observations (non-blocking)

- **O1 (Row 2 interpretation).** "SHA not found" is enforced as "no active
  approval" under `require_approved_spec`: a SHA that exists nowhere cannot
  hold a ledger approval (the approve endpoint only records approvals
  matching the ledger's `current_sha`, computed from real content), and the
  test corroborates unreachability via the MR spec endpoint 404. Consistent
  with task-139's opt-in per-repo policy design; documented in the test.
- **O2 (3 of 5 unit tests pre-exist).** task-139 authored
  `require_current_spec`/`require_signed_approval`/`warn_stale_spec` tests;
  this candidate added the two missing policies' tests. All five exist at
  the candidate, satisfying the acceptance criterion.
- **O3 (CI claim not fully verifiable).** The shipped notes state the full
  spec-enforcement suite passed in run 43585665655; the retained log tail
  shows nextest cancelled 597 tests after the flake, so that specific claim
  is unconfirmed — the verifier run above is the binding evidence.
- **Working-tree note.** `specs/tasks/task-145.md` has an unstaged revert to
  the pre-task content in this sandbox — executor staging artifact, not part
  of the candidate commit (the committed file contains the full
  ready-for-review record). Left untouched per review constraints.
