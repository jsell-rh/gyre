---
title: "Implement spec approval ledger with database schema and API"
spec_ref: "agent-gates.md §Part 2 Spec Approval Ledger"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Spec Approval Ledger"
  - "agent-gates.md §The Provenance Chain"
  - "agent-gates.md §How It Works"
commits: ["72f61cbdc55fab1c8a593b2961db695e0804dcd2", "2f424b88f966e26d656ecac0d382586f26bcc705"]
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

## Shipped

Spec approval ledger implemented as a real port-backed store with enforced
lifecycle transitions, plus the repair for the contract finding.

- **Domain** (`gyre-domain/src/spec_approval.rs`): `SpecApproval` with the
  full spec schema; `ApprovalStatus` derived from which timestamp column is
  non-null (never stored); transition methods `approve`/`revoke`/`reject`
  enforce Pending → Approved → Revoked and Pending → Rejected, reject empty
  revocation reasons, and clear sibling timestamp columns for mutual
  exclusivity.
- **Port** (`gyre-ports/src/spec_approval.rs`): `SpecApprovalRepository` —
  create (fails on duplicate id), find_by_id, list_by_path,
  **find_by_spec_sha** (exact (path, sha) version query — plan step 2/6),
  list_active_by_path, list_all, transition methods, revoke_all_for_path.
- **Migration** `2026-10-08-000056_spec_approval_ledger`: nullable
  `approved_at` (NULL while Pending), dual-dialect SQLite rebuild + PG
  portability, indexes on (spec_path, spec_sha) and approver_id.
- **Adapters**: SQLite, Postgres, and mem implement the port with identical
  contracts (mem enforces the duplicate-id guard in code;
  check-mem-port-contracts.sh passes).
- **API**: `POST /api/v1/specs/:path/approve` validates the requested SHA
  against the ledger's current blob SHA (409 on mismatch, no partial writes)
  and creates + approves the ledger entry; `POST /api/v1/specs/:path/revoke`
  requires a reason, restricts to original approver or Admin, and audits;
  `POST /api/v1/specs/:path/reject` transitions Pending rows to Rejected and
  closes spec-edit MRs; `GET /api/v1/specs/approvals` returns full ledger
  rows with derived status/active.
- **Forge enforcement** (§The Provenance Chain step 8): `verify_spec_ref`
  queries the exact (path, sha) version via `find_by_spec_sha` and accepts
  only active (Approved, not revoked/rejected) rows; `require_current_spec`
  blocks merges on stale spec SHAs.
- **Push-time invalidation**: a push modifying a spec file revokes all
  active approvals for that path (ledger-path normalization so git paths
  match ledger rows).

**Contract repair** (finding 4f9f2afe1fda4061b89edd486f71331b): the previous
assignment's failure was bookkeeping, not code — commit 90880a97 checked the
Acceptance Criteria boxes with appended evidence, which changes the task's
requirement text (`scripts/dev-contract.py requirement_parts` keeps all prose
except `## Shipped`/`## Review`). This run restores the contract verbatim
(boxes unchecked; this section carries the evidence instead) and adds the
substantive repair above: the port method the plan names (`find_by_spec_sha`)
and the per-SHA query backing "on query: return approval status per SHA",
implemented across all three adapters with `verify_spec_ref` rewired to it.

Verification (this sandbox, focused suites; evidence in
`/tmp/stage/review-evidence/task-138-repair.md`):
- `cargo test -p gyre-adapters --lib sqlite::spec_approval` — 7/7 (incl. new
  `find_by_spec_sha_returns_only_that_version`).
- `cargo test -p gyre-server --lib -- api::gates` — 10/10 (incl. new
  `find_by_spec_sha_scopes_to_exact_version`: a Pending sibling SHA must not
  verify while another SHA of the same path is approved).
- `cargo test -p gyre-server --lib -- api::specs` — 73/73;
  `push_modifying_spec_revokes_ledger_approvals` +
  `require_current_spec_blocks_stale_spec_ref` — pass.
- `cargo test -p gyre-domain spec_approval` — 4/4.
- Mechanical invariants pass: migration-versions, arch, mem-port-contracts,
  inert-enforcement, migration-SQL-portability, ABAC route registry.

Sandbox limitation (recorded, not a code defect): the e2e test
`spec_approval_auto_invalidated_on_spec_change` needs a TCP listener;
`accept(2)` is blocked with EOPNOTSUPP errno 95 (`/tmp/stage/capabilities.json`).
The identical lifecycle is proven listener-free by
`git_http::tests::push_modifying_spec_revokes_ledger_approvals`. Host
checklist: e2e test, `cargo test --all`, GitHub CI on the exact head.
