---
title: "Platform Model Secrets Admin API + MCP + CLI"
spec_ref: "platform-model.md §7 Admin API"
depends_on:
  - task-097
progress: not-started
coverage_sections:
  - "platform-model.md §7 MCP Integration"
  - "platform-model.md §7 Admin API"
commits: []
---

## Spec Excerpt

### MCP Integration

Agents don't fetch secrets via MCP tools. Secrets are environment variables. The MCP server can report what secret names are available (not values):

| Resource | Purpose |
|---|---|
| `secrets://names` | List of secret names available to this agent (not values) |
| `secrets://status` | Expiry status of rotated secrets |

### Admin API

| Endpoint | Method | Purpose |
|---|---|---|
| `POST /api/v1/secrets` | POST | Create/update secret (admin only) |
| `GET /api/v1/secrets` | GET | List secret names by scope (not values) |
| `DELETE /api/v1/secrets/{id}` | DELETE | Delete secret |
| `POST /api/v1/secrets/{id}/rotate` | POST | Force rotation |

**CLI:** `gyre secret set --repo DATABASE_URL "postgres://..."`, `gyre secret list`, `gyre secret rotate`

**UI:** Secrets management in admin panel. Shows secret names, scopes, last rotated, expiry status. Never shows values.

## Implementation Plan

1. **Admin API endpoints (gyre-server):**
   - `POST /api/v1/secrets` — create a secret. Body: `{name, scope, scope_id, secret_type, value}`. Admin role required. Value is accepted in request but never returned in responses.
   - `GET /api/v1/secrets?scope=workspace&scope_id=ws-1` — list secret metadata (name, scope, type, created_at, expires_at). Never returns values.
   - `DELETE /api/v1/secrets/:id` — delete a secret. Admin role required.
   - `POST /api/v1/secrets/:id/rotate` — force rotate. Body: `{new_value}`. Updates value and last_rotated_at.
   - All endpoints scoped by tenant_id from auth context.

2. **ABAC resource mapping:**
   - Add `secret` resource type to ABAC
   - RouteResourceMapping for each endpoint
   - Admin-only access (no agent access to secret CRUD)

3. **MCP resources:**
   - `secrets://names` — returns `["DATABASE_URL", "API_KEY", ...]` (names only, no values)
   - `secrets://status` — returns `[{name, expires_at, last_rotated_at, status: "active"|"expiring"|"expired"}]`
   - Both resolve scope from agent's JWT claims

4. **CLI commands:**
   - `gyre secret set <name> <value> [--scope workspace|repo|tenant] [--repo <name>]`
   - `gyre secret list [--scope workspace|repo|tenant]`
   - `gyre secret delete <name>`
   - `gyre secret rotate <name> <new_value>`

5. **UI (Admin panel):**
   - Secrets tab in workspace/repo/tenant admin settings
   - Table: name, scope, type, created, expires, last rotated
   - Create/delete/rotate actions
   - Value field is write-only (shown as `••••••••` after creation)

6. **Secret rotation background job:**
   - `spawn_secret_rotation_check` job runs every 5 minutes
   - Checks for secrets approaching expiry (within 1 hour)
   - Creates notification for admin users when secrets are about to expire

## Acceptance Criteria

- [ ] POST/GET/DELETE /api/v1/secrets endpoints work
- [ ] POST /api/v1/secrets/:id/rotate works
- [ ] Secret values never returned in GET responses
- [ ] ABAC enforces admin-only access
- [ ] `secrets://names` MCP resource returns names
- [ ] `secrets://status` MCP resource returns expiry info
- [ ] `gyre secret set/list/delete/rotate` CLI commands work
- [ ] Admin UI shows secrets management tab
- [ ] Rotation background job detects expiring secrets
- [ ] `cargo test --all` passes
- [ ] `npm test` passes in `web/`

## Agent Instructions

Read `specs/system/platform-model.md` §7 "MCP Integration" and "Admin API" sections. This depends on task-097 (domain types + port) being implemented first. For MCP resources, see how existing resources (spec://, briefing://, etc.) are defined in `gyre-server/src/mcp.rs`. For CLI, see `gyre-cli/src/main.rs`. For admin UI, see how other admin tabs are structured in WorkspaceSettings.svelte or TenantSettings.svelte. Register the new API routes in `gyre-server/src/api/mod.rs`.
