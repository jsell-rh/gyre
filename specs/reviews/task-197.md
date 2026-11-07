# Review: task-197 — Materialize default Tenant entity on first-startup bootstrap

**Candidate:** abe03947edf55013d01f229ca3b53872de47bf85
**Base:** 4d6759a0bfd637e899bbf65e725f91b07f745c69
**Spec:** hierarchy-enforcement.md §1 Bootstrap Behavior (§50-66); coverage §5
**Verdict:** Approved — evidence in /tmp/stage/review-evidence/task-197-review.md

## Scope

The candidate→base diff contains only process bookkeeping (task frontmatter, coverage
§5 row task-assigned → implemented). The task-197 implementation was absorbed into the
mainline base via merge 28a0b0855 and is intact in the candidate tree. Task commits
71901c549 and 2419125dd touch only `crates/gyre-server/src/lib.rs` and
`crates/gyre-server/src/main.rs`; `api/admin.rs` and `api/tenants.rs` untouched, per
instructions.

## Implementation verified

- `bootstrap_default_tenant` (gyre-server/src/lib.rs:1370) follows the existing seed-fn
  convention: async, `&Arc<AppState>`, early-return idempotency on `find_by_id`,
  `tracing::warn!` on create error.
- Field values match the reconciled spec: id `"default"` (the operative single-tenant
  scope — build_state's `store!` macro wires `state.tenants` to SqliteStorage/Postgres in
  DB-backed mode, so the row backs existing tenant-filtered data and is not an orphaned
  UUID), name "Default", slug "default", `oidc_issuer: None`,
  `budget: Some(BudgetConfig::default())`, `max_workspaces: None`, `created_at: now`.
- Wired in main.rs:47 immediately after `build_state`, before `seed_builtin_policies`.
- No `deterministic_uuid` helper, no tenant_id column migration, no default workspace —
  deferred multi-tenant work correctly left alone.

## Probes run (all evidence saved)

1. **Focused test** `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib
   bootstrap_default_tenant` → 1 passed. The test exercises the real SQLite
   TenantRepository over a fresh temp DB: asserts None before bootstrap, spec field
   values after, and exactly one default tenant after a second call.
2. **Hardness probe**: bootstrap body made a no-op → the shipped test FAILS
   ("default tenant materialized after bootstrap"). The test verifies the materialized
   row, not mirrored logic. Probe reverted; post-revert run passes; tree clean.
3. **Restart durability probe** (temporary, reverted): tenant survives a full
   SqliteStorage re-open over the same file, and a second bootstrap on the re-opened
   state creates no duplicate. The row is durable, not in-memory-only.
4. **`bash scripts/check-arch.sh`** → passed.

## Findings

None. All acceptance criteria independently proven.
