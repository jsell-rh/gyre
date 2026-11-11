---
title: "Implement tenant-scope meta-spec changes"
spec_ref: "meta-spec-reconciliation.md §8"
depends_on: [task-156]
progress: not-started
coverage_sections:
  - "meta-spec-reconciliation.md §8 Tenant-Scope Meta-Spec Changes"
commits: []
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

- [ ] MetaSpecScope includes Tenant variant
- [ ] Tenant-level meta-spec sets can be managed via API
- [ ] Workspace meta-spec sets inherit from tenant with override support
- [ ] Tenant meta-spec approval triggers per-workspace reconciliation
- [ ] Workspaces with overrides are skipped during tenant reconciliation
- [ ] Tenant admin force flag bypasses workspace rollout policies
- [ ] Tests cover inheritance, override, and force scenarios

## Agent Instructions

- Read `crates/gyre-domain/src/meta_spec.rs` for MetaSpecScope enum (currently Global, Workspace)
- Read `crates/gyre-server/src/api/meta_specs.rs` for existing meta-spec API (blast-radius endpoint at lines ~252-308)
- Read task-156 for the reconciliation controller this task extends
- The tenant meta-spec set is analogous to workspace meta-spec set — follow the same pattern in `gyre-ports/src/meta_spec_set.rs`
- Workspace inheritance: when resolving a workspace's effective meta-spec set, merge tenant defaults with workspace overrides (workspace wins on conflict)
