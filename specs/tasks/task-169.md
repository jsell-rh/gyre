---
title: "Spec registry — supply chain AIBOM integration"
spec_ref: "spec-registry.md §Supply Chain (supply-chain.md)"
depends_on: [task-165]
progress: not-started
coverage_sections:
  - "spec-registry.md §Supply Chain (supply-chain.md)"
commits: []
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
