---
title: "Wire meta-spec drift warnings into merge gates"
spec_ref: "meta-spec-reconciliation.md §9"
depends_on: [task-156]
progress: ready-for-review
coverage_sections:
  - "meta-spec-reconciliation.md §9 Merge Gate Behavior"
commits: ["11a702b89c608d1b843aa098922a729c5a97b2b1", "8a99bd8087b9ae152f08153f2f4e71afddc9b2fc", "abbcd4aadae36785f7dfe7efa4e4dec03ca73d95", "333030908ce0ff64b2810ea0003fc0e28598abf3"]
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

**Drift evaluation (§9)** — `crates/gyre-server/src/reconciliation.rs`:
`mr_meta_spec_drift(state, mr)` resolves the MR's provenance attestation (by
source-branch head commit first, then the author agent's current task,
walking derived nodes to the chain's root `SignedInput`) and compares the
recorded `meta_spec_set_sha` against the workspace's current set SHA
(`compute_meta_spec_set_sha`). Returns no drift when the workspace has no
bound set, the MR has no attestation, or the attestation records an empty
SHA — all skip the check gracefully. `format_meta_spec_drift_warning`
produces the exact §9 text `[WARN] meta-spec-drift: code produced under
meta-spec set {old}, current is {new}. Reconciliation may be needed.` and
`parse_meta_spec_drift_warning` round-trips it for the queue graph.

**Merge-gate wiring (§9)** — `crates/gyre-server/src/merge_processor.rs`:
the drift + merge-policy evaluation runs DURING candidate selection in
`process_next` (step 4c'', `selection_meta_spec_drift_hold`), exactly like
the task-077 trust hold. Default (warn only): a gate result with id
`meta-spec-drift`, `GateStatus::Passed` and the §9 warning output is
recorded on the MR (idempotent across retry cycles — an existing result is
updated in place, not duplicated) and the merge proceeds. Strict policy
(`require_current_meta_spec_set: true`): the entry is requeued carrying the
drift reason and the processor continues to the next candidate in the SAME
cycle, so a strictly-blocked drifted MR at the head of the shared queue
cannot starve later entries — in its own workspace or in other workspaces.
The entry stays Queued until the code is reconciled under the current set
or the policy is relaxed. `selection_meta_spec_drift_hold` expands
atomic-group candidates to all group members, so a drifted member holds
the whole group (each member entry is requeued with the drift reason)
without blocking later entries in that cycle; `merge_atomic_group` and
the single-entry path no longer re-check drift after selection.

**Workspace merge policy** — `crates/gyre-server/src/merge_policy.rs`:
`WorkspaceMergePolicy { require_current_meta_spec_set }` persisted through
the port-backed KV store (namespace `workspace_merge_policies`, key =
workspace id), default false. A corrupt stored policy surfaces as an error
on direct lookup and degrades to the spec default (warn only) inside the
merge processor so a policy-store outage cannot wedge the queue.
Exposed as `GET/PUT /api/v1/workspaces/{id}/merge-policy` (partial update;
PUT Admin-only, both handlers tenant-scoped; `crates/gyre-server/src/api/workspaces.rs`),
registered in the ABAC resolver (`abac_middleware.rs`) and documented in
`docs/api-reference.md`.

**UI** — `web/src/components/MergeQueueGraph.svelte`: nodes carry a ⚠
meta-spec drift badge (`data-drift`, `mq-drift-{mr_id}`) when the backend
recorded a drift warning, the hover tooltip shows "Meta-spec drift:
produced under v{old}, current is v{new}" (8-char SHA prefixes), and the
legend lists "Meta-spec drift (warn only)". The graph API
(`GET /api/v1/merge-queue/graph`, `crates/gyre-server/src/api/merge_deps.rs`)
surfaces the recorded drift per node by parsing the same gate result the
MR detail view reads, so the graph cannot disagree with gate results.

### Test evidence

- `cargo test -p gyre-server --lib meta_spec_drift` — 5 passed:
  warn-records-gate-result-and-merges, hard-block + policy-relax recovery,
  match-no-warning, no-attestation-skip, gate-result idempotency.
- `cargo test -p gyre-server --lib strict_drift_hold` — 2 passed (review
  repair regression): clean MRs behind a strictly-blocked drifted head
  merge in the same cycle (same-workspace and cross-workspace), and a
  drift-held atomic group does not starve a later independent entry.
  Kill-tested: with the old post-selection early-return shape restored,
  both tests fail on "clean MR must merge in the same cycle".
- `cargo test -p gyre-server --lib merge_policy` — 4 passed (KV default /
  roundtrip / corrupt-fallback + API test: default warn-only, Developer
  PUT 403 with no state change, Admin PUT roundtrips, unknown workspace
  404).
- `cargo test -p gyre-server --lib reconciliation::` — 24 passed,
  `merge_deps::` — 13 passed, `api::workspaces::` — 17 passed,
  `merge_processor` — 74 passed (all selection/atomic-group/trust suites
  included, since the selection loop changed).
- `cargo test -p gyre-server --lib` (full lib) — 1501 passed, 0 failed
  (merge-correctness probe after the base merge repair).
- `npx vitest run src/__tests__/MergeQueueGraph.test.js` — 29 passed
  (20 pre-existing + 5 drift badge/tooltip/legend tests + 4 added by the
  base); this repair touched no web/ sources.
- Mechanical checks green: check-inert-enforcement, check-arch,
  check-abac-route-registry, check-abac-exempt-handlers,
  check-scope-literal-defaults, check-in-memory-state-stores,
  check-forged-scope-fields, check-migration-versions,
  check-lossy-secret-conversion, check-fail-open-ref-resolution,
  check-forwarded-header-trust, check-unbounded-external-http,
  check-fabricated-scope-defaults, check-relative-path-defaults,
  check-mem-port-contracts, check-mcp-write-tools,
  check-dead-message-kinds, check-migration-sql-portability,
  check-task-commit-attribution.

### Round note (base-merge repairs)

Original backend, policy API, drift evaluation, UI, and tests shipped in
recovered checkpoint 333030908 + feat(task-159) commit abbcd4aad, followed
by the starvation review repair (commit 8a99bd808), the fefde2946 base
merge and its attestation-ABAC test repair (commit 11a702b89), and the
b5a8720878 base merge (task-108/106/116/140/205/208; merge commit
34b4b8ebc) whose single content conflict — both sides appending test
suites at the tail of `mod tests` in `merge_processor.rs` — was resolved
by keeping both blocks intact.

This round continued from checkpoint 6df4d7880 (the prior iteration
timed out mid-verification during the cold compile of the focused
workspaces suite; no code change was pending) and completed the merge of
the assigned base fca5de5fb (PR #763 admission-policy preflight fix;
merge commit e29668348). The base touches only pipeline scripts and
docs (`scripts/pipeline/compute.py`, `scripts/dev-pipeline.py`,
`scripts/test_pipeline_admission.py`, `docs/dev-pipeline.md`) — no
product surface, no content conflicts with task-159's drift hold, and
no new commits needed on the task's `commits:` list (the attribution
check excludes merge commits and non-product commits). No production
code changed this round.

Evidence this round: `cargo test -p gyre-server --lib api::workspaces::`
— 20 passed; focused drift/strict-drift/policy suites — 11 passed;
`merge_processor` — 90 passed; `reconciliation::` + `merge_deps::` —
52 passed; full `cargo test -p gyre-server --lib` — 1686 passed,
0 failed (merge-correctness probe). check-arch,
check-task-commit-attribution, check-abac-route-registry,
check-migration-versions green. Logs retained under
/tmp/stage/review-evidence/task-159-*.log. The
acceptance-criteria checkboxes are left unchecked in the task body per
the assigned contract; the criteria themselves are satisfied by the
shipped behavior above.
