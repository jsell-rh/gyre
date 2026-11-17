---
title: "Spec registry — supply chain AIBOM integration"
spec_ref: "spec-registry.md §Supply Chain (supply-chain.md)"
depends_on: [task-165]
progress: ready-for-review
coverage_sections:
  - "spec-registry.md §Supply Chain (supply-chain.md)"
commits: ["4bec0af594c555ae9dd807eacaa967cb3118722f", "ad473d3c0846cbdb5888f4973b3ea8e406f09586", "a4c24fc16aed345da6f4b1d446aef66ab9a8d8e5"]
---

## Spec Excerpt

### §Supply Chain (spec-registry.md)

The spec registry should integrate with the supply chain system to include manifest state in AIBOM artifacts. When generating an AIBOM for a release, the registry provides: which specs were approved, their approval status, the approval chain (who approved, when), and the spec-to-code binding status (all linked code covered by approved specs).

## Implementation Plan

1. **AIBOM spec registry data**: Extend the AIBOM generation (in `aibom.rs` or equivalent) to include spec registry data:
   - List of all specs in the repo's `specs/manifest.yaml` with their ledger status (Approved, Pending, Deprecated)
   - For each approved spec: approval timestamp, approver(s), approval mode
   - Spec coverage summary: how many specs are fully approved vs pending
   - Spec-to-code binding status: which MRs had `spec_ref` links and whether those specs were approved at merge time

2. **API extension**: Add a `spec_registry` section to the existing AIBOM response:
   ```json
   {
     "spec_registry": {
       "total_specs": 12,
       "approved": 10,
       "pending": 2,
       "specs": [
         {
           "path": "specs/system/auth.md",
           "status": "approved",
           "approved_by": "user-123",
           "approved_at": 1711324800
         }
       ]
     }
   }
   ```

3. **Release AIBOM packaging**: When the release endpoint (`POST /api/v1/release/prepare`) generates a release, include the spec registry AIBOM data in the release metadata.

## Acceptance Criteria

- [ ] AIBOM response includes `spec_registry` section with approval data
- [ ] All spec statuses from the ledger are included
- [ ] Approval chain (who, when) is present for approved specs
- [ ] Release preparation includes spec registry metadata
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/spec-registry.md` §"Supply Chain (supply-chain.md)". The AIBOM endpoint is registered in `api/mod.rs` at `GET /api/v1/repos/:id/aibom`. The spec registry is in `crates/gyre-server/src/spec_registry.rs` — the `SpecLedgerEntry` type has all the status and approval data. The release endpoint is in `api/release.rs`. The AIBOM generation logic is in the handler for the `/aibom` route.

## Shipped

The spec registry contributes manifest state to AIBOM artifacts and release
metadata (spec-registry.md §Supply Chain). Module
`crates/gyre-server/src/aibom_spec_registry.rs` builds one shared `spec_registry`
section from real storage, embedded verbatim by both surfaces:

- `GET /api/v1/repos/:id/aibom` — `spec_registry` field on `AibomResponse`
  (`aibom_version` bumped 1.1 → 1.2 for the schema extension).
- `POST /api/v1/release/prepare` — `spec_registry` field on
  `ReleasePrepareResponse`, so release metadata and the standalone AIBOM are
  built by the same function and cannot disagree.

### What the section contains (all from real queries)

- Every ledger approval status for the repo's specs — approved, pending,
  deprecated, revoked, rejected — with per-status counts and `total_specs`
  (coverage summary). Ledger entries are scoped by `repo_id`; other repos'
  entries do not leak in.
- Approval chain for approved specs: the active approval events for the
current version from the approval event history — `approved_at` (earliest),
  `approved_by` (ids), and a full `approval_chain` (approver_id, approver_type,
  persona, approved_at). Stale-SHA events and revoked events are excluded.
  `approval_mode` comes from the ledger entry.
- Drift risk per spec-registry.md: `drift_status` per spec plus a `drifted`
  count.
- `manifest_version`: git blob SHA of `specs/manifest.yaml` at HEAD via
  `git rev-parse` (path validated against injection); reported `null` when
  there is no manifest — never fabricated.
- Spec-to-code binding status: every repo MR carrying a `spec_ref`, with
  `approved_at_merge_time` evaluated three-valued against the same per-SHA
  approval ledger the merge gate (`verify_spec_ref`) enforces — `true` when a
  row was determinably active at the merge moment, `false` for grants after
  the merge and withdrawals at/before it, `null` when indeterminable
  (unmerged or merged-then-reverted MR, unpinned ref, or approvals withdrawn
  only after the merge — withdrawal clears the grant timestamp). A
  `bindings_summary` totals bindings, merges, and merge-time approvals.

Storage failures propagate (`anyhow::Result` → `ApiError::Internal`) so the
section cannot silently under-report spec provenance; only genuine absence
(no manifest, no events, no MRs) is reported as empty/null.

### Review repair (commit 4bec0af5)

Round-2 review finding: the original `approved_at_merge_time` predicate
required `approved_at` to be set, but `SpecApproval::revoke()`/`reject()`
clear it (mutual exclusivity), so an approval granted before the merge and
withdrawn after it — the systematic case, since every push modifying a spec
file auto-revokes that path's approvals — was misreported as
`approved_at_merge_time: false`. Replaced with the three-valued
`merge_time_approval` helper: post-merge withdrawals now report `null`
(indeterminable), withdrawals at/before the merge keep `false`, and a
determinably-active row still reports `true`. Documented in
`docs/api-reference.md`.

### Test evidence

Local probes at this head (recorded in
`/tmp/stage/review-evidence/task-169-repair.md`):

- `cargo check -p gyre-server --tests` — PASS (type-checks lib + tests).
- `cargo fmt --check -p gyre-server` — clean.
- Behavioral probe against the real `gyre-domain` `SpecApproval`
  transitions (scratch crate under review-evidence, predicate verbatim-
  identical to the shipped helper): 8/8 pass, including the reviewer's
  exact case approved@900 / merged@1000 / revoked@2000 → `null`, and the
  proof that `revoke()`/`reject()` clear `approved_at`.

The sandbox build wrapper blocks gyre-server test compilation (4 GiB
budget) — an infrastructure limit, not a code defect. The independent
verifier must execute:

- `cargo test -p gyre-server --lib aibom_spec_registry` — unit tests,
  including `bindings_report_merge_time_approval_truthfully` with the
  revoked-after-merge, rejected-after-merge, determinable-true-wins, and
  indeterminate-dominates-false cases.
- `cargo test -p gyre-server --lib api::aibom` — e2e
  `aibom_includes_spec_registry_section`.
- `cargo test -p gyre-server --lib api::release` — e2e
  `release_prepare_includes_spec_registry_metadata`.
- `cargo test --all` — acceptance criterion.
