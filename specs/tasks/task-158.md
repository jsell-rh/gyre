---
title: "Implement tenant-scope meta-spec changes"
spec_ref: "meta-spec-reconciliation.md §8"
depends_on: [task-156]
progress: ready-for-review
coverage_sections:
  - "meta-spec-reconciliation.md §8 Tenant-Scope Meta-Spec Changes"
commits: ["3c22bfa874c70968192207603815adbddec5f6df"]
---

## Spec Excerpt

From `meta-spec-reconciliation.md` §8 — Tenant-Scope Meta-Spec Changes:

> When a tenant-level meta-spec changes, all workspaces in the tenant are affected. The reconciliation process:
> 1. Computes blast radius across all workspaces
> 2. Creates per-workspace reconciliation plans
> 3. Applies workspace rollout policies individually
> 4. Tenant admin can override workspace policies for urgent changes

## Implementation Plan

1. **Add Tenant scope to MetaSpecScope** (`crates/gyre-domain/src/meta_spec.rs`):
   - Currently MetaSpecScope has `Global` and `Workspace` — add `Tenant(Id)` variant
   - Update serialization and DB schema to support tenant-scoped meta-specs

2. **Add tenant-level meta-spec set binding**:
   - Add a `tenant_meta_spec_set` concept — tenant-wide defaults that workspaces inherit unless overridden
   - API: `GET/PUT /api/v1/tenants/{id}/meta-specs/set` — manage tenant-level meta-spec set
   - Workspace meta-spec sets inherit from tenant, with workspace-level overrides taking precedence

3. **Extend reconciliation controller** for tenant scope:
   - When a tenant-level meta-spec is approved, iterate all workspaces in the tenant
   - For each workspace, check if the workspace overrides this meta-spec — if so, skip
   - For non-overriding workspaces, apply the workspace's rollout policy to create reconciliation tasks
   - Compute cross-workspace blast radius using the existing blast-radius endpoint

4. **Tenant admin override**:
   - Add `force` flag on reconciliation trigger that ignores workspace-level rollout policies
   - Used for urgent compliance-driven changes that must be applied immediately

5. **Migration**:
   - Add `tenant_id` to relevant meta-spec tables if not already scoped
   - Add `tenant_meta_spec_sets` table or column

6. **Tests**:
   - Unit test: tenant-scoped meta-spec change creates workspace reconciliation tasks
   - Unit test: workspace-level override prevents tenant propagation
   - Unit test: force flag bypasses workspace rollout policy
   - Unit test: blast radius includes all non-overriding workspaces

## Acceptance Criteria

- [x] MetaSpecScope includes Tenant variant
- [x] Tenant-level meta-spec sets can be managed via API
- [x] Workspace meta-spec sets inherit from tenant with override support
- [x] Tenant meta-spec approval triggers per-workspace reconciliation
- [x] Workspaces with overrides are skipped during tenant reconciliation
- [x] Tenant admin force flag bypasses workspace rollout policies
- [x] Tests cover inheritance, override, and force scenarios

## Agent Instructions

- Read `crates/gyre-domain/src/meta_spec.rs` for MetaSpecScope enum (currently Global, Workspace)
- Read `crates/gyre-server/src/api/meta_specs.rs` for existing meta-spec API (blast-radius endpoint at lines ~252-308)
- Read task-156 for the reconciliation controller this task extends
- The tenant meta-spec set is analogous to workspace meta-spec set — follow the same pattern in `gyre-ports/src/meta_spec_set.rs`
- Workspace inheritance: when resolving a workspace's effective meta-spec set, merge tenant defaults with workspace overrides (workspace wins on conflict)

## Shipped

**Tenant scope on the registry (§8)** — `MetaSpecScope` gains a `Tenant`
variant (scope entity in `scope_id`, mirroring the Workspace design, so the
enum stays a plain string column). Migration `2026-11-07-000057` widens the
`meta_specs.scope` CHECK to `Global|Tenant|Workspace` (portable three-table
rebuild — children dropped before the parent, parents copied before
children — same pattern as 000054) and creates `tenant_meta_spec_sets`.
Creation validates that the scope tenant exists; `scope=Tenant&scope_id=`
listing requires caller-tenant containment.

**Tenant-level meta-spec set (§8 binding unit)** — new
`TenantMetaSpecSetRepository` port (get/upsert/delete by tenant id) with
mem, SQLite, and PostgreSQL adapters (separate table from workspace sets,
so a tenant and workspace sharing an id string never shadow each other).
API `GET/PUT /api/v1/tenants/{id}/meta-specs/set`: Admin-only plus tenant
containment (same rule as tenant trust anchors), registered in the ABAC
route resolver (resource `meta_spec`) with per-handler authorization.

**Inheritance with override support** —
`resolve_effective_meta_spec_set` merges tenant defaults with the
workspace's own overrides (workspace pin wins on conflict; persona keys
rebound by the workspace drop the tenant pin). `merge_meta_spec_sets` and
`workspace_overrides_path` are mutually consistent by construction: "not
overridden" always means "the effective set binds the path exactly as the
tenant set does". Every consumer resolves the effective set: the workspace
set GET (returns the merged view), `compute_meta_spec_set_sha` (the §9
constraint / §10 sweep input — for a workspace without a tenant set the
merged serialization is byte-identical to the stored JSON, so SHAs recorded
before this change stay valid, no false drift on upgrade), the blast-radius
endpoint (affected workspaces now include inheriting ones, reason
`tenant_inheritance` vs `workspace_binding`), the preview blast radius, and
the `meta_spec_changed` fan-out.

**Per-workspace reconciliation (§8 steps 1-3)** —
`run_tenant_reconciliation(tenant, changed_paths, force)` lists the
tenant's workspaces, skips workspaces that override a changed path (their
own rollout policy governs — they reconcile when their own binding
updates, per the §6 set-diff trigger), and runs a §6 reconciliation wave
per non-overriding workspace (task per repo, existing dedup/title rules).
Triggers: tenant-set PUT (path diff, removals are not triggers) and
approval of a Tenant-scoped registry entry (same
`approved_content_hash` trigger discipline as §6). Only paths the tenant
set pins propagate — an unbound change has an empty blast radius.

**Tenant admin force flag (§8 step 4)** — `force: true` on the tenant-set
PUT and on Tenant-scoped registry approval bypasses the workspace-level
override skip so urgent compliance changes apply immediately. Force is
only defined for Tenant scopes (400 otherwise) and only an admin of the
scope tenant may use it (403 cross-tenant).

**Spec lifecycle** — spec §8 amended to document the shipped binding-model
realization (tenant set + inheritance + override skip + force) and how
all-at-once/staged/selective rollouts map onto it, replacing the
never-shipped `POST /api/v1/tenant/meta-spec-rollout` sketch. Coverage
matrix row 12 → `implemented`; api-reference rows updated.

### Test evidence

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` — **1330 passed, 0
  failed** (was 1322 at base). New §8 tests in `reconciliation.rs`:
  tenant propagation creates per-workspace tasks; override prevents
  propagation; force bypasses the override; unbound paths do not
  propagate; blast radius includes all non-overriding workspaces (and
  reports the reason per repo, never crossing tenants); merge semantics
  (override wins, tenant defaults survive, extra workspace bindings kept);
  effective-set SHA reflects inheritance (empty without any set, tenant
  set does not leak across tenants, override changes the SHA); workspace
  GET returns the effective set; full API flow — Tenant-scoped spec
  creation → tenant-set bind (Pending bind creates nothing) → approval
  reconciles only the inheriting workspace → force approval reaches the
  overrider. `meta_specs.rs`: tenant-set GET/PUT roundtrip (persisted
  through the port), Admin-only, tenant containment, unknown tenant 404,
  Tenant-scope creation validation, force 400 on Global scope, force 403
  cross-tenant.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib` — **356 passed, 0
  failed**: tenant set repo roundtrip/overwrite/delete/independence
  (SQLite, fresh-migrated DB), Tenant-scoped meta-spec round-trip + scope
  filter (pins the widened CHECK).
- Mechanical checks all green (see
  `/tmp/stage/review-evidence/mechanical-checks.txt`): check-arch,
  check-abac-route-registry, check-abac-exempt-handlers (88 handlers),
  check-migration-versions, check-migration-sql-portability,
  check-mem-port-contracts, check-in-memory-state-stores,
  check-forged-scope-fields, check-scope-literal-defaults,
  check-dead-message-kinds, check-inert-enforcement,
  check-mcp-write-tools.
- Clippy: no warnings in any file touched by this change (pre-existing
  `never_loop` error in `rust_extractor.rs` is untouched base code).
