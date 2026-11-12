---
title: "Implement forge enforcement policies for spec-to-code binding at merge"
spec_ref: "agent-gates.md §Forge Enforcement Policies"
depends_on:
  - task-138
progress: not-started
coverage_sections:
  - "agent-gates.md §Forge Enforcement Policies"
  - "agent-gates.md §Integration with Agent Gates"
commits: []
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
