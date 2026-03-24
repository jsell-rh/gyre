# API Design Conventions

This spec defines the structural rules for Gyre's REST API surface. It is prescriptive: agents adding or modifying endpoints must follow these conventions. Violations are caught by `scripts/check-api-conventions.sh` and CI.

---

## 1. URL Structure

### 1.1 Hierarchy-Scoped Primary Routes

The API reflects the ownership hierarchy defined in `platform-model.md`:

```
Tenant → Workspace → Repository
```

**Collection endpoints for scoped entities use the parent as a URL prefix:**

```
GET  /api/v1/workspaces/:workspace_id/repos
GET  /api/v1/workspaces/:workspace_id/tasks
GET  /api/v1/workspaces/:workspace_id/agents
GET  /api/v1/workspaces/:workspace_id/merge-requests
GET  /api/v1/workspaces/:workspace_id/repos/:repo_id/merge-requests
```

These are the **primary access patterns**. Clients list entities within a governance boundary.

### 1.2 Flat Convenience Routes

Single-entity-by-ID access uses flat routes:

```
GET    /api/v1/tasks/:id
GET    /api/v1/agents/:id
GET    /api/v1/merge-requests/:id
PUT    /api/v1/tasks/:id
DELETE /api/v1/tasks/:id
```

These exist because once you have an entity's ID, the hierarchy is implicit (the entity knows its own `workspace_id`). ABAC still enforces that the caller can access the entity's workspace.

### 1.3 Admin Cross-Cutting Routes

Admin endpoints that span workspaces/tenants live under `/api/v1/admin/`:

```
GET  /api/v1/admin/tasks         (cross-workspace task view)
GET  /api/v1/admin/agents        (cross-workspace agent view)
POST /api/v1/admin/search/reindex
GET  /api/v1/admin/audit
```

All `/admin/` endpoints require the `AdminOnly` extractor. No exceptions.

### 1.4 Rule Summary

| Pattern | When to use | Auth |
|---|---|---|
| `/api/v1/workspaces/:ws_id/<entities>` | List/create within a workspace | ABAC: caller must be workspace member |
| `/api/v1/<entities>/:id` | Read/update/delete by ID | ABAC: caller must have access to entity's workspace |
| `/api/v1/<entities>/:id/<sub-resource>` | Sub-resources of an entity | ABAC: same as parent |
| `/api/v1/admin/<anything>` | Cross-cutting admin operations | AdminOnly extractor required |

---

## 2. Naming Rules

### 2.1 Resource Names

- **Always plural nouns:** `/tasks`, `/agents`, `/repos`, `/merge-requests`, `/personas`
- **Hyphenated for multi-word:** `/merge-requests`, `/merge-queue`, `/compute-targets`
- **Never verbs as resource names.** Use HTTP methods for CRUD semantics.

Justified exceptions for uncountable/singleton nouns:
- `/api/v1/activity` (uncountable)
- `/api/v1/version` (singleton)
- `/api/v1/search` (action resource)
- `/api/v1/budget/summary` (computed aggregate)

### 2.2 Action Endpoints

Non-CRUD operations that don't fit standard HTTP methods use a **verb sub-path on the parent resource:**

```
POST /api/v1/agents/spawn              (not: POST /api/v1/spawn-agent)
POST /api/v1/agents/:id/complete       (not: POST /api/v1/complete-agent/:id)
POST /api/v1/merge-queue/enqueue       (not: POST /api/v1/enqueue)
POST /api/v1/specs/:path/approve       (not: POST /api/v1/approve-spec)
```

Action endpoints are always `POST`, never `GET` or `PUT`.

### 2.3 Path Parameters

| Context | Convention | Example |
|---|---|---|
| Primary resource ID | `:id` | `/repos/:id` |
| Parent resource ID in nested routes | `:parent_id` with full entity name | `/workspaces/:workspace_id/repos` |
| Child resource ID in nested routes | `:child_id` with full entity name | `/repos/:repo_id/gates/:gate_id` |
| File paths (URL-encoded) | `:path` | `/specs/:path` |

**Never abbreviate:** `:worktree_id` not `:wt_id`, `:dependency_id` not `:dep_id`.

---

## 3. Request/Response Conventions

### 3.1 Collection Responses

All list endpoints return a JSON array at the top level:

```json
[{"id": "...", ...}, {"id": "...", ...}]
```

Not wrapped in `{"items": [...]}` or `{"data": [...]}`. Pagination metadata, if needed, uses response headers (`X-Total-Count`, `Link`).

### 3.2 Error Responses

All errors return:

```json
{"error": "human-readable message"}
```

With the appropriate HTTP status code. No error codes, no nested error objects.

### 3.3 Query Parameters

- **Filtering:** `?status=active&workspace_id=<uuid>` — exact match filters
- **Pagination:** `?limit=50&offset=0` — limit/offset model
- **Sorting:** `?sort=created_at&order=desc` — explicit field + direction
- **Temporal:** `?since=<epoch>&until=<epoch>` — Unix epoch seconds, not ISO strings

### 3.4 Status Codes

| Code | When |
|---|---|
| 200 | Successful read or update |
| 201 | Successful create (with `Location` header) |
| 202 | Accepted for async processing (e.g., idempotent re-complete) |
| 400 | Invalid input, validation failure, business rule violation |
| 401 | Missing or invalid authentication |
| 403 | Authenticated but insufficient permissions (RBAC/ABAC denial) |
| 404 | Resource not found |
| 409 | Conflict (e.g., duplicate name, state transition violation) |
| 429 | Rate limited or budget exceeded |

---

## 4. Sub-Resource Depth

**Maximum nesting depth: 2 levels** from the API version prefix.

```
OK:   /api/v1/repos/:id/gates/:gate_id
OK:   /api/v1/workspaces/:workspace_id/repos

BAD:  /api/v1/workspaces/:ws_id/repos/:repo_id/gates/:gate_id/results
```

If you need deeper access, use the flat convenience route for the intermediate entity, then access its sub-resources:

```
GET /api/v1/repos/:id/gates/:gate_id       (not nested under workspace)
```

---

## 5. Deprecation Protocol

When an endpoint is superseded:

1. Add a `Deprecation: true` response header and `Sunset: <date>` header
2. Log a warning on each call with the caller's identity
3. Document the replacement in the API reference
4. Remove after two milestone cycles (minimum 2 weeks)

Never leave two endpoints that do the same thing without a clear deprecation path.

---

## 6. Authorization Contract

Every endpoint handler must declare its authorization requirements via one of:

| Mechanism | When |
|---|---|
| No auth (public) | Health checks, OIDC discovery, version |
| Global auth middleware only | Endpoints transitioning to ABAC (must have a tracking issue) |
| ABAC middleware evaluation | Standard — subject/resource/action evaluated against policies |
| `AdminOnly` extractor | Admin endpoints (defense-in-depth alongside ABAC) |

**No handler that modifies state may rely solely on global auth middleware.** This is enforced by `scripts/check-api-auth.sh`.

---

## 7. Mechanical Enforcement

### `scripts/check-api-auth.sh`

Scans all handler function signatures registered in `api_router()`. Fails if any `POST`, `PUT`, or `DELETE` handler lacks an authorization extractor (`AdminOnly`, `RequireDeveloper`, `RequireAgent`, `RequireReadOnly`, or `AuthenticatedAgent` with inline ABAC check).

### `scripts/check-api-conventions.sh`

Scans route registrations for:
- Abbreviated path parameters (`:wt_id`, `:dep_id`)
- Admin-namespace endpoints missing `AdminOnly`
- Singular resource names where plural is required
- Nesting depth > 2

Both scripts run in CI and as pre-commit hooks.
