---
title: "Implement reconciliation controller and conformance sweep background job"
spec_ref: "meta-spec-reconciliation.md §6, §10"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "meta-spec-reconciliation.md §6 Reconciliation: The Slow Rollout"
  - "meta-spec-reconciliation.md §10 Conformance Sweeps (Steady State)"
  - "meta-spec-reconciliation.md §11 Observability"
commits: ["66a072184b7fe4ea60946ee1c9a9f0758b6bd2b5", "d0c1d9a75ccdef99ada0da887239b0e04c6760c8", "6ed3c5c37225f0db021e25d7d00fe09926a727ba", "77dbd920e954d7dd52ba883e061bd762db8a064f", "8c8e5056b80cc28d713dd88e3993c975b280e109", "f078e3af143024d9fd271e774d0eb6c1cf5d0d61", "82833df6483d1965d978419cbe34bd731a1627ef"]
---

## Spec Excerpt

From `meta-spec-reconciliation.md` §6 — Reconciliation:

> When a meta-spec change is approved and rolled out, the reconciliation controller:
> 1. Identifies all repos in the affected workspace(s) that are bound to the changed meta-spec
> 2. For each repo, creates a reconciliation task: "Align code with updated meta-spec {name} v{N}"
> 3. Task is assigned to the repo orchestrator, which spawns an agent to assess compliance
> 4. Agent produces a diff showing what needs to change (if anything)

From §10 — Conformance Sweeps:

> A background job periodically (default: daily) sweeps all repos to verify their code conforms to the active meta-spec set. This catches drift that accumulates between explicit reconciliation runs.

## Implementation Plan

1. **Create reconciliation controller** (`crates/gyre-server/src/reconciliation.rs` — new file):
   - Function `run_reconciliation(state: &AppState, workspace_id: Id, meta_spec_path: &str, new_version: u32)`
   - Query all repos in the workspace whose meta-spec set references the changed meta-spec
   - For each affected repo, create a reconciliation task (title: "Align code with updated {kind} {name} v{version}")
   - Label: `meta-spec-reconciliation`, priority: Medium
   - Emit `ReconciliationCompleted` message (infrastructure already exists in lib.rs)

2. **Wire reconciliation trigger** into meta-spec update flow:
   - When a meta-spec is approved via `POST /api/v1/meta-specs/{id}/approve` or when a meta-spec set is updated via `PUT /api/v1/workspaces/{id}/meta-specs/set`, trigger `run_reconciliation()`
   - Only trigger when the meta-spec version changes (not on re-approval of same version)

3. **Add conformance sweep background job** (register in `crates/gyre-server/src/jobs.rs`):
   - Job name: `meta_spec_conformance_sweep`
   - Default interval: 24 hours (86400 seconds)
   - For each workspace, compare the active meta-spec set SHA against the meta_spec_set_sha in recent agent commits
   - If drift is detected (code was produced under an older meta-spec set), create a drift-review task
   - Emit `MetaSpecDrift` notifications (notification type already exists)

4. **Add observability**:
   - Emit structured log events for reconciliation start/complete
   - Use existing ReconciliationCompleted MessageKind for event bus notifications
   - Add MetaSpecDriftDetected to domain events if not present

5. **Tests**:
   - Unit test: reconciliation creates tasks for affected repos
   - Unit test: repos not referencing the changed meta-spec are not affected
   - Unit test: conformance sweep detects version drift
   - Unit test: duplicate reconciliation tasks are not created (deduplication)

## Acceptance Criteria

- [ ] Meta-spec approval triggers reconciliation task creation for affected repos
- [ ] Reconciliation tasks have correct title, labels, and priority
- [ ] Conformance sweep background job runs on schedule (configurable interval)
- [ ] Conformance sweep creates drift-review tasks when meta-spec version mismatch detected
- [ ] ReconciliationCompleted events emitted on completion
- [ ] MetaSpecDrift notifications sent to workspace members
- [ ] Duplicate reconciliation tasks are deduplicated
- [ ] Tests cover trigger, sweep, and deduplication

## Agent Instructions

- Read `crates/gyre-server/src/api/meta_specs.rs` for existing meta-spec approval and set-update handlers
- Read `crates/gyre-server/src/lib.rs` for `emit_reconciliation_completed()` — the notification infrastructure is ready
- Read `crates/gyre-server/src/jobs.rs` for how to register a new background job
- Read `crates/gyre-common/src/notification.rs` for MetaSpecDrift notification type
- Read `crates/gyre-common/src/message.rs` for ReconciliationCompleted MessageKind
- The task creation pattern is in `crates/gyre-server/src/git_http.rs` (spec lifecycle task creation) — follow the same deduplication pattern

## Implementation Result

Review findings from the prior two rounds are repaired in the working tree:

1. **§10 SHA-flip defect (reconciliation.rs finding)** — `MetaSpecSet.personas`
   and `UpdateMetaSpecSetRequest.personas` are `BTreeMap` instead of `HashMap`
   (`crates/gyre-server/src/api/meta_specs.rs`): the stored set bytes feed
   `compute_meta_spec_set_sha`, and `HashMap` iteration order is seed-random
   per deserialization, so an identical re-PUT could flip the set SHA and make
   the §10 conformance sweep manufacture false drift against provenance
   recorded minutes earlier. Regression test
   `reconciliation::tests::identical_reput_keeps_set_sha_stable` (5 persona
   bindings, re-PUT, SHA equality, then sweep asserts zero drift) — fails on
   the old HashMap behavior.
2. **Review probe test (zz_review_probe.rs finding)** — the temporary
   integration probe was deleted; the regression test now lives in-crate as
   `reconciliation::tests::identical_reput_keeps_set_sha_stable`, and the
   `make_set` test helper uses distinct persona keys so multi-entry sets no
   longer collapse to their last entry.
3. **Baseline reproduction (no-test-inflation evidence)** — in an isolated
   worktree at the pre-fix base (own `CARGO_TARGET_DIR`), a temporary in-crate
   probe asserting `distinct > 1` over 40 identical re-PUTs **passed on the
   old HashMap code**, mechanically proving the SHA flip; the committed fix
   inverts that outcome (stable SHA across re-PUTs).

Focused checks (this sandbox): `cargo test -p gyre-server --lib reconciliation`
(14 passed incl. the new regression test), `cargo test -p gyre-server --lib
meta_specs` (14 passed), `cargo build -p gyre-server` clean, clippy adds no
new findings (the `-D warnings` error in `gyre-common/src/view_query.rs:798`
is baseline code untouched by this task).

Unresolved: none for §6/§10/§11 — full-suite and lint gates left to the
controller per round instructions.
