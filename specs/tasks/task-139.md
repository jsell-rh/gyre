---
title: "Implement forge enforcement policies for spec-to-code binding at merge"
spec_ref: "agent-gates.md §Forge Enforcement Policies"
depends_on:
  - task-138
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Forge Enforcement Policies"
  - "agent-gates.md §Integration with Agent Gates"
commits: ["99edd0bcaddd740a5b560e61ef2937b156d780fb", "059586a37a49b184d7a32a47d8c11c59da5702a3"]
---

## Spec Excerpt

From `agent-gates.md` §Forge Enforcement Policies:

| Policy | Effect |
|---|---|
| `require_spec_ref` | MRs must include a spec_ref. MRs without one are rejected. |
| `require_approved_spec` | The referenced spec SHA must have an approval. Unapproved specs block merge. |
| `warn_stale_spec` | If spec HEAD has moved past referenced SHA, warn but allow. |
| `require_current_spec` | The referenced spec SHA must match spec HEAD. Forces re-review if spec changed. |
| `require_signed_approval` | Spec approvals must include Sigstore signature. |

Policies are configurable per repository.

From §Integration with Agent Gates:

The `spec-alignment-review` gate type (using Accountability agent persona) is designed for this:
1. Gate agent receives MR diff AND spec at pinned SHA
2. Agent compares: does code actually implement what spec describes?
3. Agent checks: are there spec requirements that code doesn't address?
4. Agent submits Approved or ChangesRequested with specific findings

## Implementation Plan

1. **Spec enforcement policy configuration:**
   - Add `spec_enforcement_policy` to repo settings (stored in DB or repo config)
   - Fields: require_spec_ref (bool), require_approved_spec (bool), warn_stale_spec (bool), require_current_spec (bool), require_signed_approval (bool)
   - Default: all false (opt-in per repo)

2. **Add `spec_ref` field to MR model:**
   - Extend MergeRequest entity with `spec_ref: Option<String>` (format: `"path@sha"`)
   - Migration to add column to merge_requests table
   - MR creation and update accept spec_ref

3. **Merge-time enforcement checks:**
   - In merge processor, before executing merge:
   - `require_spec_ref`: reject if MR has no spec_ref
   - `require_approved_spec`: look up spec_ref SHA in spec_approvals ledger, reject if no Approved entry
   - `warn_stale_spec`: compare spec_ref SHA to current spec HEAD SHA, emit warning if different
   - `require_current_spec`: reject if spec_ref SHA ≠ current spec HEAD SHA
   - `require_signed_approval`: reject if approval lacks signature field

4. **Enforcement result reporting:**
   - On policy violation, return clear error to merge processor
   - Error propagated to MR status / author notification
   - Warning-only policies (warn_stale_spec) add advisory comment but don't block

5. **Spec-alignment gate integration:**
   - When AgentReview gate with accountability persona is configured:
   - Gate agent receives spec content at the pinned SHA (not HEAD)
   - This ensures review is against the version the code was written for

6. **API: repo spec policy endpoints:**
   - Check if `GET/PUT /api/v1/repos/:id/spec-policy` already exists
   - Extend to include new enforcement policy fields
   - Or create new endpoint if not present

## Acceptance Criteria

- [ ] 5 enforcement policies configurable per repo
- [ ] `spec_ref` field on MergeRequest entity (path@sha format)
- [ ] `require_spec_ref`: MRs without spec_ref rejected at merge
- [ ] `require_approved_spec`: unapproved spec SHAs block merge
- [ ] `warn_stale_spec`: warning when spec HEAD ≠ referenced SHA
- [ ] `require_current_spec`: rejection when spec HEAD ≠ referenced SHA
- [ ] `require_signed_approval`: rejection when approval lacks signature
- [ ] Enforcement errors reported clearly to author
- [ ] Spec-alignment gate receives spec at pinned SHA, not HEAD
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-gates.md` §Forge Enforcement Policies and §Integration with Agent Gates. Existing spec policy: check `gyre-server/src/api/spec_policy.rs` and routes at `GET/PUT /api/v1/repos/:id/spec-policy` (registered in `gyre-server/src/api/mod.rs` at line ~171). MR model: `gyre-domain/src/merge_request.rs` or equivalent. Merge processor: grep for `merge_processor\|process_merge` in `gyre-server/src/`. Spec approvals: depends on task-138's ledger. Git SHA comparison: need to resolve current HEAD SHA for a spec file path in the repo. Check migration numbering: currently at 000049.

## Shipped

All five forge enforcement policies run at merge time with real ledger and
git lookups, violations reach the MR author directly, and the
spec-alignment gate receives the spec at the pinned SHA. Four policies
(`require_spec_ref`, `require_approved_spec`, `warn_stale_spec`,
`require_current_spec`) and the `spec_ref` MR field shipped with the
task-138 lineage; commit 99edd0bc adds the missing pieces:

### The five policies, configurable per repo

- `SpecPolicy.require_signed_approval` (`gyre-domain/src/spec_policy.rs`),
  persisted via portable migration
  `2026-11-09-000059_require_signed_approval` (SQLite + Postgres adapters,
  mem store). Defaults false — opt-in per repo; `GET/PUT
  /api/v1/repos/:id/spec-policy` round-trips all five fields (Admin-only
  PUT), and the RepoSettings web form binds the real API field names for
  all five.
- `verify_signed_spec_ref` (`api/gates.rs`): exact-(path, sha) ledger
  query where the active approval must additionally carry a non-empty
  signature; one signed active approval among several satisfies the
  policy, whitespace-only signatures do not.
- Merge processor (`merge_processor.rs`) enforces every policy before
  executing the merge: missing spec_ref rejected under
  `require_spec_ref`/`require_approved_spec`/`require_signed_approval`
  (no spec_ref → no referenced approval), unapproved SHA rejected,
  `require_current_spec` rejected when spec HEAD ≠ referenced SHA,
  `require_signed_approval` rejected when the active approval lacks a
  signature.

### Enforcement errors reported to the author

- Blocking rejections fail the queue entry with the specific reason, emit
  a `GateFailure` workspace event, and deliver an inbox notification to
  the author's spawning user
  (`notifications::notify_spec_policy_violation`, tenant resolved from
  the workspace record — skipped + logged when unresolvable, no fabricated
  "default" scope).
- `warn_stale_spec` warns without blocking: keeps the `StaleSpecWarning`
  event and records a deduplicated advisory MR comment
  (`forge:spec-policy` author) visible via
  `GET /api/v1/merge-requests/:id/comments`.

### Spec-alignment gate receives spec at pinned SHA

- `GET /api/v1/merge-requests/:id/spec` serves the spec content at the
  MR's pinned `spec_ref` SHA via `git cat-file blob` (SHA validated
  40-hex before reaching the command line); 404 when the MR has no
  spec_ref or the pinned blob is unreachable. Route registered with an
  ABAC `RouteResourceMapping`.
- `gate_executor` passes `GYRE_SPEC_URL` to AgentReview/AgentValidation
  gate agent processes (documented in `docs/agent-protocol.md`), derived
  from the review context's pinned `spec_ref` — review runs against the
  version the code was written for, not HEAD.

### Test evidence

Hard tests distinguishing real behavior (real git repos with HEAD moved
past pinned blobs, real ledger queries, real merge queue transitions):
`signed_spec_ref_requires_signature_on_active_approval`,
`require_signed_approval_blocks_unsigned_spec_approval` (asserts author
notification via `NotificationRepository::list_for_user`; control MR with
signed approval merges),
`warn_stale_spec_adds_advisory_comment_without_blocking`,
`get_mr_spec_serves_pinned_sha_not_head`,
`set_require_signed_approval_round_trips`,
`set_and_get_round_trips_all_five_policies` (fresh SQLite DB proves
migration 000059 runs), plus regression controls
(`require_current_spec_blocks_stale_spec_ref`,
`revoked_approval_no_longer_verifies_spec_ref`,
`find_by_spec_sha_scopes_to_exact_version`).

Prior-round suite evidence on equivalent source: `cargo test -p
gyre-server --lib` 1664/1664; `gyre-domain` 426+6; `gyre-adapters --lib`
448; integration git/api/graph 17/66/38; frontend RepoSettings vitest
58/58; all check-* mechanical invariants pass.

### This round: verification repair (commit 059586a3)

Verification attempt 24d519fb failed only the rustfmt changed-lines gate
(4 files). This round applies rustfmt to those changed regions — 41
lines, no semantic changes — and re-proves locally:
`scripts/check-rustfmt-diff.py cba416b` clean; `cargo check -p
gyre-server` and `cargo check -p gyre-server --tests` clean; all 20
check-* invariants pass; `check-task-commit-attribution` clean with both
commits recorded. Evidence: `review-evidence/rustfmt-repair.txt`.

Deferred to the independent verifier (sandbox wrapper refuses
gyre-server test compilation): `cargo test -p gyre-server --lib`
(merge_processor and api suites named above), integration suites, full
`cargo test --all`, all-target Clippy, and exact-head GitHub CI.
