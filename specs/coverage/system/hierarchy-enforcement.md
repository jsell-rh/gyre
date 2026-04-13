# Coverage: Hierarchy Enforcement

**Spec:** [`system/hierarchy-enforcement.md`](../../system/hierarchy-enforcement.md)
**Last audited:** 2026-04-13 (full audit — bulk reclassification from not-started)
**Coverage:** 13/18 (11 n/a)

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Problem | 2 | n/a | - | Context/rationale — no implementable requirement. |
| 2 | 1. Tenant as a Real Entity | 2 | n/a | - | Section heading only — no implementable requirement. |
| 3 | Domain Type | 3 | implemented | - | Tenant struct in gyre-domain/src/tenant.rs with all spec fields (id, name, slug, oidc_issuer, budget, max_workspaces, created_at). `budget` is `Option<BudgetConfig>` — spec says non-optional; minor gap tracked in task-160. |
| 4 | Port Trait | 3 | implemented | - | TenantRepository in gyre-ports/src/tenant.rs with create, find_by_id, find_by_slug, list, update, delete. Matches spec plus delete (bonus). |
| 5 | Bootstrap Behavior | 3 | implemented | - | Default tenant with id "default" used throughout (lib.rs, mcp.rs, workspaces.rs). SqliteStorage constructed with tenant_id. |
| 6 | API (Deferred) | 3 | n/a | - | Explicitly deferred — "not required for single-tenant deployments." Tenant CRUD endpoints exist (api/tenants.rs) as bonus implementation. |
| 7 | 2. Non-Optional Hierarchy Fields | 2 | n/a | - | Section heading only — no implementable requirement. |
| 8 | Domain Type Changes | 3 | implemented | - | All spec-required fields are non-optional: Repository.workspace_id: Id, Task.workspace_id: Id, Task.repo_id: Id, Agent.workspace_id: Id, MergeRequest.workspace_id: Id. Comments reference "M34 hierarchy enforcement." |
| 9 | Migration Strategy | 3 | implemented | - | Fields are NOT NULL in DB schema. Existing data backfilled to default workspace/tenant. |
| 10 | Invariant Enforcement | 3 | task-assigned | task-160 | check-hierarchy.sh script does not exist yet. Spec requires it to scan domain structs for Option<Id> on hierarchy fields. |
| 11 | 3. Consistent Tenant Filtering | 2 | n/a | - | Section heading only — no implementable requirement. |
| 12 | The Problem | 3 | n/a | - | Context describing the gap — no implementable requirement. |
| 13 | The Fix | 3 | implemented | - | 60 occurrences of tenant_id filtering across 13 adapter files. SqliteStorage has tenant_id field used in queries. Substantial coverage. |
| 14 | Enforcement | 3 | task-assigned | task-160 | check-tenant-filter.sh script does not exist yet. Spec requires it to scan Diesel query methods for tenant_id filter pattern. tenant_isolation.rs integration test also not yet standalone. |
| 15 | 4. ABAC as Request Infrastructure | 2 | n/a | - | Section heading only — no implementable requirement. |
| 16 | Current State | 3 | n/a | - | Describes pre-existing state — no implementable requirement. |
| 17 | Target State | 3 | implemented | - | ABAC middleware exists at crates/gyre-server/src/abac_middleware.rs. Runs after require_auth_middleware, before handlers. Pipeline position matches spec exactly. |
| 18 | Middleware Design | 3 | implemented | - | RouteResourceMapping struct with pattern, resource_type, action_override, exempt fields. ResourceResolver with route registry. Subject resolution from AuthenticatedAgent. Action resolution from HTTP method. Full evaluation pipeline. |
| 19 | Built-In Policies | 3 | implemented | - | builtin_policies() in gyre-domain/src/policy.rs seeds policies at startup. system-full-access, admin-all-operations, default-deny and others present. Matches spec's policy table. |
| 20 | Endpoints That Bypass ABAC | 3 | implemented | - | RouteResourceMapping::exempt() used for /api/v1/version, notifications, SCIM, trace-spans, and others. Matches spec's bypass list. |
| 21 | 5. Legacy Cleanup | 2 | n/a | - | Section heading only — no implementable requirement. |
| 22 | Duplicate Spec Approval | 3 | implemented | - | POST /api/v1/specs/approve and /revoke removed. Comment in mod.rs:337: "removed in M34 Slice 5." |
| 23 | Audit/Analytics/Cost Recording | 3 | task-assigned | task-161 | Need to verify: agents must only record their own telemetry (subject.id matches agent_id in payload), admin can record on behalf. Currently expressed as ABAC policies but completeness unverified. |
| 24 | In-Memory meta_spec_sets | 3 | implemented | - | MetaSpecSetRepository port trait exists (gyre-ports/src/meta_spec_set.rs). SQLite and Postgres adapters exist. No longer in-memory HashMap. |
| 25 | 6. Git URL Alignment | 2 | implemented | - | Git routes use /git/:workspace_slug/:repo_name/* format (lib.rs:667-675). git_http.rs resolves workspace_slug + repo_name to repo entity. Tests confirm format (git_http.rs:3413+). |
| 26 | 7. Mechanical Enforcement | 2 | n/a | - | Section heading only — no implementable requirement. |
| 27 | New Scripts | 3 | task-assigned | task-160 | check-api-auth.sh, check-tenant-filter.sh, check-hierarchy.sh do not exist. Spec requires all three for CI enforcement. |
| 28 | New Integration Tests | 3 | task-assigned | task-161 | No dedicated tenant_isolation.rs, abac_middleware.rs, workspace_scoping.rs, hierarchy_cascade.rs test files. Some ABAC/tenant tests exist inline in other test files. |
| 29 | Relationship to Existing Specs | 2 | n/a | - | Cross-reference section — no implementable requirement. |
