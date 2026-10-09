---
title: "Materialize default Tenant entity on first-startup bootstrap"
spec_ref: "hierarchy-enforcement.md §1 Bootstrap Behavior (§50-66)"
depends_on: []
progress: not-started
coverage_sections:
  - "hierarchy-enforcement.md §5"
commits: ["a5f43d426920d4fd61defcf8aec505cf6fc8d95f"]
---

## Spec Excerpt

From `specs/system/hierarchy-enforcement.md` §1 "Bootstrap Behavior" (§50-66):

> On first startup (empty database), the server creates a default tenant:
>
> ```rust
> Tenant {
>     id: Id::new(deterministic_uuid("default-tenant")),
>     name: "Default".to_string(),
>     slug: "default".to_string(),
>     oidc_issuer: None,
>     budget: BudgetConfig::default(),   // No limits set — effectively unlimited
>     max_workspaces: None,
>     created_at: now(),
> }
> ```
>
> The existing `tenant_id TEXT DEFAULT 'default'` columns are migrated to reference this
> tenant's UUID. All existing rows get the default tenant's ID.

## Why this section is open (auditor finding)

The server startup path never materializes a `Tenant` row. `main.rs` (crates/gyre-server/src/main.rs:40-67) runs `build_state → seed_builtin_policies → seed_builtin_meta_specs → register_default_compute_target` — none call `state.tenants.create`. The only `tenants.create` call sites are the demo-only `/admin/seed` endpoint (`api/admin.rs:435`) and `POST /api/v1/tenants` (`api/tenants.rs:76`). On a fresh production database, **no Tenant entity exists**, so the tenant that every tenant-scoped query resolves against is never a real row.

## Critical reconciliation (read before coding)

The spec's literal `id: Id::new(deterministic_uuid("default-tenant"))` describes the *future multi-tenant* form. In the **current single-tenant** codebase the operative tenant scope is the string `"default"`: every Diesel adapter filters `.filter(<table>::tenant_id.eq("default"))` and `SqliteStorage` defaults its scope to `"default"` (crates/gyre-adapters/src/sqlite/mod.rs:84-92; see repository.rs:94, merge_request.rs:163/604, activity.rs:56, analytics.rs:115/212, etc.). The tenant table's row `id` **is** that tenant_id scope.

Therefore the bootstrap tenant MUST be created with **`id = Id::new("default")`** so it is the real row that all existing tenant-scoped data references. Creating a tenant with a fresh UUID id that no query filters on would produce an **orphaned, hollow row** — exactly the anti-pattern this task exists to eliminate. The spec's `deterministic_uuid` indirection and the "migrate tenant_id columns to the UUID" step belong to the deferred multi-tenant work (spec §68-70 "API (Deferred)" — "not required for single-tenant deployments"). Do NOT undertake that migration here; do NOT introduce a `deterministic_uuid` helper for a value nothing references.

Keep the spec-mandated field values otherwise: `name = "Default"`, `slug = "default"`, `oidc_issuer = None`, `budget = Some(BudgetConfig::default())`, `max_workspaces = None`, `created_at = now`.

Note: the demo `/admin/seed` path uses id `"default-tenant"` and name `"Default Tenant"` (admin.rs:429-435). That is inconsistent with both the spec and the adapter scope; leave it untouched (it is a demo-only bonus path, row 6 API is `n/a`). This task's bootstrap is the authoritative production path and uses `"default"`.

## Implementation Plan

1. **Add an idempotent bootstrap function** in `crates/gyre-server/src/lib.rs`, mirroring the
   existing `seed_builtin_meta_specs` / `register_default_compute_target` shape (async, takes
   `&Arc<AppState>`, exported `pub`):

   ```rust
   /// Materialize the default Tenant entity on first startup (hierarchy-enforcement §Bootstrap).
   /// Idempotent: a no-op when the default tenant already exists.
   pub async fn bootstrap_default_tenant(state: &Arc<AppState>) {
       use gyre_domain::budget::BudgetConfig;
       use gyre_domain::tenant::Tenant;

       let default_id = gyre_common::Id::new("default");
       if let Ok(Some(_)) = state.tenants.find_by_id(&default_id).await {
           return; // already bootstrapped
       }
       let now = /* unix secs, same pattern as seed_builtin_meta_specs */;
       let mut tenant = Tenant::new(default_id, "Default", "default", now);
       tenant.budget = Some(BudgetConfig::default());
       if let Err(e) = state.tenants.create(&tenant).await {
           tracing::warn!("failed to bootstrap default tenant: {e}");
       }
   }
   ```

   (`Tenant::new` at tenant.rs:19 sets `oidc_issuer=None`, `budget=None`, `max_workspaces=None`;
   only `budget` needs overriding to `Some(BudgetConfig::default())` per spec.)

2. **Export it** from the `gyre_server` crate the same way the other seed fns are re-exported
   (add to the `pub use` / module surface so `main.rs` can import it alongside
   `seed_builtin_meta_specs`).

3. **Wire it into startup** in `crates/gyre-server/src/main.rs`: call
   `bootstrap_default_tenant(&state).await;` immediately after `build_state` and **before**
   `seed_builtin_policies` (so the tenant exists before any tenant-scoped seeding), updating
   the import list at main.rs:2-7.

## Acceptance Criteria

- After startup against an empty database, `state.tenants.find_by_id(&Id::new("default"))`
  returns `Some(tenant)` with `slug == "default"`, `name == "Default"`, and
  `budget == Some(_)` (BudgetConfig::default()). The row's `id` equals the `"default"` tenant
  scope that the storage adapters filter on — i.e. it is the real tenant backing existing data,
  not an orphan.
- **Idempotent:** calling `bootstrap_default_tenant` twice (simulating a restart against a
  populated DB) results in exactly one default tenant and does not error.
- A hard test in `crates/gyre-server` that: builds an `AppState` over a fresh temp SQLite DB
  (or the in-memory store used by existing server tests), asserts `tenants.find_by_id("default")`
  is `None` before bootstrap, calls `bootstrap_default_tenant`, then asserts the tenant now
  exists with the spec field values — and asserts a second call leaves the count at one. This
  test MUST fail if the bootstrap call is removed/not wired (it verifies the materialized row,
  not mirrored logic).
- `cargo test -p gyre-server` passes.
- `bash scripts/check-arch.sh` passes (all work in `gyre-server`; no domain→adapter import).

## Agent Instructions

- Do NOT create a `deterministic_uuid` helper or migrate tenant_id columns to a UUID — that is
  deferred multi-tenant work (spec §68-70). The single-tenant operative scope is the string
  `"default"`; the bootstrap tenant's id MUST be `"default"` so it is not orphaned.
- Do NOT modify the demo `/admin/seed` path (admin.rs) or `POST /api/v1/tenants` (tenants.rs).
- Do NOT create a default workspace here — this task closes only the Tenant bootstrap section.
- Follow the existing seed-fn conventions (async, `&Arc<AppState>`, `tracing::warn!` on error,
  early-return idempotency) rather than introducing a new pattern.
- Skip formatters/linters/full-suite runs beyond the two commands above; the loop handles
  global validation.
