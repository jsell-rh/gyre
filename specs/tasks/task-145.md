---
title: "Implement spec enforcement validation tests for attack/drift prevention"
spec_ref: "agent-gates.md §Part 2 What This Prevents"
depends_on:
  - task-139
progress: complete
coverage_sections:
  - "agent-gates.md §What This Prevents"
commits: ["de74a59fa8656c4cb224ac96996d8479925bf733", "0d3c6a58147ae47ff616371d8db02b49c15883f2", "6128a39c8fa30617634c5ac192529ee469506ffb", "0c0529449e16bbb3ae93b099f5b5907fc2aa791b", "0de2f4e4cb9b591e951718aa7bba459e77479152", "f4b7f15195215cd8695e682404343ea4f1db325a"]
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
own state, own real on-disk git repo, own policies; no shared fixtures):

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
  `require_current_spec` the stale reference is blocked with a reason
  naming the policy.
- **Row 5 (agent implements wrong spec version)** —
  `gate_agent_reviews_pinned_sha_and_catches_wrong_version`: a real
  reviewer script (registered gate agent, scoped token) reads the spec at
  the PINNED SHA via `GYRE_SPEC_FILE` (never HEAD), diffs it against the
  MR implementation, and submits its verdict through the review API;
  matching and mismatching versions produce approved / changes_requested.
- **Row 6 (retroactive spec modification)** —
  `retroactive_spec_modification_requires_new_approval`: approving SHA-1
  leaves SHA-2 unapproved in the ledger; an MR referencing SHA-2 under
  `require_approved_spec` is rejected with the unapproved SHA named.
- **Row 7 (spec approved by unauthorized person)** —
  `unauthorized_approver_is_denied_by_abac`: a registered agent JWT
  (minted with self-consistent scope claims) is refused by the immutable
  `builtin:require-human-spec-approval` deny, and the denial is recorded
  in the audit trail against that policy id.

Forge enforcement policy unit tests (merge_processor.rs): one per policy —
`require_spec_ref_blocks_mr_without_spec_ref`,
`require_approved_spec_blocks_unapproved_spec`,
`warn_stale_spec_adds_advisory_comment_without_blocking`,
`require_current_spec_blocks_stale_spec_ref`,
`require_signed_approval_blocks_unsigned_spec_approval`.

### CI repair (commit `de74a59f`)

Candidate head `39388628` failed CI run 43585665655 on
`notifications::tests::notify_rich_falls_back_to_workspace_home_link`
("unresolvable workspace must not fabricate a link"). Root cause is a test
flake, not a production defect: `notify_rich` stamps `created_at` at
whole-second granularity and `list_for_user` sorts `created_at` descending,
so when the test's two `notify_rich` calls straddle a wall-clock second
boundary the newer unknown-workspace notification sorts first and index
`[1]` is the known-workspace notification, whose legitimate deep link then
reads as a fabricated one. Identical code passed the main-branch CI run
43586273005 seven minutes later, confirming a one-off timing flake. The
test now selects the unknown-workspace notification by source entity id
instead of list index; `deep_link` already failed closed and needed no
change.

### Test evidence

- `cargo check -p gyre-server --tests` — clean (this sandbox; server test
  execution deferred to the verifier per build-wrapper policy).
- Verifier must run:
  `cargo nextest run -p gyre-server --lib notifications::tests::notify_rich_falls_back_to_workspace_home_link`
  and
  `cargo nextest run -p gyre-server --test spec_enforcement`
  plus the five merge_processor policy unit tests
  (`cargo nextest run -p gyre-server --lib spec_ref spec_policy stale signed_approval`
  filter set). All 3764-test suite evidence from CI run 43585665655:
  every test except the flake passed, including the full spec-enforcement
  suite at candidate head `39388628`.
