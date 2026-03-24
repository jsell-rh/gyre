# M34: Hierarchy Enforcement

**Goal:** Make the ownership hierarchy (Tenant -> Workspace -> Repo) load-bearing in code. Fix tenant isolation, wire ABAC into the request pipeline, remove authorization gaps, align URL structure.

**Specs:** `system/hierarchy-enforcement.md`, `development/api-conventions.md`

**Audit findings addressed:** C1 (tenant isolation), C2 (missing RBAC), C3 (duplicate spec approval), H1 (domain-schema mismatch), H2 (optional workspace_id), H3 (unprotected recording), H4 (no object-level authz), M1-M6 (structural/naming).

---

## Sequencing

Six vertical slices, each independently shippable and testable. Earlier slices create enforcement guardrails that protect later work.

### Slice 1: Enforcement Scripts (guardrails first)

**What:** Add three architecture lint scripts that catch hierarchy and authorization violations. These run in CI and pre-commit, giving agents immediate feedback.

**Deliverables:**
- `scripts/check-api-auth.sh` — scans `api_router()` route registrations; for every `post()`, `put()`, `delete()` handler, greps the handler's function signature for an authorization extractor (`AdminOnly`, `RequireDeveloper`, `RequireAgent`, `RequireReadOnly`, or `AuthenticatedAgent`). Fails with remediation instructions if any are missing.
- `scripts/check-tenant-filter.sh` — scans all `impl ... for SqliteStorage` and `impl ... for PgStorage` query methods; verifies that every method building a Diesel `QueryDsl` chain includes `.filter(<table>::tenant_id.eq(`. Fails with the method name and file location.
- `scripts/check-hierarchy.sh` — scans domain struct definitions in `crates/gyre-domain/src/` for the fields `workspace_id` and `tenant_id`; fails if any are declared as `Option<Id>` instead of `Id`. (Note: this script will fail immediately against current code — that's expected. It documents the target state. Disable it until Slice 3 completes, then enable.)
- Add all three to `.pre-commit-config.yaml` (check-hierarchy disabled initially).
- Add all three to CI workflow.

**Tests:** The scripts themselves are the tests. Verify they catch known violations in current code.

**Why first:** These scripts protect every subsequent slice. An agent working on Slice 4 who forgets an auth extractor gets immediate feedback, not a security review finding months later.

### Slice 2: Tenant Entity

**What:** Make Tenant a real domain entity with a default instance created at bootstrap.

**Deliverables:**
- `crates/gyre-domain/src/tenant.rs` — `Tenant` struct with `id`, `name`, `slug`, `oidc_issuer`, `budget`, `max_workspaces`, `created_at`
- `crates/gyre-ports/src/tenant.rs` — `TenantRepository` trait: `create`, `find_by_id`, `find_by_slug`, `list`, `update`
- `crates/gyre-adapters/src/sqlite/tenant.rs` — SQLite implementation
- `crates/gyre-adapters/src/pg/tenant.rs` — PostgreSQL implementation
- `crates/gyre-adapters/src/mem/tenant.rs` — In-memory implementation (for tests)
- Diesel migration: `CREATE TABLE tenants (id TEXT PRIMARY KEY, name TEXT NOT NULL, slug TEXT NOT NULL UNIQUE, oidc_issuer TEXT, budget_json TEXT, max_workspaces INTEGER, created_at INTEGER NOT NULL)`
- Diesel migration: `INSERT INTO tenants (id, name, slug, created_at) VALUES ('<deterministic-uuid>', 'Default', 'default', <epoch>)` — idempotent
- Diesel migration: Backfill existing `tenant_id = 'default'` columns to reference the new tenant's UUID
- `AppState` gains `tenants: Arc<dyn TenantRepository>`
- Server startup: ensure default tenant exists (idempotent)
- Unit tests for all adapter implementations

**Does NOT include:** Tenant CRUD API endpoints. Those come when multi-tenant is prioritized.

### Slice 3: Non-Optional Hierarchy Fields

**What:** Make `workspace_id` non-optional on Task, Agent, MergeRequest, Repository.

**Deliverables:**
- Diesel migration: create default workspace under default tenant (idempotent), backfill `NULL` workspace_id rows, alter columns to `NOT NULL`
- Domain type changes: `workspace_id: Id` (not `Option<Id>`) on `Task`, `Agent`, `MergeRequest`, `Repository`
- Port trait changes: update all method signatures that construct or return these types
- Adapter changes: update Diesel mappings, remove `Option` handling
- Handler changes: update all API handlers that create these entities to require `workspace_id` in the request body (400 if missing)
- Enable `scripts/check-hierarchy.sh` in pre-commit and CI
- Update all existing tests that construct these entities without a workspace_id

**Migration safety:** The backfill migration runs before the column constraint change. If any row has `workspace_id IS NULL` and there's no default workspace, the migration fails loudly rather than silently corrupting data.

### Slice 4: Fix Authorization Gaps

**What:** Add RBAC extractors to all unprotected mutating endpoints. Fix tenant filtering in adapters. Remove duplicate spec approval.

**Deliverables:**

Authorization extractors added:
| Endpoint | Extractor |
|---|---|
| `POST /api/v1/projects` | `RequireDeveloper` |
| `PUT /api/v1/projects/:id` | `RequireDeveloper` |
| `DELETE /api/v1/projects/:id` | `AdminOnly` |
| `POST /api/v1/repos` | `RequireDeveloper` |
| `POST /api/v1/tasks` | `RequireDeveloper` |
| `PUT /api/v1/tasks/:id` | `RequireDeveloper` |
| `POST /api/v1/agents` | `RequireAgent` |
| `PUT /api/v1/agents/:id/status` | `RequireAgent` |
| `POST /api/v1/merge-requests` | `RequireDeveloper` |
| `POST /api/v1/agents/spawn` | `RequireDeveloper` |
| `POST /api/v1/analytics/events` | `RequireAgent` |
| `POST /api/v1/costs` | `RequireAgent` |
| `POST /api/v1/audit/events` | `RequireAgent` |

Tenant filtering fixed:
- `MergeRequestRepository`: add `tenant_id` filter to `list()`, `list_by_status()`, `list_by_repo()`, `find_by_id()`
- `RepoRepository`: add `tenant_id` filter to `list()`, `list_by_project()`
- Verify all other repository implementations filter consistently

Legacy cleanup:
- Remove `POST /api/v1/specs/approve` from `gates.rs` (or redirect to path-scoped variant)
- Remove `POST /api/v1/specs/revoke` from `gates.rs`
- Move `POST /api/v1/search/reindex` to `POST /api/v1/admin/search/reindex`

Persistence fix:
- Create `MetaSpecSetRepository` port trait
- Implement in SQLite/PG/memory adapters
- Replace `meta_spec_sets: Arc<Mutex<HashMap<...>>>` in `AppState`

**Tests:**
- `scripts/check-api-auth.sh` passes (all mutating handlers have extractors)
- `scripts/check-tenant-filter.sh` passes (all adapter queries filter tenant)
- Integration test: `tests/tenant_isolation.rs` — two-tenant data isolation
- Integration test: `tests/authorization_coverage.rs` — verify each newly-protected endpoint returns 403 for unauthorized callers

### Slice 5: ABAC Middleware

**What:** Wire ABAC evaluation into the axum middleware stack. Deploy built-in policies. Verify behavior matches existing RBAC.

**Deliverables:**
- `crates/gyre-server/src/abac_middleware.rs` — axum middleware that:
  1. Extracts subject attributes from auth context
  2. Resolves resource type and workspace from route pattern + path params
  3. Resolves action from HTTP method + action registry
  4. Evaluates ABAC policies
  5. Returns 403 on Deny, proceeds on Allow
  6. Logs decision to audit trail
- `ResourceResolver` struct: registry mapping route patterns to resource types
- Built-in policies (seeded at startup, cannot be deleted):
  - `system-full-access` — system tokens bypass ABAC
  - `admin-all-operations` — Admin role allows all actions
  - `developer-write-access` — Developer role allows read + write
  - `agent-scoped-access` — Agent role allows read + write in scoped repo
  - `readonly-get-only` — ReadOnly role allows only read
  - `tenant-isolation` — deny cross-tenant access
  - `workspace-membership-required` — deny access to workspaces the user isn't a member of
  - `default-deny` — deny everything not explicitly allowed (lowest priority)
- Policy cache: in-memory cache invalidated on policy mutation
- Membership cache: per-request cache of user's workspace memberships
- Entity lookup cache: per-request cache of resolved entity (avoids double-lookup in handler)

**Tests:**
- `tests/abac_middleware.rs` — unit tests for middleware with mock policies
- `tests/abac_builtin_policies.rs` — integration tests verifying built-in policies replicate current RBAC behavior exactly
- `tests/workspace_scoping.rs` — entities in workspace A invisible from workspace B

**Important:** RBAC extractors remain during this slice as defense-in-depth. They are not removed until Slice 5 is proven in CI.

### Slice 6: URL Restructure

**What:** Add hierarchy-scoped routes as primary access patterns. Deprecate (don't remove) flat collection routes. Standardize parameter naming.

**Deliverables:**

New routes (additions, not replacements):
```
GET  /api/v1/workspaces/:workspace_id/tasks
GET  /api/v1/workspaces/:workspace_id/agents
GET  /api/v1/workspaces/:workspace_id/repos/:repo_id/merge-requests
GET  /api/v1/workspaces/:workspace_id/merge-requests
```

Parameter renaming (in existing routes):
| Current | Target |
|---|---|
| `:wt_id` | `:worktree_id` |
| `:dep_id` | `:dependency_id` |

Existing flat routes (`/api/v1/tasks`, `/api/v1/agents`, `/api/v1/merge-requests`) remain but add `Deprecation: true` response header. They will be moved to `/api/v1/admin/` in a future milestone.

Git URL alignment (if M33 has landed):
```
/git/:workspace_slug/:repo_name/*   (new, primary)
/git/:repo_id/:repo_name/*          (existing, deprecated)
```

**Tests:**
- `scripts/check-api-conventions.sh` passes
- Integration tests for new scoped routes return the same data as flat routes (when called with the correct workspace)
- Integration tests for parameter renaming (old names return 404 or redirect)

---

## Acceptance Criteria

| Criterion | Verified by |
|---|---|
| `Tenant` exists as domain entity with default instance | Unit tests, bootstrap smoke test |
| `workspace_id` is `Id` (not `Option<Id>`) on Task, Agent, MR, Repo | `scripts/check-hierarchy.sh`, compiler |
| Every mutating endpoint has an auth extractor | `scripts/check-api-auth.sh` |
| Every adapter query filters by `tenant_id` | `scripts/check-tenant-filter.sh` |
| Two-tenant data isolation holds | `tests/tenant_isolation.rs` |
| ABAC middleware evaluates on every request | `tests/abac_middleware.rs` |
| Built-in policies replicate RBAC behavior | `tests/abac_builtin_policies.rs` |
| Workspace-scoped routes exist | Integration tests |
| Legacy spec approval endpoints removed | Route scan |
| `meta_spec_sets` persisted to DB | Restart test |
| No `Option<Id>` on hierarchy fields | `scripts/check-hierarchy.sh` |

---

## Dependencies

- **M33 (Remove Project):** Slice 6 git URL changes depend on M33 landing first. All other slices are independent.
- **Existing specs:** `platform-model.md`, `abac-policy-engine.md`, `identity-security.md` — this milestone implements what they define.

## Risk

- **Slice 3 (non-optional workspace_id)** is the highest-risk migration. It touches every entity type and every test. The backfill migration must be tested against a copy of production data before running.
- **Slice 5 (ABAC middleware)** changes the authorization model for every request. Defense-in-depth (keeping RBAC extractors) mitigates regression risk during rollout.
