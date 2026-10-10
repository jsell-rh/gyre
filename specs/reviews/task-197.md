# Review — task-197 (Materialize default Tenant entity on first-startup bootstrap)

Spec: `specs/system/hierarchy-enforcement.md` §1 "Bootstrap Behavior" (§50-66); coverage row §5 "Bootstrap Behavior".
Commit under review: `6258aa4b168249eb7d21fa862a05d79db8ea3b9d` (implementation carried by `71901c54`, recorded in the task's `commits:` frontmatter; `6258aa4b` restores the task file after the resume-flow reset — no production-code delta beyond `71901c54`).
Verdict: **approved**.

## Round 1

Test runs (working tree == candidate, `git status` clean, HEAD == `6258aa4b`; no shared-target contamination — isolated checkout):

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib bootstrap_default_tenant` → **1 passed, 0 failed** (1189 filtered out).
- Hardness probe: replaced the entire `bootstrap_default_tenant` body with a no-op → same test **FAILS** (exit 101, panic "default tenant materialized after bootstrap" at lib.rs:1655). Probe reverted; post-restore re-run passes (1 passed). Evidence: `/tmp/stage/review-evidence/task-197-focused-test.md`.
- `bash scripts/check-arch.sh` → OK. `bash scripts/check-mem-port-contracts.sh` → OK (no new adapter divergence).

Verified working (no findings):

- **Real durable row, not orphaned**: the bootstrap creates `Tenant { id: "default", name: "Default", slug: "default", oidc_issuer: None, budget: Some(BudgetConfig::default()), max_workspaces: None, created_at: now }`. `id = "default"` is the operative single-tenant scope every Diesel adapter filters on (`SqliteStorage::new` → `new_for_tenant(…, "default")`, sqlite/mod.rs:86-93), so the row is the tenant existing data references — exactly the reconciliation the task mandates. No `deterministic_uuid` helper, no tenant_id-column migration (correctly deferred per spec §68-70).
- **Startup wiring**: `main.rs` calls `bootstrap_default_tenant(&state)` immediately after `build_state`, before `abac_middleware::seed_builtin_policies` and `seed_builtin_meta_specs` — the tenant exists before any tenant-scoped seeding.
- **Durable in production mode**: `build_state` wires `state.tenants` to the real `SqliteStorage`/`PgStorage` `TenantRepository` via the `store!` macro when `GYRE_DATABASE_URL` is set (lib.rs:995-998, 836-846), so the bootstrap writes through the same repository production handlers use. In pure in-memory mode it materializes into the mem store (harmless, consistent).
- **Idempotency / convergence**: early-return on `find_by_id("default")`; both SQLite and PG `TenantRepository::create` are upserts on `tenants.id` (on_conflict do_update), so even a concurrent-startup race converges on one row rather than erroring. The test asserts a second call leaves exactly one slug="default" tenant.
- **Test quality**: the test wires a fresh temp-file `SqliteStorage` (real Diesel migrations) into `state.tenants` the same way `build_state` does in DB mode, asserts `find_by_id("default")` is `None` pre-bootstrap, then asserts the materialized row's id/name/slug/oidc_issuer/max_workspaces/created_at and budget (serialized comparison — `BudgetConfig` lacks `PartialEq`). It fails when the bootstrap is a no-op (probe above), so it verifies the row, not mirrored logic.
- **Non-goals respected**: demo `/admin/seed` (admin.rs) and `POST /api/v1/tenants` untouched; no default workspace created. The admin seed path's `find_by_id` idempotency check means it skips creation for a "default"-tenant caller where bootstrap already ran; its `slug = tenant_id` construction does not collide with the bootstrap row's slug for other tenants (different slug values).

Notes (not findings):

- TCP listener probes are unsupported in this review sandbox (capabilities.json, errno 95), so no live-server smoke was run; the SQLite-backed lib test exercises the production bootstrap function against the real adapter, which is the smallest meaningful probe. Host/CI verification of the live startup path: set `GYRE_DATABASE_URL=sqlite://<tmp>/gyre.db`, start `gyre-server`, then `GET /api/v1/tenants` (admin auth) must list the "Default" tenant, and a restart must leave exactly one.
- `tracing::warn!`-and-continue on create failure matches the repo's seed-fn convention (`seed_builtin_meta_specs`, `register_default_compute_target`); a failed bootstrap does not block startup, consistent with the task's chosen pattern.
