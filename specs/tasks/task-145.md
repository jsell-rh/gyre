---
title: "Implement spec enforcement validation tests for attack/drift prevention"
spec_ref: "agent-gates.md §Part 2 What This Prevents"
depends_on:
  - task-139
progress: not-started
coverage_sections:
  - "agent-gates.md §What This Prevents"
commits: []
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
