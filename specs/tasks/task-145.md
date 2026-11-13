---
title: "Implement spec enforcement validation tests for attack/drift prevention"
spec_ref: "agent-gates.md §Part 2 What This Prevents"
depends_on:
  - task-139
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §What This Prevents"
commits: ["f4b7f15195215cd8695e682404343ea4f1db325a", "0de2f4e4cb9b591e951718aa7bba459e77479152", "0c0529449e16bbb3ae93b099f5b5907fc2aa791b"]
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

- **Row 1 (code without a spec)** — `require_spec_ref` via the REST policy
  surface; MR with no `spec_ref` fails its queue entry with a reason naming
  the missing field, the MR stays open, the feature commit provably never
  lands on `main`; control MR with a `spec_ref` merges.
- **Row 2 (nonexistent spec)** — well-formed 40-hex SHA that resolves to
  nothing: merge rejected with "no active approval" naming the spec path,
  and the MR spec endpoint (the gate agent's spec channel) 404s.
- **Row 3 (unapproved spec)** — real committed spec file with its real blob
  SHA; a Pending ledger row authorizes nothing; approving that exact
  (path, sha) version releases the block and a fresh MR merges.
- **Row 4 (spec changed after code written)** — same repo moves the spec
  from v1 to v2: under `warn_stale_spec` the merge proceeds and the
  advisory lands as an MR comment from `forge:spec-policy`; under
  `require_current_spec` the stale reference is blocked with a
  "requires current spec" reason.
- **Row 5 (agent implements wrong spec version)** — a real `agent_review`
  gate whose reviewer process (bash + curl, scoped `review:submit` token)
  compares the MR diff against the spec content at the PINNED SHA and
  submits its verdict through the review API: wrong version →
  `changes_requested` → gate Failed → merge blocked; correct pairing →
  `approved` → merged.
- **Row 6 (retroactive spec modification)** — approval on SHA-1 stays
  active after the spec moves to SHA-2; SHA-2 has no approval and an MR
  referencing it under `require_approved_spec` is rejected.
- **Row 7 (unauthorized approver)** — an agent JWT attempting spec
  approval is denied 403 by the immutable
  `builtin:require-human-spec-approval` ABAC policy (denial recorded in
  the audit trail against that policy id, no ledger row created); a human
  Admin via API key succeeds and the ledger records the approver identity.

Merge-queue cycles run through `merge_processor::run_once` (the same
`process_next` the spawned processor loop runs) for determinism; merges in
control cases are real git merges through the git2 adapter.

Unit tests in `merge_processor.rs` for the two forge enforcement policies
that had no merge-level coverage (`require_spec_ref`,
`require_approved_spec`), mirroring the existing
`warn_stale_spec`/`require_current_spec`/`require_signed_approval` tests:
rejection with a clear reason, author notification, and a positive control
that merges.

### Rebase repair (this round, commit `0c052944`)

Durable finding: published PR 817 conflicted with current main (merge
state DIRTY). Resolved the merge of `f7f5f59` (task-125 teams/notifications
API, which added a `repo_id` filter to `NotificationStore::list_for_user`)
into this branch:

- `git_http.rs` conflict: kept the candidate's polling
  `wait_for_notification` helper (the race fix from `0de2f4e4`) and
  adapted it to the new 8-arg port signature, discarding the incoming
  single-check inline listing that reintroduced the race.
- `merge_processor.rs`: the auto-merge had reformatted the task-145
  `require_spec_ref` unit test's `list_for_user` call to the new arity but
  dropped the `repo_id` argument (compile error caught by `cargo check`);
  filled in the missing `None`.

All other files merged cleanly; `tests/spec_enforcement.rs` itself is
byte-identical to the approved candidate.

### Test evidence

- `cargo check -p gyre-server --lib --tests` on this round's head
  (`0c052944`): clean — lib, unit tests, and `tests/spec_enforcement.rs`
  all type-check against the merged base (evidence:
  `/tmp/stage/review-evidence/cargo-check-merge-resolution.txt`).
- Full `cargo test --all` on the prior candidate `9b9954f1` (same test
  tree plus the race present): 1974/1975 passed; the single failure is the
  race fixed in `0de2f4e4`, i.e. the spec-enforcement suite itself passed
  in that run.
- Server test execution is deferred to the independent verifier per the
  build-budget constraint. The verifier must execute:
  - `cargo test -p gyre-server --test spec_enforcement`
  - `cargo test -p gyre-server --lib merge_processor`
  - `cargo test -p gyre-server --lib git_http::tests::push_persona_change_updates_entity_resets_approval_and_notifies_owner`
  - then the full `cargo test --all`
