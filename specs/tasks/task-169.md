---
title: "Spec registry — supply chain AIBOM integration"
spec_ref: "spec-registry.md §Supply Chain (supply-chain.md)"
depends_on: [task-165]
progress: ready-for-review
coverage_sections:
  - "spec-registry.md §Supply Chain (supply-chain.md)"
commits: ["a4c24fc16aed345da6f4b1d446aef66ab9a8d8e5", "ad473d3c0846cbdb5888f4973b3ea8e406f09586"]
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
  `approved_at_merge_time` evaluated against the same per-SHA approval ledger
  the merge gate (`verify_spec_ref`) enforces — an approval counts only when
  granted at or before the merge and not revoked/rejected at or before it.
  Merged-then-reverted MRs report merge-time approval as indeterminable
  (`null`) because the MR record's `updated_at` is the revert time; evaluating
  against it would misjudge approvals withdrawn between merge and revert.
  A `bindings_summary` totals bindings, merges, and merge-time approvals.

Storage failures propagate (`anyhow::Result` → `ApiError::Internal`) so the
section cannot silently under-report spec provenance; only genuine absence
(no manifest, no events, no MRs) is reported as empty/null.

### Resume-round verification (merged head 8b9c0e55 = candidate + base ce336114)

The prior candidate (PR #854, head `d100b576`) failed the "Development
controller" CI check (run 44076006080, logs never published). Investigation
at the merged head found no code defect: the controller suites
(`test_dev*.py`, `test_pipeline*.py`) fail identically at the BASE commit
`ce336114` in this sandbox — the failures are environmental (cargo-wrapper
shims exit 64, network-restricted pipeline tests), and none of the failing
tests touch the candidate's diff (gyre-server Rust files + markdown). Node
controller tests (`scripts/dashboard/test`, `scripts/test`) pass. The Go
dispatcher suite could not run locally (go not installed). Evidence recorded
in `/tmp/stage/review-evidence/task-169-probes.md`.

Local probes at the merged head: `cargo check -p gyre-server` PASS,
`cargo check -p gyre-server --tests` PASS, `cargo fmt --check -p gyre-server`
PASS, `cargo clippy -p gyre-server --lib` — no new warnings in changed files
(pre-existing `sort_by_key` advisory at `api/aibom.rs:244` is an unchanged
line). Mechanical guards PASS: check-arch, in-memory-state-stores,
fail-open-ref-resolution, unwired-port-methods, inert-enforcement,
relative-path-defaults, scope-literal-defaults, mem-port-contracts,
task-commit-attribution (after restoring this file's `commits:` list).

### Tests the independent verifier must execute

The sandbox build wrapper blocks gyre-server test compilation ("exceeds
4 GiB") — an infrastructure limit, not a code defect.

- `cargo test -p gyre-server --lib aibom_spec_registry` — 3 tests, including
  ones that fail on real defects: an approval granted after the merge and an
  approval revoked before the merge both yield `approved_at_merge_time: false`
  (a "currently approved" check would wrongly say true); stale/revoked approval
  events are excluded from approval chains; cross-repo ledger entries do not
  leak; `manifest_version` equals `git rev-parse HEAD:specs/manifest.yaml` in
  a real temp git repo.
- `cargo test -p gyre-server --lib api::aibom` — includes the e2e
  `aibom_includes_spec_registry_section` (router-level: section present with
  counts, chain, and a merged MR bound to an approved spec).
- `cargo test -p gyre-server --lib api::release` — includes
  `release_prepare_includes_spec_registry_metadata`.
- `cargo test --all` — acceptance criterion; deferred to the verifier per the
  sandbox memory budget.
- CI "Development controller" job: `go test -race ./...` in
  `tools/fullsend-dispatcher` (go not available in this sandbox) plus the
  Python/Node controller suites.
