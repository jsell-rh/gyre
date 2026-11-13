---
title: "Wire meta-spec drift warnings into merge gates"
spec_ref: "meta-spec-reconciliation.md §9"
depends_on: [task-156]
progress: ready-for-review
coverage_sections:
  - "meta-spec-reconciliation.md §9 Merge Gate Behavior"
commits: ["733f1bd6aa2cd179c662c5a057764acb1f972065", "04df217adba0528df3e0a54538c793bb9b0ec0e6", "e0a97a8a6f011f8e60817782d75431763c7419e6", "51528a6c51b11f2714bf026c34bc9886d4a93d1d", "2d15f77c41a8073cb3fa0047347eba92910802a8", "631e821555325960b44efb361a527ef8f52fe59b", "bd83468915b374048847de7dc7d4551aaa95f857", "11a702b89c608d1b843aa098922a729c5a97b2b1", "8a99bd8087b9ae152f08153f2f4e71afddc9b2fc", "abbcd4aadae36785f7dfe7efa4e4dec03ca73d95", "333030908ce0ff64b2810ea0003fc0e28598abf3", "ef9680cd63c187c1b0912590e12dec3fa8b1a6fd"]
---

## Spec Excerpt

From `meta-spec-reconciliation.md` §9 — Merge Gate Behavior:

> When an MR enters the merge queue, the merge processor checks whether the code was produced under the current meta-spec set version:
>
> - If `meta_spec_set_sha` on the MR's attestation matches the workspace's current set → pass
> - If they differ → warn: `[WARN] meta-spec-drift: code produced under meta-spec set {old_sha}, current is {new_sha}. Reconciliation may be needed.`
> - Warn only, do not block. The code may be valid even under the new meta-spec set (many meta-spec changes are additive or don't affect existing code).
>
> If a workspace admin wants to block merges during reconciliation, they set the workspace merge policy to `require_current_meta_spec_set: true`. This converts the warning to a hard block.

## Implementation Plan

1. **Add meta-spec drift check to merge processor** (`crates/gyre-server/src/merge_processor.rs`):
   - Before processing an MR, retrieve the MR's attestation (if present) and check `meta_spec_set_sha`
   - Compare against the workspace's current meta-spec set SHA
   - If they differ, add a `[WARN] meta-spec-drift` entry to the MR's gate results
   - The warning is informational by default (does not block merge)

2. **Add workspace merge policy option**:
   - Add `require_current_meta_spec_set: bool` to workspace settings or merge policy
   - When true, meta-spec drift converts from warning to hard block (MR stays in queue until reconciled or overridden)
   - Default: false (warn only)

3. **Display drift warning in UI**:
   - MergeQueueGraph component should show drift warnings alongside other gate results
   - Warning badge: "Meta-spec drift: produced under v{old}, current is v{new}"

4. **Add API for workspace merge policy** (if not already covered by workspace settings):
   - This may be part of existing workspace settings — check before creating new endpoints

5. **Tests**:
   - Unit test: matching meta_spec_set_sha → no warning
   - Unit test: mismatched SHA → warning added to gate results
   - Unit test: `require_current_meta_spec_set: true` → hard block on mismatch
   - Unit test: MR without attestation → skip check (no crash)

## Acceptance Criteria

- [ ] Merge processor checks meta_spec_set_sha on MRs with attestations
- [ ] Mismatched SHA produces `[WARN] meta-spec-drift` in gate results
- [ ] Warning does not block merge by default
- [ ] `require_current_meta_spec_set: true` workspace setting converts warning to hard block
- [ ] MRs without attestation skip the check gracefully
- [ ] Tests cover match, mismatch, hard-block, and no-attestation cases

## Agent Instructions

- Read `crates/gyre-server/src/merge_processor.rs` for the merge queue processing loop — find where gate checks are evaluated
- Read `crates/gyre-server/src/gate_executor.rs` for how gate results are structured and reported
- Read `crates/gyre-domain/src/attestation.rs` for the MergeAttestation struct (meta_spec_set_sha field)
- Read `crates/gyre-server/src/constraint_check.rs` for existing constraint evaluation (meta_spec_set_sha constraint exists but may not be wired into gates)
- The meta_spec_set_sha is already captured in attestations — the check is comparing it against the workspace's current set SHA
- Follow the existing gate result pattern for adding the drift warning

## Shipped

### §9 drift evaluation and merge-gate wiring

`crates/gyre-server/src/reconciliation.rs`: `mr_meta_spec_drift(state,
mr)` resolves the MR's provenance attestation (source-branch head commit
first, then the author agent's current task, walking derived nodes to the
chain's root `SignedInput`) and compares the recorded `meta_spec_set_sha`
against the workspace's current set SHA (`compute_meta_spec_set_sha` — the
effective set: tenant defaults merged with workspace overrides). No drift
is reported when the workspace has no bound set, the MR has no
attestation, or the attestation records an empty SHA — all skip the check
gracefully. `format_meta_spec_drift_warning` produces the exact §9 text
`[WARN] meta-spec-drift: code produced under meta-spec set {old}, current
is {new}. Reconciliation may be needed.`;
`parse_meta_spec_drift_warning` round-trips it for the queue graph.

`crates/gyre-server/src/merge_processor.rs`: drift + merge-policy
evaluation runs during candidate selection in `process_next`
(`selection_meta_spec_drift_hold`, expanded to whole atomic groups).
Default (warn only): a gate result with id `meta-spec-drift`,
`GateStatus::Passed` and the §9 warning output is recorded on the MR
(idempotent across retry cycles) and the merge proceeds. Strict policy
(`require_current_meta_spec_set: true`): the entry is requeued carrying
the drift reason and the processor continues to the next candidate in
the same cycle, so a strictly-blocked drifted MR at the head of the
shared queue cannot starve later entries — in its own workspace or in
others.

### Workspace merge policy + API

`crates/gyre-server/src/merge_policy.rs` +
`crates/gyre-server/src/api/workspaces.rs`: `WorkspaceMergePolicy {
require_current_meta_spec_set }` persisted through the port-backed KV
store, default false. Exposed via `GET/PUT
/api/v1/workspaces/:id/merge-policy` (ABAC-registered in
`abac_middleware.rs`, Admin-only PUT). A corrupt stored policy surfaces
as an error on direct lookup and degrades to the spec default (warn only)
inside the merge processor.

### UI

`crates/gyre-server/src/api/merge_deps.rs` parses the recorded drift gate
result into a `meta_spec_drift` node field;
`web/src/components/MergeQueueGraph.svelte` renders the ⚠ badge with
tooltip "Meta-spec drift: produced under v{old}, current is v{new}".

### Rebase repair round — merge of current main b9487b1b (task-182)

The published PR (head 3e724333) conflicted with current main (merge
state DIRTY). This round merges main b9487b1b (task-182 Concept Views:
view-query grammar/resolver, graph concepts endpoints with their ABAC
mappings, Explorer UI and dist) into the branch as merge commit
f8076591. No source file was touched by both sides — all six task-159
files (`reconciliation.rs`, `merge_processor.rs`, `merge_policy.rs`,
`api/workspaces.rs`, `api/merge_deps.rs`, `MergeQueueGraph.svelte`) are
byte-identical to the approved head 3e724333, and the `merge-policy`
ABAC mapping coexists with main's new `graph/concepts` mappings.
Conflicts were confined to `web/dist`, resolved to the assignment-base
build (identical to main b9487b1b); regenerated assets are
restored/republished by the executor and final integration. The
`commits:` frontmatter re-records the attributed product commits (the
restored contract had blanked the list, failing
`scripts/check-task-commit-attribution.sh` scoping).

### Test evidence

In-sandbox (4 GiB budget), this round on the merged tree f8076591
(evidence: /tmp/stage/review-evidence/task-159-rebase-repair-2.txt):
`cargo check -p gyre-server` passes (8m 12s, via the build wrapper);
`cargo check -p gyre-server --tests` passes (3m 37s — the merge brings
main's task-182 server changes, so the merged tree including test code
is type-checked); `npx vitest run
src/__tests__/MergeQueueGraph.test.js` — 29/29 pass (drift badge +
tooltip); `scripts/check-abac-route-registry.sh` — pass;
`scripts/check-task-commit-attribution.sh` — pass after the frontmatter
re-record. `cargo test -p gyre-server` is owned by the independent
verifier (server test compilation exceeds the sandbox budget with the
model resident) — the verifier must run the recorded server probes
(all test names confirmed present in the merged tree):

    cargo test -p gyre-server --lib reconciliation::tests::drift_
    cargo test -p gyre-server --lib reconciliation::tests::drift_warning_parse
    cargo test -p gyre-server --lib merge_processor::tests::meta_spec_drift
    cargo test -p gyre-server --lib merge_processor::tests::strict_drift_hold
    cargo test -p gyre-server --lib merge_policy::tests
    cargo test -p gyre-server --lib api::workspaces::tests::merge_policy_default_admin_only_update_and_roundtrip

These cover the acceptance criteria (match → no warning, mismatch → §9
warning, warn-only proceeds, strict policy hard-blocks, no-attestation
skip, gate-result idempotency, queue-starvation regression). Main's
task-182 tests (`ExplorerConceptViews.test.js`, view-query suites) are
preserved in the merged tree for the verifier's full run.
