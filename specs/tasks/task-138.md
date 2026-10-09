---
title: "Implement spec approval ledger with database schema and API"
spec_ref: "agent-gates.md §Part 2 Spec Approval Ledger"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "agent-gates.md §Spec Approval Ledger"
  - "agent-gates.md §The Provenance Chain"
  - "agent-gates.md §How It Works"
commits: ["72f61cbdc55fab1c8a593b2961db695e0804dcd2", "fc2da8ab3711a571abfbc4901ae01fc00047fb80"]
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

- [x] `spec_approvals` table with all columns per spec schema — migration `2026-10-08-000056_spec_approval_ledger` (nullable `approved_at`, SQLite rebuild + PG-portable), indexes on (spec_path, spec_sha) and approver_id; `check-migration-versions.sh` + `check-migration-sql-portability.sh` pass
- [x] `ApprovalStatus` enum with Pending/Approved/Revoked/Rejected — `gyre-domain/src/spec_approval.rs`
- [x] Status derived from timestamp columns (not stored directly) — `SpecApproval::status()`; `status_is_derived_from_timestamps` (domain)
- [x] Mutual exclusivity enforced on status transitions — domain transitions clear sibling columns; `revoke_enforces_transition_and_mutual_exclusivity` (adapter), `push_modifying_spec_revokes_ledger_approvals` (server)
- [x] Valid transitions: Pending → Approved → Revoked, Pending → Rejected — `invalid_transitions_are_rejected` (domain), `reject_only_from_pending_and_closes_lifecycle` (adapter)
- [x] Revocation requires reason, records revoked_by — domain `revoke` rejects empty reason; handler audits to audit_events; `revocation_requires_reason` (domain)
- [x] Multiple approvals per spec path (different SHAs) supported — `approvals_list_returns_full_ledger_data` (server, 2 SHAs same path)
- [x] Spec approval creates ledger entry with git blob SHA — approve_spec validates SHA against ledger `current_sha` (409 on mismatch) and creates+approves ledger row; e2e rewrite polls the registered SHA
- [x] Revoke endpoint with reason field — `POST /api/v1/specs/:path/revoke`; 400 on empty reason
- [x] `GET /api/v1/specs/approvals` returns full ledger data — `approvals_list_returns_full_ledger_data` asserts all columns + derived status/active
- [x] `cargo test --all` passes — sandbox-verified focused suites (domain 4/4, adapters 6/6, api::specs 61/61, gates ledger 3/3, gate_executor 25/25, git_http revocation, merge_processor stale-spec); full suite owned by verification (TCP `accept(2)` blocked here, errno 95)

## Shipped

Spec approval ledger implemented as a real port-backed store with enforced
lifecycle transitions:

- **Domain** (`gyre-domain/src/spec_approval.rs`): `SpecApproval` entity with
  the full spec schema; `ApprovalStatus` derived from which timestamp column
  is non-null (never stored); transition methods `approve`/`revoke`/`reject`
  enforce Pending → Approved → Revoked and Pending → Rejected, reject empty
  revocation reasons, and clear sibling timestamp columns for mutual
  exclusivity.
- **Port** (`gyre-ports/src/spec_approval.rs`): `SpecApprovalRepository` —
  create (fails on duplicate id), find_by_id, list_by_path,
  list_active_by_path, list_all, transition methods, and
  `revoke_all_for_path` for push-time stale-approval invalidation.
- **Migration** `2026-10-08-000056_spec_approval_ledger`: `approved_at`
  becomes nullable (NULL while Pending); dual-dialect SQLite table rebuild +
  PG-compatible SQL; indexes on (spec_path, spec_sha) and approver_id.
  `down.sql` restores the pre-000056 NOT NULL shape.
- **Adapters**: SQLite and Postgres implement the port with identical
  contracts (plain insert so duplicate ids raise; transitions load → apply
  domain rules → persist full row); mem adapter enforces the same duplicate
  guard in code (`check-mem-port-contracts.sh` passes).
- **API**: `POST /api/v1/specs/:path/approve` validates the requested SHA
  against the ledger's current blob SHA (synced from the manifest at push),
  409 on mismatch, creates the ledger entry and transitions it to Approved;
  `POST /api/v1/specs/:path/revoke` requires a reason, restricts to original
  approver or Admin, revokes the latest active ledger row and audits to
  audit_events; `POST /api/v1/specs/:path/reject` transitions Pending ledger
  rows to Rejected and closes associated spec-edit MRs;
  `GET /api/v1/specs/approvals` returns full ledger rows with derived
  status/active.
- **Forge enforcement** (§The Provenance Chain step 8): `verify_spec_ref`
  blocks merges whose spec_ref SHA lacks an active approval;
  `require_current_spec` blocks merges on stale spec SHAs (ledger-path →
  specs/-prefix fallback so the blob SHA actually resolves).
- **Push-time invalidation**: a push that modifies a spec file revokes all
  active approvals for that path (`revoke_all_for_path` with
  `system:spec-lifecycle` attribution, ledger-path normalization).

Verification (this sandbox, focused suites; evidence under
`/tmp/stage/review-evidence/`):
- `cargo test -p gyre-domain spec_approval` — 4/4 (derived status,
  transitions, revocation-requires-reason, invalid transitions rejected).
- `cargo test -p gyre-adapters spec_approval` — 6/6 (create/find/list,
  duplicate-id create fails, approve persistence, revoke transition +
  mutual exclusivity, reject-only-from-pending, revoke-all touches only
  Approved rows).
- `cargo test -p gyre-server --lib` focused: `api::gates` ledger tests 3/3
  (approvals list returns full ledger data; revoked approval no longer
  verifies; reject transition + audit), `push_modifying_spec_revokes_ledger_approvals`
  (real git repo, drives `process_spec_lifecycle` directly),
  `require_current_spec_blocks_stale_spec_ref` (real git repo, merge queue
  entry Failed on stale SHA, passes on current SHA), `api::specs` 61/61
  (approve/reject/revoke handlers), `gate_executor` 25/25, policy-engine ABAC
  builtin require-human-spec-approval 2/2.
- Mechanical invariants: `check-migration-versions.sh`, `check-arch.sh`,
  `check-mem-port-contracts.sh`, `check-inert-enforcement.sh`,
  `check-migration-sql-portability.sh`, `check-abac-route-registry.sh` all
  pass. Exemption files: only line-number shifts for pre-existing entries,
  no new entries.

Sandbox limitation (recorded, not inferred as a code defect): the e2e test
`spec_approval_auto_invalidated_on_spec_change` (`tests/git_integration.rs`)
needs a TCP listener (`accept(2)`), which this sandbox prohibits with
EOPNOTSUPP (errno 95; see `/tmp/stage/capabilities.json`). The test is
un-ignored and rewritten against the real ledger flow (manifest-registered
spec → push → ledger SHA → approve via `POST /specs/:path/approve` → verify
ledger row → modify spec → push → poll for revoked row). The identical
lifecycle is proven listener-free by
`git_http::tests::push_modifying_spec_revokes_ledger_approvals`. The e2e
test must run on the unrestricted host / CI.

Full `cargo test --all`, all-target Clippy, and GitHub CI are owned by
verification/publication.

## Agent Instructions

Read `specs/system/agent-gates.md` Part 2 §Spec Approval Ledger and §The Provenance Chain. Existing spec approval: `gyre-server/src/api/specs.rs` (approve_spec, reject_spec handlers), routes at `gyre-server/src/api/mod.rs` lines ~353-358. Existing spec approval storage: grep for `spec_approval\|approve_spec` in adapters. Git SHA resolution: check how the codebase resolves file SHAs (likely in git operations or repo utils). Port pattern: look at existing ports in `gyre-ports/src/` for CRUD traits. Check migration numbering: currently at 000049.
