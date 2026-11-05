# Review — task-197 (Materialize default Tenant entity on first-startup bootstrap)

Spec: `specs/system/hierarchy-enforcement.md` §1 "Bootstrap Behavior" (§50-66): on first startup against an empty database the server creates the default tenant; per the task's Critical reconciliation the row is created with `id = "default"` — the operative single-tenant scope — rather than the spec's deferred `deterministic_uuid` multi-tenant form (§68-70).

Commits under review: `2419125dd`, `71901c549` (both verified ancestors of candidate `2bfa6bbb`). Cumulative diff `git diff 3c39ab1f..2bfa6bbb` is confined to `crates/gyre-server/src/lib.rs` (+98), `crates/gyre-server/src/main.rs` (+9/−1), `specs/tasks/task-197.md` (notes only). No other files touched; the later `process:` commits in branch history carry no product-surface changes.

Verdict: **complete** (approved).

## Evidence (commands + output under /tmp/stage/review-evidence/)

- Focused test `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-server --lib bootstrap_default_tenant` → **1 passed** (`task-197-focused-test.txt`).
- **Mutation probe:** bootstrap body temporarily made a no-op (early `return` before the create) → same test **FAILED (0 passed, 1 failed)** (`task-197-mutation-probe.txt`); reverted via `git checkout`, post-revert re-run passes (`task-197-post-revert.txt`), tree verified pristine. The test verifies the materialized row, not mirrored logic.
- Full lib suite `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-server --lib` → **1243 passed, 0 failed** (`task-197-full-lib.txt`), matching the shipped claim.
- `bash scripts/check-arch.sh` → passed (`task-197-arch.txt`).
- `bash scripts/check-task-commit-attribution.sh` → passed (`task-197-attribution.txt`): every task-labeled product-surface commit is recorded in the task's `commits:` frontmatter.

Verified working (no findings):

- **The row is the real referenced tenant, not an orphan.** The bootstrap id is `Id::new("default")`, which is exactly the scope every Diesel adapter filters on (`tenant_id.eq("default")`, e.g. `sqlite/repository.rs:94`, `merge_request.rs:163/604`, `activity.rs:56`, `analytics.rs:115/212`) and the scope `SqliteStorage::new` defaults to (`sqlite/mod.rs:84-92`). The tenants table maps `id` directly to the scope string (`sqlite/tenant.rs`), so the materialized row is the entity existing tenant-scoped data references. No `deterministic_uuid` helper, no tenant_id-column migration — correctly deferred per the reconciliation and spec §68-70.
- **Spec field values are exact.** `Tenant::new(Id::new("default"), "Default", "default", now)` with `budget = Some(BudgetConfig::default())` overriding the constructor's `None`; `oidc_issuer`/`max_workspaces` remain `None` (domain `tenant.rs:19-30` confirmed). The test asserts every field including the serialized budget round-trip through the SQLite `budget` JSON column, plus `created_at > 0`.
- **Idempotency is doubly enforced.** Early return on `find_by_id` hit, and — belt under that — the SQLite `create` is itself an upsert on `tenants::id` (`on_conflict ... do_update`), so even concurrent startups cannot produce a duplicate. The test's second call leaves exactly one `slug == "default"` row (checked via `list()`).
- **Test wiring mirrors production.** The test clones `mem::test_state()` and sets `base.tenants = Arc::new(storage)` over a fresh temp-file SQLite DB — byte-for-byte the same type/config `build_state`'s `store!` macro yields in DB-backed mode (`lib.rs:1007-1010`, `830-838`). It asserts `None` before bootstrap (fresh-DB precondition), so an unwired/removed bootstrap fails it.
- **Startup wiring is correct and ordered.** `main.rs` calls `bootstrap_default_tenant(&state).await` immediately after `build_state` and before `abac_middleware::seed_builtin_policies`, with the import added to the existing `gyre_server::{...}` list — exactly the plan.
- **Backend portability.** `PgStorage` also implements `TenantRepository` (`postgres/tenant.rs:57`), so in PostgreSQL mode `state.tenants` is the PG adapter and the bootstrap runs there too (the `store!` macro routes both).
- **Scope discipline.** Demo `/admin/seed` and `POST /api/v1/tenants` untouched (verified in diff); no workspace creation; convention match with sibling seed fns (async, `&Arc<AppState>`, `tracing::warn!` on error, early-return idempotency) confirmed against `seed_builtin_meta_specs`/`register_default_compute_target`.
