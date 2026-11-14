---
title: "Implement spec enforcement validation tests for attack/drift prevention"
spec_ref: "agent-gates.md §Part 2 What This Prevents"
depends_on:
  - task-139
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §What This Prevents"
commits: ["0d3c6a58147ae47ff616371d8db02b49c15883f2", "6128a39c8fa30617634c5ac192529ee469506ffb", "0c0529449e16bbb3ae93b099f5b5907fc2aa791b", "0de2f4e4cb9b591e951718aa7bba459e77479152", "f4b7f15195215cd8695e682404343ea4f1db325a"]
---

## Spec Excerpt

From `agent-gates.md` §Part 2 What This Prevents:

| Attack/Drift | How Binding Prevents It |
|---|---|
| Code without a spec | MR rejected: no spec_ref |
| Code referencing a nonexistent spec | MR rejected: SHA not found |
| Code referencing an unapproved spec | MR rejected: no approval for that SHA |
| Spec changed after code was written | Warning or rejection (policy-dependent) |
| Agent implements wrong spec version | Gate agent reviews against pinned SHA, catches mismatch |
| Retroactive spec modification | Approval is on a specific SHA. Changing the spec creates a new SHA requiring new approval. |
| Spec approved by unauthorized person | Approval ledger records approver identity; ABAC controls who can approve |

## Implementation Plan

1. **Create integration test suite** `tests/spec_enforcement.rs` that validates each attack/drift scenario:

   a. **Code without a spec:**
      - Create an MR with no `spec_ref` field
      - Configure `require_spec_ref` policy on the repo
      - Assert: merge processor rejects the MR with a clear error message

   b. **Nonexistent spec reference:**
      - Create an MR with `spec_ref: "specs/nonexistent.md@abc123"`
      - Assert: merge processor rejects — SHA not found in repo

   c. **Unapproved spec:**
      - Create a spec file and commit it (getting a real SHA)
      - Create an MR referencing that SHA — but do NOT approve the spec
      - Configure `require_approved_spec` policy
      - Assert: merge processor rejects — no approval for that SHA

   d. **Stale spec (spec changed after code written):**
      - Approve spec at SHA-1, create MR referencing SHA-1
      - Modify the spec (creating SHA-2)
      - With `warn_stale_spec`: assert warning emitted but merge proceeds
      - With `require_current_spec`: assert merge blocked

   e. **Retroactive spec modification:**
      - Approve spec at SHA-1
      - Modify the spec (SHA-2)
      - Assert: approval for SHA-1 still valid, but SHA-2 has no approval
      - Create MR referencing SHA-2 with `require_approved_spec`: assert rejected

   f. **Unauthorized approver:**
      - Attempt spec approval with a non-authorized user/agent
      - Assert: ABAC denies the approval (per `builtin:require-human-spec-approval` policy)

2. **Add unit tests for each forge enforcement policy** (task-139):
   - `require_spec_ref`: MR without spec_ref → rejected
   - `require_approved_spec`: MR with unapproved spec → rejected
   - `warn_stale_spec`: stale spec → warning logged, merge proceeds
   - `require_current_spec`: stale spec → rejected
   - `require_signed_approval`: approval without signature → rejected

3. **Tests should be self-contained** — each test sets up its own repo, policies, and MR state.

## Acceptance Criteria

- [ ] Integration test for each of the 7 attack/drift scenarios in the spec table
- [ ] Unit tests for each of the 5 forge enforcement policies
- [ ] All tests are self-contained (create own state, no shared fixtures)
- [ ] Error messages are clear and actionable (tell the user WHY the MR was rejected)
- [ ] Tests pass

## Agent Instructions

- Read task-138 and task-139 for the spec approval ledger and forge enforcement policy implementations
- Read `crates/gyre-server/src/merge_processor.rs` for the merge processing flow
- Read `crates/gyre-server/src/api/specs.rs` for spec approval endpoints
- Read existing integration tests in `crates/gyre-server/tests/` for test setup patterns
- These tests validate the ENFORCEMENT behavior, not the schema — assume task-138 and task-139 are complete
- Each test should clearly document which attack/drift scenario it covers

## Shipped

### Behavior

`crates/gyre-server/tests/spec_enforcement.rs` (commit `f4b7f151`) — one
integration test per attack/drift row of agent-gates.md §Part 2 "What This
Prevents", each self-contained (own server on an ephemeral loopback port,
own state, own real on-disk git repos, own policies — no shared fixtures):

- **Row 1 (code without a spec)** — `code_without_spec_is_rejected`:
  `require_spec_ref` via the REST policy surface; MR with no `spec_ref`
  fails its queue entry with a reason naming the missing field, the MR
  stays open, the feature commit provably never lands on `main`; control
  MR with a `spec_ref` merges.
- **Row 2 (nonexistent spec)** — `nonexistent_spec_reference_is_rejected`:
  well-formed 40-hex SHA that resolves to nothing: merge rejected with
  "no active approval" naming the spec path, and the MR spec endpoint
  (the gate agent's spec channel) 404s.
- **Row 3 (unapproved spec)** — `unapproved_spec_is_rejected_until_approved`:
  real committed spec file with its real blob SHA; a Pending ledger row
  authorizes nothing; approving that exact (path, sha) version releases
  the block and a fresh MR merges.
- **Row 4 (spec changed after code written)** —
  `stale_spec_warns_or_blocks_by_policy`: same repo moves the spec from
  v1 to v2: under `warn_stale_spec` the merge proceeds and the advisory
  lands as an MR comment from `forge:spec-policy`; under
  `require_current_spec` the stale reference is blocked.
- **Row 5 (agent implements wrong spec version)** —
  `gate_agent_reviews_pinned_sha_and_catches_wrong_version`: a real
  `agent_review` gate whose reviewer process (bash + curl, scoped
  `review:submit` token) compares the MR diff against the spec content
  at the PINNED SHA and submits its verdict through the review API:
  wrong version → `changes_requested` → gate Failed → merge blocked;
  correct pairing → `approved` → merged.
- **Row 6 (retroactive spec modification)** —
  `retroactive_spec_modification_requires_new_approval`: approval on
  SHA-1 stays active after the spec moves to SHA-2; SHA-2 has no
  approval and an MR referencing it under `require_approved_spec` is
  rejected.
- **Row 7 (unauthorized approver)** — `unauthorized_approver_is_denied_by_abac`:
  an agent JWT attempting spec approval is denied 403 by the immutable
  `builtin:require-human-spec-approval` ABAC policy (denial recorded in
  the audit trail against that policy id, no ledger row created); a
  human Admin via API key succeeds and the ledger records the approver
  identity.

Merge-queue cycles run through `merge_processor::run_once` (the same
`process_next` the spawned processor loop runs) for determinism; merges
in control cases are real git merges through the git2 adapter.

Unit tests in `merge_processor.rs` (commit `f4b7f151`) cover all five
forge enforcement policies at the merge level:
`require_spec_ref_blocks_mr_without_spec_ref`,
`require_approved_spec_blocks_unapproved_spec` (new), mirroring the
existing `warn_stale_spec_adds_advisory_comment_without_blocking`,
`require_current_spec_blocks_stale_spec_ref`, and
`require_signed_approval_blocks_unsigned_spec_approval`: rejection with
a clear reason, author notification, and a positive control that
merges.

Follow-up commits on this branch keep the suite green against merged
upstream changes: `0de2f4e4` polls for the owner notification in the
persona push test support (fixes a notification race observed once in a
full-suite run), `0c052944` adapts to the repo-scoped notification-list
port signature, and `6128a39c` supplies the four hierarchy scope
arguments to `AgentSigningKey::mint` in the Row 7 rogue-JWT fixture
(task-130's token-scoping API, merged via `b6892403`). The Row 7 denial
under test is role-based — the immutable
`builtin:require-human-spec-approval` Deny fires on
`subject.type != "user"` — so the scoped claims neither widen nor
narrow the condition.

### Verification repair (this round)

Durable finding: the verification round (attempt
`9f427619fe5e4592819aa3020b0bb8f8`) exited 1 solely on
`check-task-commit-attribution.sh`: commit `6128a39c`'s subject
("supply task-130 scope args") references task-130, and every
task-labeled product-surface commit must be recorded in each referenced
task's `commits:` frontmatter. Repair: `6128a39c` is appended to
`specs/tasks/task-130.md`'s commits frontmatter — the commit adapts a
consumer of task-130's `AgentSigningKey::mint` signature, so it belongs
in that task's review scope — and re-listed in this task's frontmatter
after the round reset. No exemption entries were added.

Earlier repair lineage (already in-branch): the E0061 mint-arity compile
failure from attempt `9428944faefc42fe8635f3e009b4d12f` was fixed in
`6128a39c`.

### Test evidence

- `cargo check -p gyre-server --test spec_enforcement` on this round's
  head: clean (evidence:
  `/tmp/stage/review-evidence/cargo-check-spec-enforcement.txt`). This
  is the exact probe that failed with E0061 in the earlier round.
- `bash scripts/check-task-commit-attribution.sh` on this round's head:
  passes with the task-130 frontmatter repair (evidence:
  `/tmp/stage/review-evidence/check-task-commit-attribution.txt`).
- Full `cargo test --all` on the earlier candidate `9b9954f1` (same test
  tree): 1974/1975 passed, the single failure being the notification
  race fixed in `0de2f4e4` — i.e. the spec-enforcement suite itself
  passed in that run.
- Server test execution is deferred to the independent verifier per the
  build-budget constraint. The verifier must execute:
  - `cargo test -p gyre-server --test spec_enforcement`
  - `cargo test -p gyre-server --lib merge_processor`
  - then the full `cargo test --all`
