---
title: "Implement spec approval ledger with database schema and API"
spec_ref: "agent-gates.md §Part 2 Spec Approval Ledger"
depends_on: []
progress: not-started
coverage_sections:
  - "agent-gates.md §Spec Approval Ledger"
  - "agent-gates.md §The Provenance Chain"
  - "agent-gates.md §How It Works"
commits: []
---

## Spec Excerpt

From `agent-gates.md` §Spec Approval Ledger:

```sql
spec_approvals table:
  id              TEXT PRIMARY KEY
  spec_path       TEXT NOT NULL        -- e.g., "specs/system/identity-security.md"
  spec_sha        TEXT NOT NULL        -- git blob SHA at approval time
  approver_id     TEXT NOT NULL        -- user or agent who approved
  signature       TEXT                 -- Sigstore signature (optional but recommended)
  approved_at     INTEGER             -- NULL when status is Pending
  revoked_at      INTEGER             -- NULL unless revoked
  revoked_by      TEXT
  revocation_reason TEXT
  rejected_at     INTEGER             -- NULL unless rejected
  rejected_by     TEXT
  rejected_reason TEXT
```

**ApprovalStatus enum:**
```rust
pub enum ApprovalStatus {
    Pending,    // no timestamp columns set
    Approved,   // approved_at is set
    Revoked,    // revoked_at is set (post-merge withdrawal)
    Rejected,   // rejected_at is set (pre-merge decline)
}
```

**Status Logic:** Status derived from which timestamp column is non-null. Mutual exclusivity: handler clears all other timestamp columns when setting new status.

**Transitions:** Pending → Approved → Revoked OR Pending → Rejected. Revocation requires reason and is audited. Rejection closes associated MR. Multiple approvals can exist for same spec (different versions).

From §The Provenance Chain (9-step chain):
1. Spec authored (committed, gets git SHA)
2. Spec approved (reviewer signs spec version via `POST /api/v1/specs/approve`)
3. Task created (references `spec_ref: "path@sha"`)
4. Agent dispatched (context includes spec content at exact SHA)
5. Commits produced (include spec provenance in agent_commits)
6. MR created (references `spec_ref`)
7. Gate agent reviews (receives spec at pinned SHA, not HEAD)
8. Forge validates at merge (spec SHA exists, has approval, not revoked)
9. Merged (full provenance chain recorded)

## Implementation Plan

1. **Domain types in `gyre-domain` or `gyre-common`:**
   - `ApprovalStatus` enum: Pending, Approved, Revoked, Rejected
   - `SpecApproval` entity with all spec fields
   - Status derived from timestamp columns (computed property)

2. **Port trait in `gyre-ports`:**
   - `SpecApprovalRepository`: create, get_by_id, list_by_spec_path, find_by_spec_sha, update_status
   - Transition methods: approve, revoke, reject (enforce valid transitions)

3. **Database migration:**
   - Create `spec_approvals` table per schema above
   - Index on (spec_path, spec_sha) for lookup
   - Index on approver_id for audit queries

4. **SQLite adapter:**
   - Implement `SpecApprovalRepository`
   - Enforce mutual exclusivity: clearing other timestamps on status change
   - Support multiple approvals per spec (different versions)

5. **Enhance existing spec approval API:**
   - `POST /api/v1/specs/:path/approve` already exists — extend to create ledger entry with SHA
   - Add `POST /api/v1/specs/:path/revoke` for revocation (with reason)
   - `GET /api/v1/specs/approvals` already exists — extend to include full ledger data
   - Rejection: `POST /api/v1/specs/:path/reject` already exists — wire to ledger

6. **Spec SHA resolution:**
   - On approval: resolve spec file to its current git blob SHA
   - Store SHA in approval record
   - On query: return approval status per SHA

## Acceptance Criteria

- [ ] `spec_approvals` table with all columns per spec schema
- [ ] `ApprovalStatus` enum with Pending/Approved/Revoked/Rejected
- [ ] Status derived from timestamp columns (not stored directly)
- [ ] Mutual exclusivity enforced on status transitions
- [ ] Valid transitions: Pending → Approved → Revoked, Pending → Rejected
- [ ] Revocation requires reason, records revoked_by
- [ ] Multiple approvals per spec path (different SHAs) supported
- [ ] Spec approval creates ledger entry with git blob SHA
- [ ] Revoke endpoint with reason field
- [ ] `GET /api/v1/specs/approvals` returns full ledger data
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/agent-gates.md` Part 2 §Spec Approval Ledger and §The Provenance Chain. Existing spec approval: `gyre-server/src/api/specs.rs` (approve_spec, reject_spec handlers), routes at `gyre-server/src/api/mod.rs` lines ~353-358. Existing spec approval storage: grep for `spec_approval\|approve_spec` in adapters. Git SHA resolution: check how the codebase resolves file SHAs (likely in git operations or repo utils). Port pattern: look at existing ports in `gyre-ports/src/` for CRUD traits. Check migration numbering: currently at 000049.
