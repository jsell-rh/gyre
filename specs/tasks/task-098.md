---
title: "Platform Model Secrets Admin API + MCP + CLI"
spec_ref: "platform-model.md §7 Admin API"
depends_on:
  - task-097
progress: ready-for-review
coverage_sections:
  - "platform-model.md §7 MCP Integration"
  - "platform-model.md §7 Admin API"
commits: ["010df9cd36dc3f81ae136173b522caaa7b3d4a84"]
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

Implemented on top of the task-097 `SecretRepository` port. The task-098
source was recovered from an interrupted worker lease as checkpoint
`010df9cd3` (behavior and prior gate/repair rounds described below); this
round resolved the outstanding rebase onto the assignment base `4d6759a0b`
(HSI Policies UI + meta-spec sweep) as merge commit `dce3f8d1b`. No
behavior was added in the merge round.

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

### Rebase-resolution round (`dce3f8d1b`)

All conflicts were additive both-sides features and were resolved by
keeping both sides:

- `jobs.rs` / `main.rs`: both the task-098 `secret_rotation_check` job and
  the upstream `meta_spec_conformance_sweep` job are registered and
  spawned.
- `WorkspaceSettings.svelte`: `TAB_IDS` merged to 9 tabs — upstream
  `policies` tab alongside the task-098 `secrets` tab; both panels and
  their locale keys verified present in the merged tree.
- `WorkspaceSettings.test.js`: the "renders all tabs" count corrected
  from 8 to 9 — a semantic conflict the textual merge missed (the merged
  label list already asserted both `Policies` and `Secrets`, so the stale
  count would fail on the correct component).
- `conditional-test-guard-exemptions.txt`: re-pointed the two pre-existing
  WorkspaceSettings guard entries to their merge-shifted line numbers
  (same guards, zero new entries).
- `web/dist`: left as the executor-restored assignment-base state
  (byte-identical to `4d6759a0b`); final integration rebuilds and
  publishes those assets remotely.

### Test evidence

All suites below run green on the merged tree at `dce3f8d1b`
(probe logs under `/tmp/stage/review-evidence/`, prefixed `repair-`):

- `cargo check -p gyre-server` — clean (SKIP_WEB_BUILD=1, committed dist)
- `cargo check -p gyre-adapters -p gyre-cli --tests` — clean
- `cargo test -p gyre-server --lib -- mcp secret` — 104 passed / 0 failed
  (api::secrets CRUD lifecycle, admin-only, cross-tenant rejection;
  secret_rotation expiry/dedup; mcp incl. `secrets://names` cascade
  names-only, `secrets://status` expiry reporting, no-agent-scope error
  path; abac secrets_routes_are_admin_only)
- `cargo test -p gyre-server --lib -- jobs` — 11/11 (covers the
  conflicted `jobs.rs` registration path)
- `cargo test -p gyre-adapters secret` — 17/17 (encryption at rest, wrong
  key/tamper failures, cascade resolution, finer-scope override, expiry
  exclusion, tenant isolation)
- `cargo test -p gyre-cli --bin gyre secret` — 6/6 (arg parsing incl.
  unknown-scope rejection)
- vitest focused settings suites (WorkspaceSettings, TenantSettings,
  RepoSettings, SecretsPanel) — 4 files, 170 passed / 0 failed
- Mechanical gates on the merged tree: check-arch,
  check-abac-route-registry, check-mcp-write-tools,
  check-migration-versions, check-dead-message-kinds,
  check-mem-port-contracts, check-in-memory-state-stores,
  check-byte-slice-truncation, check-conditional-test-guards,
  check-dependent-scope-resolution, check-fail-open-ref-resolution,
  check-scope-literal-defaults, check-fabricated-scope-defaults,
  check-lossy-secret-conversion, check-inert-enforcement,
  check-relative-path-defaults, check-forwarded-header-trust,
  check-forged-scope-fields, check-task-commit-attribution,
  check-abac-exempt-handlers, check-migration-sql-portability,
  check-non-atomic-creation, check-warn-continue-creation,
  check-unwritten-store-fields — all pass.
  check-unnamed-tuple-carriers reports the same 3 pre-existing hits as
  both merge parents (otlp_receiver.rs:238, mem.rs ConvMap,
  git_http.rs accumulate_chain_constraints; check not CI-wired); the
  merge adds zero new carriers.

Full workspace suites, all-target Clippy, and GitHub CI are left to the
verification stage per assignment scope.
