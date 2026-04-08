# Review: TASK-004 — Authorization Provenance Storage Adapters and Migration

**Reviewer:** Verifier  
**Date:** 2026-04-08 (R1), 2026-04-08 (R2)  
**Verdict:** complete

---

## R1 Findings

- [x] [process-revision-complete] **F1 — All 5 query methods in `ChainAttestationRepository` adapter lack `tenant_id` filtering — spec-mandated tenant isolation violation.**  
  The hierarchy-enforcement spec (`hierarchy-enforcement.md` §3) states: *"Every query method on every Diesel adapter must filter by `tenant_id`."* The `save` method correctly stores `self.tenant_id` in the `tenant_id` column (chain_attestation.rs:110), but `find_by_id` (line 158), `load_chain` (line 173), `find_by_task` (line 210), `find_by_commit` (line 227), and `find_by_repo` (line 242) do not filter by `self.tenant_id`. This means attestations saved by tenant A are readable by tenant B via any of these methods. The existing `scripts/check-tenant-filter.sh` lint (required by hierarchy-enforcement.md §3) flags all 5 methods. Fix: add `.filter(chain_attestations::tenant_id.eq(&tenant_id))` to every query, and add `let tenant_id = self.tenant_id.clone();` to each method's setup. Also update `load_chain`'s inner loop query to filter by tenant_id.  
  **Files:** `crates/gyre-adapters/src/sqlite/chain_attestation.rs:158,173,210,227,242` (missing tenant filter), `specs/system/hierarchy-enforcement.md` §3 (requirement).

- [x] [process-revision-complete] **F2 — `find_active_by_identity` does not filter by `expires_at` — returns expired key bindings as "active".**  
  The port trait (`gyre-ports/src/key_binding_repo.rs:24`) documents this method as: *"Find all active (non-expired, non-invalidated) key bindings for a user identity."* The implementation (key_binding.rs:131-149) filters by `revoked_at.is_null()` (non-invalidated) but does NOT filter by `expires_at > now` (non-expired). An expired binding that hasn't been explicitly revoked will be returned as "active." The spec (authorization-provenance.md §2.3, lines 147-148) states: *"On expiry or logout, the binding is invalidated."* — but this relies on a cleanup process running. Between expiry and cleanup, `find_active_by_identity` incorrectly returns the expired binding. Fix: add `.filter(key_bindings::expires_at.gt(now))` where `now` is the current Unix epoch seconds, alongside the existing `revoked_at.is_null()` filter.  
  **Files:** `crates/gyre-adapters/src/sqlite/key_binding.rs:131-149` (missing expires_at filter), `crates/gyre-ports/src/key_binding_repo.rs:24` (port contract).

- [x] [process-revision-complete] **F3 — Missing `workspace_id` index on `chain_attestations` table per spec §5.3.**  
  The spec (authorization-provenance.md §5.3, line 517) states attestations are *"indexed by `id` (content hash), `task_id`, `repo_id`, `workspace_id`."* The migration (up.sql) creates indexes on `task_id`, `commit_sha`, `repo_id+created_at`, and `parent_ref` — but no index on `workspace_id`. The `commit_sha` and `parent_ref` indexes are correct additions (needed by `find_by_commit` and `load_chain`), but the spec-required `workspace_id` index is missing. Fix: add `CREATE INDEX IF NOT EXISTS idx_chain_attestations_workspace ON chain_attestations (workspace_id);` to up.sql and `DROP INDEX IF EXISTS idx_chain_attestations_workspace;` to down.sql.  
  **Files:** `crates/gyre-adapters/migrations/2026-04-07-000047_authorization_provenance/up.sql` (missing index), `crates/gyre-adapters/migrations/2026-04-07-000047_authorization_provenance/down.sql` (missing reverse), spec §5.3 line 517.

---

## R2 — Verification Pass

**Date:** 2026-04-08  
**Result:** All 3 R1 findings verified resolved. No new findings.

Verified:
- F1: All 5 `ChainAttestationRepository` query methods now filter by `self.tenant_id`. Tenant isolation tests pass for each method.
- F2: `find_active_by_identity` now filters by `expires_at > now` alongside `revoked_at.is_null()`. Dedicated test `find_active_excludes_expired_bindings` confirms.
- F3: `idx_chain_attestations_workspace` index present in up.sql with symmetric DROP in down.sql.

Additional verification:
- All 34 adapter tests pass (chain_attestation: 17, key_binding: 10, trust_anchor: 7).
- Schema.rs matches migration DDL for all 3 tables.
- Port trait implementations match spec §5.4 signature exactly.
- Down.sql properly reverses all indexes and tables in correct order.
- No hexagonal boundary violations (adapters import only gyre-common + gyre-ports).
