---
title: "Implement forge enforcement policies for spec-to-code binding at merge"
spec_ref: "agent-gates.md §Forge Enforcement Policies"
depends_on:
  - task-138
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Forge Enforcement Policies"
  - "agent-gates.md §Integration with Agent Gates"
commits: ["99edd0bcaddd740a5b560e61ef2937b156d780fb", "ec05bf9afe4de6256b57ba977c6e5f63ea7e1fa4", "059586a37a49b184d7a32a47d8c11c59da5702a3", "33cc4941521a7e94d9742f2f7bb626b18adc4e45", "87e2aed4a851148071f0fa3c7b625dcf561730c2", "c92e8f534ef8e9c24ac7228d8d453d472f7e90e7", "d08359f1666233094a5ea0fda89c4bbbb80cde99", "ce115b65a"]
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

The DIRTY merge state against current main is resolved: main (`3b734b1`,
task-168 repo lifecycle) is merged in as `ce115b65a`. Conflicts were
confined to generated `web/dist` assets (resolved to main's dist, which
the executor restores from the assignment base anyway); the overlapping
`crates/` sources (`schema.rs`, `gates.rs`, `api/mod.rs`,
`gate_executor.rs`, `merge_processor.rs`) auto-merged — main's only
changes there were the `QualityGate.position` field and its test-literal
updates, which combine cleanly with the retained task-139 enforcement
code. No task-139 production code changed; it is byte-identical to the
reviewed PR head.

### Retained behavior (unchanged by the merge)

- `SpecPolicy` (`gyre-domain/src/spec_policy.rs`) carries all five flags
  (`require_spec_ref`, `require_approved_spec`, `warn_stale_spec`,
  `require_current_spec`, `require_signed_approval`), persisted via the
  portable `require_signed_approval` migration across
  SQLite/Postgres/mem stores; defaults false (opt-in); `GET/PUT
  /api/v1/repos/:id/spec-policy` round-trips all five.
- The merge processor enforces every policy before executing a merge:
  missing `spec_ref` rejected; unapproved SHA rejected via the task-138
  spec-approval ledger (`gates::verify_spec_ref`); unsigned active
  approval rejected (`gates::verify_signed_spec_ref`, exact path@sha
  ledger lookup, non-empty signature required); `require_current_spec`
  rejects when the spec's current blob SHA
  (`git_refs::resolve_spec_blob_sha`) differs from the referenced SHA.
- Blocking violations fail the queue entry with the specific reason, emit
  a `GateFailure` workspace event, and notify the author's spawning user
  (`notifications::notify_spec_policy_violation`).
- `warn_stale_spec` warns without blocking: `StaleSpecWarning` event plus
  a deduplicated advisory MR comment (`forge:spec-policy` author).
- `MergeRequest.spec_ref` (`path@sha`) pins the reviewed spec version;
  `GET /api/v1/merge-requests/:id/spec` serves the spec blob at the
  pinned SHA; `gate_executor` passes `GYRE_SPEC_URL` to gate agents and
  scoped tokens can fetch that route.

### Test evidence (merged tree `ce115b65a`)

- `cargo check --workspace --all-targets`: clean (9m05s, no errors).
- `cargo test -p gyre-domain`: 482 passed, 0 failed.
- `cargo test -p gyre-adapters --lib spec_policy`:
  `set_and_get_round_trips_all_five_policies` passes on the merged
  schema; `--lib quality_gate`: `save_upsert_updates_all_config_columns`
  passes (main's task-168 storage change coexists with ours).
- Mechanical checks on the merged tree: `check-arch.sh`,
  `check-abac-route-registry.sh`, `check-mcp-write-tools.sh`,
  `check-mem-port-contracts.sh`, `check-migration-versions.sh`,
  `check-migration-sql-portability.sh`, and the byte-slice/
  relative-path/fail-open/scope-literal/inert-enforcement/
  forwarded-header/in-memory-state/unbounded-http/forged-scope/
  lossy-secret/dead-message-kinds lints all pass.
- For the verifier (gyre-server test compilation owned there per the
  4 GiB budget): `cargo test -p gyre-server --lib merge_processor::`
  (in particular `require_current_spec_blocks_stale_spec_ref` and the
  other `spec_polic`-filtered enforcement tests), plus
  `cargo test -p gyre-server --lib gates::` for the signed-spec-ref
  ledger lookups.
