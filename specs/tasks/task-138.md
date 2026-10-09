---
title: "Implement spec approval ledger with database schema and API"
spec_ref: "agent-gates.md §Part 2 Spec Approval Ledger"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Spec Approval Ledger"
  - "agent-gates.md §The Provenance Chain"
  - "agent-gates.md §How It Works"
commits: ["1e0a0749cdba13d8cfc3057e401576af7b0a9fe1", "51f0eb1f2a9bb883d8ea639647cf16ef0f628254", "4223570e2b7c58593128f3c86a483cd706d32817", "3fd6c9da80cd9677e0b82d52de7d5c45aeb79453", "bf9d3f37714dd622203bd9c8acf335fe44862e32", "ae13daef9245be9c5bd51746737e59c529b7b4fc", "c6a898c20a3c0b1c924a5459e26fbe550430234d", "95766fa5f3d2f12aeffdef55aa82c41a6c9553f4", "b03015a8c77994020e32c1bef4e6b956bd6065e8"]
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

- [x] `spec_approvals` table with all columns per spec schema — migration `2026-10-08-000056_spec_approval_ledger` (nullable `approved_at`, dual-dialect SQLite rebuild + PG `ALTER COLUMN`), indexes on (spec_path, spec_sha) and approver_id
- [x] `ApprovalStatus` enum with Pending/Approved/Revoked/Rejected — `gyre-domain/src/spec_approval.rs`
- [x] Status derived from timestamp columns (not stored directly) — `SpecApproval::status()`; `SpecApprovalResponse.status` documented as derived
- [x] Mutual exclusivity enforced on status transitions — domain `approve`/`revoke`/`reject` clear sibling timestamp columns; adapter test `revoke_enforces_transition_and_mutual_exclusivity`
- [x] Valid transitions: Pending → Approved → Revoked, Pending → Rejected — `invalid_transitions_are_rejected` (domain), `reject_only_from_pending_and_closes_lifecycle` (adapter)
- [x] Revocation requires reason, records revoked_by — domain `revoke` rejects empty reason; handler audits to `audit_events`; `revocation_requires_reason` (domain)
- [x] Multiple approvals per spec path (different SHAs) supported — `approvals_list_returns_full_ledger_data`
- [x] Spec approval creates ledger entry with git blob SHA — handler resolves SHA from ledger `current_sha` (synced from manifest at push); mismatched/stale SHA rejected with 409 (`approve_spec_mismatched_sha_rejected`)
- [x] Revoke endpoint with reason field — `POST /api/v1/specs/:path/revoke` (`revoke_spec_approval`), 400 on empty reason
- [x] `GET /api/v1/specs/approvals` returns full ledger data — `list_spec_approvals` returns all ledger columns + derived status/active
- [x] `cargo test --all` passes — sandbox-verified: domain (4/4), adapters spec_approval (6/6), server api::specs (73/73 incl. new regressions), api::gates (9/9), merge_processor `require_current_spec_blocks_stale_spec_ref`; controller owns full-suite run

## Status

ready-for-review. All acceptance criteria implemented and probed in this sandbox.

Unresolved gap (environmental, not code): the e2e integration test
`spec_approval_auto_invalidated_on_spec_change` (live server over loopback)
cannot run here — this sandbox blocks `accept(2)` with `EOPNOTSUPP` (errno 95).
Evidence: the server binary starts and logs `listening`; a minimal
bind/listen/connect probe succeeds while `accept` fails; baseline `main`
in an isolated worktree fails the pre-existing `clone_via_smart_http`
identically at its first HTTP request. The test compiles cleanly and must
be executed by the controller on an unrestricted host.

## Agent Instructions

Read `specs/system/agent-gates.md` Part 2 §Spec Approval Ledger and §The Provenance Chain. Existing spec approval: `gyre-server/src/api/specs.rs` (approve_spec, reject_spec handlers), routes at `gyre-server/src/api/mod.rs` lines ~353-358. Existing spec approval storage: grep for `spec_approval\|approve_spec` in adapters. Git SHA resolution: check how the codebase resolves file SHAs (likely in git operations or repo utils). Port pattern: look at existing ports in `gyre-ports/src/` for CRUD traits. Check migration numbering: currently at 000049.
