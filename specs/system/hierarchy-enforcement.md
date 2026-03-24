# Hierarchy Enforcement

> Amendment to `platform-model.md` and `abac-policy-engine.md`. This spec defines how the ownership hierarchy becomes load-bearing in code — domain types, authorization middleware, data isolation, and the Tenant entity.

## Problem

The platform model spec defines a clear `Tenant -> Workspace -> Repo` hierarchy with scoping rules, budget cascade, and token-based isolation. The implementation has:

- No `Tenant` entity in the domain layer (just a `tenant_id TEXT DEFAULT 'default'` column)
- `workspace_id: Option<Id>` on Task, Agent, MergeRequest, and Repository — meaning entities can float outside the hierarchy
- ABAC policy engine built as a standalone feature (`/api/v1/policies/`) rather than integrated into the request pipeline
- Inconsistent `tenant_id` filtering: some adapter methods filter, others don't
- No object-level authorization — RBAC checks role vertically but not resource ownership horizontally

The hierarchy is a spec concept, not a code concept. This spec makes it load-bearing.

---

## 1. Tenant as a Real Entity

### Domain Type

```rust
pub struct Tenant {
    pub id: Id,
    pub name: String,
    pub slug: String,
    pub oidc_issuer: Option<String>,
    pub budget: BudgetConfig,          // Non-optional — tenant is the absolute budget ceiling
    pub max_workspaces: Option<u32>,
    pub created_at: u64,              // Unix epoch seconds (consistent with all domain entities)
}
```

**Timestamp convention:** All `created_at` fields on domain entities (Tenant, Workspace, Repository, Task, Agent, MergeRequest) use **Unix epoch seconds**. The message bus `Message.created_at` uses **epoch milliseconds** for sub-second cursor ordering — this is a deliberate exception documented in `message-bus.md`. These two timestamp spaces should never be compared directly.

### Port Trait

```rust
#[async_trait]
pub trait TenantRepository: Send + Sync {
    async fn create(&self, tenant: &Tenant) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<Tenant>>;
    async fn find_by_slug(&self, slug: &str) -> Result<Option<Tenant>>;
    async fn list(&self) -> Result<Vec<Tenant>>;
    async fn update(&self, tenant: &Tenant) -> Result<()>;
}
```

### Bootstrap Behavior

On first startup (empty database), the server creates a default tenant:

```rust
Tenant {
    id: Id::new(deterministic_uuid("default-tenant")),
    name: "Default".to_string(),
    slug: "default".to_string(),
    oidc_issuer: None,
    budget: BudgetConfig::default(),   // No limits set — effectively unlimited
    max_workspaces: None,
    created_at: now(),
}
```

The existing `tenant_id TEXT DEFAULT 'default'` columns are migrated to reference this tenant's UUID. All existing rows get the default tenant's ID.

### API (Deferred)

Tenant CRUD endpoints (`/api/v1/tenants`) are not required for single-tenant deployments. They are defined in `platform-model.md` and will be implemented when multi-tenant support is prioritized. For now, the Tenant entity exists in domain, ports, and adapters, and is created at bootstrap.

---

## 2. Non-Optional Hierarchy Fields

### Domain Type Changes

These fields change from `Option<Id>` to `Id` in **`gyre-domain` structs** (the hierarchy lint only scans `crates/gyre-domain/src/`). Shared wire types in `gyre-common` (e.g., `Message`) may use `Option<Id>` where the domain requires it (e.g., Broadcast messages have no workspace):

| Entity | Field | Current | Target |
|---|---|---|---|
| Repository | `workspace_id` | `Option<Id>` | `Id` |
| Task | `workspace_id` | `Option<Id>` | `Id` |
| Task | `repo_id` | (not present) | `Id` — repo that owns the spec this task implements |
| Agent | `workspace_id` | `Option<Id>` | `Id` |
| MergeRequest | `workspace_id` | `Option<Id>` | `Id` |
| Workspace | `tenant_id` | `Id` | `Id` (already non-optional) |

**Task scoping note:** Tasks must reference a spec (`spec_ref`), and specs live in repos. Therefore tasks are **repo-scoped** — every task has a `repo_id: Id` (non-optional) pointing to the repo that owns the spec it implements. Tasks also carry `workspace_id: Id` (non-optional) for authorization — this is the governance boundary used by ABAC. The workspace_id is derivable from the repo's workspace, but stored denormalized for efficient queries. This is consistent with `platform-model.md` §1 which says "Task → Repository."

### Migration Strategy

1. Create default tenant (if not exists)
2. Create default workspace under default tenant (if not exists)
3. Backfill: `UPDATE repos SET workspace_id = '<default-ws-id>' WHERE workspace_id IS NULL`
4. Same for tasks, agents, merge_requests
5. Alter columns to `NOT NULL`

### Invariant Enforcement

The domain types encode the invariant at the type level — `workspace_id: Id` means you cannot construct a Task without a workspace. The compiler enforces this.

Additionally, `scripts/check-hierarchy.sh` scans domain struct definitions for `Option<Id>` on hierarchy fields and fails if any are found. The script is gated by `GYRE_CHECK_HIERARCHY=1` env var — initially disabled, enabled in M34 Slice 3 after the non-optional migration lands (see `m34-hierarchy-enforcement.md`):

```bash
# Fields that MUST be non-optional
REQUIRED_FIELDS=(
    "workspace_id"  # on Task, Agent, MergeRequest, Repository
    "tenant_id"     # on Workspace
)
```

---

## 3. Consistent Tenant Filtering

### The Problem

The adapter layer (`SqliteStorage`) has a `tenant_id: String` field set once at construction. Some query methods filter by it, others don't:

| Method | Filters tenant_id? |
|---|---|
| `TaskRepository::list()` | Yes |
| `AgentRepository::list()` | Yes |
| `MergeRequestRepository::list()` | **No** |
| `MergeRequestRepository::list_by_status()` | **No** |
| `MergeRequestRepository::find_by_id()` | **No** |
| `RepoRepository::list()` | **No** |
| `RepoRepository::list_by_project()` | **No** |

### The Fix

**Every query method on every Diesel adapter must filter by `tenant_id`.** Exception: adapters that enforce tenant isolation structurally (e.g., `MessageRepository` queries by globally-unique `workspace_id`, which is tenant-bound) may be exempted from the lint with a documented rationale in `check-tenant-filter.sh`'s skip list.

This includes `find_by_id()` — looking up an entity by UUID must still verify it belongs to the current tenant. This prevents horizontal privilege escalation where a user with a valid token guesses another tenant's entity UUIDs.

**Per-request tenant scoping:** The current `SqliteStorage` has a `tenant_id` baked in at construction time. For multi-tenant support, the adapter layer must accept tenant context per-request rather than per-instance. Two approaches:
- **Option A (recommended for greenfield):** Pass `tenant_id` as a parameter to every repository method. This is explicit and the compiler enforces that every call site provides a tenant.
- **Option B:** Use `SqliteStorage::with_tenant(id)` to create a tenant-scoped view per-request (the method already exists). The ABAC middleware resolves `tenant_id` from the JWT and stores a tenant-scoped adapter in request extensions.

For the single-tenant bootstrap (M34), Option A is not yet needed — the default tenant is the only tenant and the construction-time `tenant_id` suffices. Document Option A as the multi-tenant path.

### Enforcement

`scripts/check-tenant-filter.sh` scans all Diesel query methods in `crates/gyre-adapters/src/sqlite/` and `crates/gyre-adapters/src/pg/` for the pattern `.filter(table::tenant_id.eq(`. Any query method that builds a Diesel query without this filter fails the check.

**KvJsonStore gap:** The generic `KvJsonStore` (used for agent_tokens, compute_targets, abac_policies, etc.) uses namespace+key lookups with no `tenant_id` column. These stores are not covered by the Diesel lint script. `agent_messages` is replaced by the unified message bus (`message-bus.md`). The remaining KvJsonStore namespaces should be migrated to proper port traits with tenant-scoped adapters when multi-tenant support is prioritized — tracked as a backlog item, not part of M34.

Additionally, an integration test (`tests/tenant_isolation.rs`) creates two tenants, populates entities in both, and verifies that listing/finding through one tenant's storage never returns the other tenant's entities.

---

## 4. ABAC as Request Infrastructure

### Current State

The ABAC policy engine exists as a standalone feature:
- `POST /api/v1/policies` — CRUD on policy definitions
- `POST /api/v1/policies/evaluate` — dry-run evaluation
- `GET /api/v1/policies/decisions` — audit log

But only three code paths actually evaluate ABAC: git push, agent spawn, and worktree creation. The rest of the API uses ad-hoc RBAC extractors (`AdminOnly`, `RequireDeveloper`) or nothing at all.

### Target State

ABAC evaluation runs on **every authenticated request** as axum middleware, after authentication and before the handler.

```
Request
  |
  v
rate_limit_middleware        (existing)
  |
  v
request_tracing              (existing)
  |
  v
require_auth_middleware      (existing — identifies the caller)
  |
  v
abac_middleware              (NEW — evaluates access policy)
  |
  v
Handler
```

### Middleware Design

#### Subject Resolution

The `require_auth_middleware` already validates the token. The ABAC middleware extracts subject attributes from the authenticated context:

| Attribute | Source |
|---|---|
| `subject.type` | Token kind: `user`, `agent`, `system` |
| `subject.id` | User ID or agent ID from token |
| `subject.tenant_id` | From JWT claim, or default tenant for global tokens |
| `subject.global_role` | User's role: Admin, Developer, Agent, ReadOnly |
| `subject.workspace_ids` | User's workspace memberships (cached per-request) |
| `subject.workspace_role` | Role in the target resource's workspace |
| `subject.persona` | Agent's persona (from JWT claim) |
| `subject.attestation_level` | Agent's attestation level (from JWT claim) |

System tokens (global `GYRE_AUTH_TOKEN`) bypass ABAC entirely — they are the superuser escape hatch for bootstrap and emergency access.

#### Resource Resolution

The middleware extracts the resource context from the request path:

1. **Parse the route pattern** to identify the resource type and ID. The router provides matched route information via axum's `MatchedPath` extractor.
2. **Look up the entity** if an ID is present, to get its `workspace_id` and `tenant_id`.
3. **For collection endpoints** (e.g., `GET /api/v1/workspaces/:ws_id/tasks`), the workspace ID from the path is the resource scope.

Resource resolution uses a registry that maps route patterns to resource types:

```rust
struct ResourceResolver {
    /// Maps axum route patterns to resource type + ID extraction.
    routes: Vec<RouteResourceMapping>,
}

struct RouteResourceMapping {
    /// e.g., "/api/v1/tasks/:id"
    pattern: &'static str,
    /// e.g., "task"
    resource_type: &'static str,
    /// Which path param holds the resource ID (if any)
    id_param: Option<&'static str>,
    /// Which path param holds the parent workspace ID (if any)
    workspace_param: Option<&'static str>,
}
```

For routes with an entity ID (`:id`), the middleware does a single lookup to get the entity's `workspace_id`. This lookup is cached in the request extensions so the handler doesn't repeat it.

For routes without an entity ID (collection/create endpoints), the middleware resolves workspace context from the URL path (e.g., `POST /api/v1/workspaces/:workspace_id/tasks`). Per `api-conventions.md` §1.1, create endpoints use the workspace-scoped route form — there are no flat `POST /api/v1/tasks` create routes. Admin-only endpoints (no workspace context) fall back to tenant-level scoping.

#### Action Resolution

The HTTP method maps to an ABAC action:

| Method | Action |
|---|---|
| GET | `read` |
| POST | `write` (or specific action for action endpoints, e.g., `approve`, `spawn`) |
| PUT | `write` |
| DELETE | `delete` |

Action endpoints (e.g., `POST /agents/spawn`, `POST /specs/:path/approve`) have explicit action mappings in the route registry.

#### Evaluation

With subject, resource, and action resolved, the middleware calls the existing `PolicyRepository` evaluation logic. The evaluation:

1. Collects applicable policies (repo-scoped, workspace-scoped, tenant-scoped)
2. Sorts by priority (highest first)
3. Evaluates conditions against the attribute context
4. Returns Allow or Deny on first match
5. Logs the decision to the audit trail

On Deny: returns `403 {"error": "insufficient permissions"}`. The matched policy name is logged server-side at `warn` level for debugging but is NOT included in the response (per `api-conventions.md` §3.2 — no nested error objects, and policy names are internal implementation details).
On Allow: request proceeds to handler.

#### Performance

- **Policy cache:** Policies are cached in memory, invalidated on create/update/delete. Not re-fetched from DB per request.
- **Membership cache:** User's workspace memberships are resolved once per request and stored in request extensions.
- **Entity lookup:** Single DB query per request (for ID-based routes). Cached in request extensions for handler reuse.
- **Target: <1ms** additional latency per request (policy evaluation is in-memory matching).

### Built-In Policies

ABAC ships with built-in policies seeded at startup (cannot be deleted):

| Policy | Effect | Purpose |
|---|---|---|
| `system-full-access` | Allow | Global `GYRE_AUTH_TOKEN` gets full access |
| `admin-all-operations` | Allow | Admin role allows all actions |
| `developer-write-access` | Allow | Developer role allows read + write |
| `agent-scoped-access` | Allow | Agent role allows read + write in scoped repo |
| `readonly-get-only` | Allow | ReadOnly role allows only read |
| `tenant-isolation` | Deny | Cross-tenant access denied |
| `workspace-membership-required` | Deny | Non-members denied access to workspace resources |
| `default-deny` | Deny | Everything not explicitly allowed is denied (lowest priority) |

These replace the current RBAC extractors (`AdminOnly`, `RequireDeveloper`, etc.). Since this is greenfield, ABAC is the authorization layer from the start — there is no RBAC-to-ABAC migration. The old extractors are removed entirely.

Admins can create custom policies via `POST /api/v1/policies` that layer on top of built-in ones.

### Endpoints That Bypass ABAC

These endpoints are exempt from ABAC evaluation (handled before the middleware):

| Endpoint | Reason |
|---|---|
| `GET /health`, `/healthz`, `/readyz` | Infrastructure probes |
| `GET /metrics` | Prometheus scraping |
| `GET /.well-known/*` | OIDC discovery |
| `GET /api/v1/version` | Public info |
| `GET /ws` | WebSocket (auth in handshake) |
| `GET /ws/agents/:id/tty` | WebSocket (auth in first message) |
| `/git/*` | Git smart HTTP (per-handler auth + ABAC, existing) |
| `/mcp`, `/mcp/sse` | MCP (per-handler auth + ABAC, existing) |
| `/scim/v2/*` | SCIM (separate `GYRE_SCIM_TOKEN` auth) |
| `GET /*` | SPA static files |

---

## 5. Legacy Cleanup

### Duplicate Spec Approval

`POST /api/v1/specs/approve` (in `gates.rs`) is superseded by `POST /api/v1/specs/:path/approve` (in `specs.rs`). The legacy endpoint:

- Has no RBAC check (any authenticated user can approve)
- Does not enforce spec linkage validation
- Does not use the spec ledger state machine

**Action:** Remove `POST /api/v1/specs/approve` and `POST /api/v1/specs/revoke` from `gates.rs`. Redirect callers to the path-scoped variants.

### Audit/Analytics/Cost Recording

`POST /api/v1/analytics/events`, `POST /api/v1/costs`, and `POST /api/v1/audit/events` currently accept any authenticated user. These should:

- Require `subject.type == agent` for recording (agents record their own telemetry)
- Validate that the `agent_id` in the payload matches `subject.id` (agents can only record their own events)
- Allow Admin to record on behalf of any agent (for backfill/correction)

This is naturally expressed as ABAC policies, not hardcoded logic.

### In-Memory meta_spec_sets

`meta_spec_sets: Arc<Mutex<HashMap<...>>>` in `AppState` must be replaced with a proper `MetaSpecSetRepository` port trait and adapter, matching the persistence model of all other domain entities.

---

## 6. Git URL Alignment

When M33 (Remove Project) lands, git URLs change. The target format uses workspace slug + repo name for human-readable remotes:

```
/git/:workspace_slug/:repo_name/info/refs
/git/:workspace_slug/:repo_name/git-upload-pack
/git/:workspace_slug/:repo_name/git-receive-pack
```

```bash
git clone http://localhost:3000/git/platform/gyre-server
```

**Prerequisites:** Workspace `slug` must be unique within a tenant (enforced by a unique constraint on `(tenant_id, slug)` added in the Slice 2 migration). The server resolves `workspace_slug` + `repo_name` to the repo entity, validating uniqueness. ABAC validates the caller has access to the workspace.

**Replaces:** The current `/git/:project/:repo/*` routes (or `/git/:repo_id/:repo/*` after M33). Old URLs are not supported — this is a greenfield change.

---

## 7. Mechanical Enforcement

### New Scripts

| Script | What it checks | Run by |
|---|---|---|
| `scripts/check-api-auth.sh` | Every route (GET/POST/PUT/DELETE) has a `RouteResourceMapping` in the ABAC `ResourceResolver` — workspace isolation requires resource resolution on reads too | Pre-commit, CI |
| `scripts/check-tenant-filter.sh` | Every Diesel query method in adapters filters by `tenant_id` | Pre-commit, CI |
| `scripts/check-hierarchy.sh` | Domain structs don't use `Option<Id>` for hierarchy fields | Pre-commit, CI |

### New Integration Tests

| Test | What it verifies |
|---|---|
| `tests/tenant_isolation.rs` | Two-tenant scenario: listing/finding through tenant A never leaks tenant B data |
| `tests/abac_middleware.rs` | ABAC middleware denies access when policy conditions aren't met; allows when they are |
| `tests/workspace_scoping.rs` | Entities created in workspace A are not visible in workspace B's scoped routes |
| `tests/hierarchy_cascade.rs` | Budget cascade: workspace budget can't exceed tenant, repo can't exceed workspace |

---

## Relationship to Existing Specs

- **Supersedes** the authorization sections of `platform-model.md` §1 (Scoping Rules, Token Scoping) — this spec makes them concrete. Also adds `repo_id: Id` to Task (not in platform-model's struct but consistent with its "Task → Repository" scoping rule).
- **Amends** `abac-policy-engine.md` — adds §"Request Pipeline Integration" (the middleware design)
- **Amends** `identity-security.md` — RBAC extractors are a migration step toward ABAC, not the permanent architecture
- **Implements** the built-in policies listed in `abac-policy-engine.md` §"Built-In Policies"
- **Depends on** `api-conventions.md` for URL structure conventions
- **Requires amendment to** `platform-model.md` — several endpoints defined there (personas, secrets) use flat routes that don't conform to `api-conventions.md` §1.1. When implemented, these must be workspace-scoped (e.g., `POST /api/v1/workspaces/:ws_id/personas`) for ABAC resource resolution.
- **Informed by** the API audit findings (C1–C3, H1–H4) documented in the milestone spec
