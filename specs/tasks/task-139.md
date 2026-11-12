---
title: "Implement forge enforcement policies for spec-to-code binding at merge"
spec_ref: "agent-gates.md §Forge Enforcement Policies"
depends_on:
  - task-138
progress: complete
coverage_sections:
  - "agent-gates.md §Forge Enforcement Policies"
  - "agent-gates.md §Integration with Agent Gates"
commits: ["87e2aed4a851148071f0fa3c7b625dcf561730c2", "c92e8f534ef8e9c24ac7228d8d453d472f7e90e7", "33cc4941521a7e94d9742f2f7bb626b18adc4e45", "ec05bf9afe4de6256b57ba977c6e5f63ea7e1fa4", "059586a37a49b184d7a32a47d8c11c59da5702a3", "99edd0bcaddd740a5b560e61ef2937b156d780fb"]
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

The published PR head (`7d74ce9`) conflicted with current main
(`d31906c`, task-181 Risk Map). The DIRTY merge state is resolved: main
is merged into this branch as `ae76f2763` with all conflicts confined
to generated `web/dist` assets (resolved in favor of the staged merge
resolution; main's `web/src` Risk Map sources and `task-181.md` merge
cleanly and are taken wholesale). No `crates/` source overlapped, so
the retained task-139 implementation is byte-identical to the reviewed
PR head; `cargo check -p gyre-server -p gyre-adapters -p gyre-domain`
passes clean on the merged tree.

### Five per-repo enforcement policies

- `SpecPolicy` (`gyre-domain/src/spec_policy.rs`) carries all five flags
  (`require_spec_ref`, `require_approved_spec`, `warn_stale_spec`,
  `require_current_spec`, `require_signed_approval`), persisted via
  portable migration `2026-11-09-000059_require_signed_approval` across
  SQLite/Postgres/mem stores. Defaults false (opt-in); `GET/PUT
  /api/v1/repos/:id/spec-policy` round-trips all five (Admin-only PUT).
- The merge processor (`merge_processor.rs`) enforces every policy before
  executing a merge: missing `spec_ref` rejected under
  `require_spec_ref`/`require_approved_spec`/`require_signed_approval`;
  unapproved SHA rejected via the task-138 spec-approval ledger
  (`gates::verify_spec_ref`); unsigned active approval rejected via
  `gates::verify_signed_spec_ref` (exact path@sha ledger lookup, non-empty
  signature required); `require_current_spec` rejects when the spec's
  current blob SHA (`git_refs::resolve_spec_blob_sha`) differs from the
  referenced SHA.
- Blocking violations fail the queue entry with the specific reason, emit
  a `GateFailure` workspace event, and notify the author's spawning user
  (`notifications::notify_spec_policy_violation`, tenant resolved from the
  workspace record — skipped and logged when unresolvable).
- `warn_stale_spec` warns without blocking: `StaleSpecWarning` event plus
  a deduplicated advisory MR comment (`forge:spec-policy` author) served by
  `GET /api/v1/merge-requests/:id/comments`.

### Spec-alignment gate at pinned SHA

- `MergeRequest.spec_ref` (`path@sha`, migration
  `2026-10-10-000057_task_spec_ref`) pins the reviewed spec version.
- `GET /api/v1/merge-requests/:id/spec` serves the spec blob at the pinned
  SHA via `git cat-file` (SHA validated 40-hex first); `gate_executor`
  passes `GYRE_SPEC_URL` to AgentReview/AgentValidation gate agents, and
  the scoped gate-agent tokens can fetch that route
  (`MR_SPEC_ROUTE` in reviewer/validator allowed routes).

### Test evidence

- `cargo test -p gyre-domain` on the merged tree: 466 passed, 0 failed
  (evidence retained under `/tmp/stage/review-evidence`).
- `cargo check -p gyre-server -p gyre-adapters -p gyre-domain` on the
  merged tree: clean.
- Server-side tests for the verifier to execute (large test compilation
  owned by verification per the 4 GiB budget):
  `cargo test -p gyre-server --lib merge_processor::` (in particular
  `require_current_spec_blocks_stale_spec_ref`,
  `require_signed_approval_blocks_unsigned_spec_approval`,
  `warn_stale_spec_adds_advisory_comment_without_blocking`,
  `policy_store_read_failure_holds_*_fail_closed`),
  `cargo test -p gyre-server --lib api::gates::tests::` (signed spec-ref
  verification cases), `cargo test -p gyre-server --lib
  api::spec_policy::tests::`, and `cargo test -p gyre-server --test
  task134_review_probe` (pinned-spec delivery to gate agents).
- `scripts/check-task-commit-attribution.sh` passes on the merged tree
  with the restored `commits:` frontmatter.
