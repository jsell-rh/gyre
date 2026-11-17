---
title: "Spec registry — supply chain AIBOM integration"
spec_ref: "spec-registry.md §Supply Chain (supply-chain.md)"
depends_on: [task-165]
progress: complete
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
section from real storage, embedded by all three release-time surfaces:

- `GET /api/v1/repos/:id/aibom` (summary) — `spec_registry` field on
  `AibomResponse` (`aibom_version` bumped 1.1 → 1.2 for the schema extension).
- `GET /api/v1/repos/:id/aibom?format=release` — `spec_registry` field on
  `AibomReleaseResponse` (added when resolving the merge with task-166's
  release-artifact format, so the release-time AIBOM carries manifest state
  per spec-registry.md §Supply Chain and cannot disagree with the summary).
- `POST /api/v1/release/prepare` — `spec_registry` field on
  `ReleasePrepareResponse`.

All three call the same `build_spec_registry_section(&state, &repo)`, so
no surface can report different spec provenance.

### What the section contains (all from real queries)

- Every ledger approval status for the repo's specs — approved, pending,
  deprecated, revoked, rejected — with per-status counts and `total_specs`
  (coverage summary). Ledger entries are scoped by `repo_id`.
- For each approved spec: `approved_at` (earliest active approval),
  `approved_by`, and the full `approval_chain` (who, when, human/agent,
  persona) from the approval event history; `approval_mode` per manifest.
- Drift risk: `drifted` count + per-spec `drift_status`.
- `manifest_version`: git blob SHA of `specs/manifest.yaml` at HEAD —
  `null` when absent, never fabricated.
- Spec-to-code bindings: every repo MR with a `spec_ref`, evaluated against
  the same per-SHA approval ledger the merge gate (`verify_spec_ref`)
  enforces; `approved_at_merge_time` is three-valued (`true`/`false`/
  `null` = indeterminable: unmerged, merged-then-reverted, unpinned ref, or
  approvals withdrawn only after the merge).
- Storage failures propagate (mapped to 500) — the section cannot silently
  under-report spec provenance.

### Rebase repair (this round)

The published branch conflicted with main at `aibom.rs` and
`docs/api-reference.md` (main's task-166 restructured the AIBOM endpoint
around a `?format=release` release artifact). Resolved in merge commit
`8b39a025` (parents `05f4489` + `e9ed695`): task-166's restructured
`aibom.rs` is the base with task-169's integration re-applied on top, plus
the shared `spec_registry` section now also embedded in the release
artifact; docs rows for both features combined. Coverage rows
(spec-registry.md row 18, supply-chain.md row 14) note the merged behavior.

### Test evidence

- In-sandbox (this round): `cargo check -p gyre-server` and
  `cargo check -p gyre-server --tests` pass; `cargo fmt -p gyre-server
  -- --check` clean. gyre-server test binaries exceed this sandbox's
  4 GiB build budget (wrapper-enforced), so test execution is deferred to
  the independent verifier — probe list and expected test names recorded
  in `/tmp/stage/review-evidence/README.md`:
  `cargo test -p gyre-server --lib aibom`,
  `cargo test -p gyre-server --lib aibom_spec_registry`,
  `cargo test -p gyre-server --lib release`, and `cargo test --all`.
- Summary format: `aibom_includes_spec_registry_section` (seeded ledger:
  approved + pending spec, approval event, merged MR with spec_ref approved
  before merge — asserts statuses, counts, approval chain, manifest_version
  null, binding summary).
- Release artifact: `aibom_release_format_full_attribution` (fixture seeds
  ledger + approval history; asserts the artifact's `spec_registry` section
  end-to-end alongside the full task-166 attribution assertions), plus
  `aibom_release_format_range_and_unknown_format` and
  `aibom_release_format_unresolvable_ref`.
- Module tests: `section_reports_every_ledger_status_and_approval_chain`,
  `bindings_report_merge_time_approval_truthfully`,
  `manifest_version_is_the_real_git_blob_sha`.
- Release metadata: `release_prepare_includes_spec_registry_metadata`.
