---
title: "Platform Model Secrets Admin API + MCP + CLI"
spec_ref: "platform-model.md §7 Admin API"
depends_on:
  - task-097
progress: ready-for-review
coverage_sections:
  - "platform-model.md §7 MCP Integration"
  - "platform-model.md §7 Admin API"
commits: ["1d4a583730b59b652758067ad6819143539769bb", "d44ab7676100568e0a84c74f9c30b28450257d00"]
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

## Shipped

Implemented on top of the task-097 `SecretRepository` port. The branch was
recovered from an interrupted worker lease (checkpoint `d44ab767`, content-
equal to the previously attributed `b569f988` + `794b018d`); this round
verified the recovered source end-to-end, repaired CI-gate findings, and
added no new behavior beyond those repairs.

### Behavior

- **Admin API** (`crates/gyre-server/src/api/secrets.rs`):
  `POST/GET /api/v1/secrets`, `DELETE /api/v1/secrets/:id`,
  `POST /api/v1/secrets/:id/rotate`. Create is create-or-update on
  `(scope, scope_id, name)` within the tenant. All handlers derive
  `tenant_id` from the auth context and validate `scope_id` against the
  caller's tenant (cross-tenant `scope_id` → 403/404). Responses use a
  dedicated metadata struct — the plaintext `value` is accepted on
  create/rotate and never serialized back.
- **ABAC** (`abac_middleware.rs`): `secret` resource type with
  `RouteResourceMapping` for all four routes; the
  `builtin-secret-admin-only` policy (priority 860) denies every non-Admin
  role, so secret CRUD is Admin-only at the policy layer (Developer and
  agent tokens are denied; verified by
  `abac_middleware::tests::secrets_routes_are_admin_only`).
- **MCP resources** (`mcp.rs`): `secrets://names` (list of names) and
  `secrets://status` (`{name, scope, expires_at, last_rotated_at, status:`
  `active|expiring|expired}`), resolved from the caller's agent JWT
  claims/agent record (tenant from the governing workspace — never a
  fabricated default). Values are never read: the metadata cascade
  mirrors `resolve_for_agent` scoping without decryption.
- **CLI** (`gyre-cli`): `gyre secret set/list/delete/rotate` with
  `--scope workspace|repo|tenant|task`, `--repo`/`--workspace` name
  resolution, `--type`, `--expires-in`.
- **UI**: `SecretsPanel.svelte` tab in Workspace/Repo/Tenant settings —
  name/scope/type/created/expires/last-rotated table, create/delete/rotate
  actions, value field write-only.
- **Rotation job** (`secret_rotation.rs`, wired as the
  `secret_rotation_check` 5-minute job in `jobs.rs`/`main.rs`): scans all
  scopes for secrets expiring within 1h (or already expired), notifies
  Admin/Owner members of the owning workspace via `SecretExpiring`
  notifications, with duplicate suppression and 24h dismissal windows.

### Repairs this round (`1d4a5837`)

- Replaced `agent_secret_scope`'s 4-tuple return with a named
  `AgentSecretScope` struct (unnamed-tuple-carrier baseline held at 3).
- Dropped three needless `&Secret` reborrows in `run_once` (clippy
  `needless_borrow` on new-file lines — inside CI clippy-diff scope).
- rustfmt on task-added lines (CI check-rustfmt-diff scope).
- `caller-scope:ok` annotation on `notify_workspace_admins` (the
  `workspace_id` is the secret's own governing workspace, not a
  caller-propagated scope).
- Re-pointed 7 pre-existing conditional-test-guard exemption line numbers
  shifted by the new Secrets tab tests (same guards, zero new entries).

### Test evidence

Focused suites, all green at `1d4a5837`:

- `cargo test -p gyre-server --lib api::secrets` — 6/6 (CRUD lifecycle,
  admin-only, cross-tenant rejection, scope/type validation, task-scope
  validation)
- `cargo test -p gyre-server --lib secret_rotation` — 3/3 (expiry
  detection + admin-only notification, dedup on rerun, healthy secrets
  never notify)
- `cargo test -p gyre-server --lib mcp` — 83/83 (incl.
  `secrets://names` cascade names-only, `secrets://status` expiry
  reporting, no-agent-scope error path, resources list)
- `cargo test -p gyre-server --lib abac_middleware::tests::secrets_routes_are_admin_only` — pass
- `cargo test -p gyre-adapters secret` — 17/17 (encryption at rest, wrong
  key/tamper failures, cascade resolution, finer-scope override, expiry
  exclusion, tenant isolation)
- `cargo test -p gyre-cli secret` — 6/6 (arg parsing incl. unknown-scope
  rejection)
- `npx vitest run src/__tests__/SecretsPanel.test.js` — 9/9;
  Workspace/Tenant/RepoSettings suites — 143/143
- Mechanical gates: check-arch, check-abac-route-registry,
  check-mcp-write-tools, check-migration-versions,
  check-dead-message-kinds, check-mem-port-contracts,
  check-in-memory-state-stores, check-byte-slice-truncation,
  check-conditional-test-guards, check-dependent-scope-resolution,
  check-unnamed-tuple-carriers — all pass; clippy/rustfmt clean on all
  task-added lines. Probe logs: /tmp/stage/review-evidence/.

Full workspace suites, all-target Clippy, and GitHub CI are left to the
verification stage per assignment scope.
