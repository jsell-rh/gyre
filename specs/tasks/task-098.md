---
title: "Platform Model Secrets Admin API + MCP + CLI"
spec_ref: "platform-model.md §7 Admin API"
depends_on:
  - task-097
progress: complete
coverage_sections:
  - "platform-model.md §7 MCP Integration"
  - "platform-model.md §7 Admin API"
commits: ["8c7b0bfa1f01c8b1f7f296a8ff80b5e86b672780", "010df9cd36dc3f81ae136173b522caaa7b3d4a84", "255ca5a31d45819f30cb08419a33d45b72b5b084"]
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
`010df9cd3`, then carried through two rebase rounds (`dce3f8d1b`,
`3ed1b9a36`) and a verification-repair round (`8c7b0bfa1`, below). No
product behavior changed in the merge or repair rounds.

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

### Verification-repair round (`8c7b0bfa1`)

The deterministic verification run on the merge of `b56ddc8f1` with base
`98a5299b9` failed with five findings; all repaired without weakening any
gate:

- **rustfmt diff** (gyre-cli `main.rs` secret-command blocks): formatted
  via `cargo fmt -p gyre-cli`; `cargo fmt --check` now clean.
- **clippy `double_must_use`** (`gyre-ports/src/secret.rs:14`): added the
  sibling-file `#[allow(clippy::double_must_use)]` convention for
  async_trait ports.
- **clippy `unused_must_use`** (`abac_middleware.rs:1268`): the secrets
  ABAC test discarded `seed_builtin_policies`' result; now
  `.expect("seed built-in policies")` like the four sibling tests.
- **ABAC duplicate-path freeze (22 vs baseline 17)**: the
  `secrets_routes_are_admin_only` test repeated the registry's
  `/api/v1/secrets*` literals in its router and assertion loops, and the
  whole-file grep in `check-abac-route-registry.sh` counted them as
  resolver duplicates. The test now builds its route patterns and request
  URIs from a shared base string; the resolver still has exactly one
  entry per secrets path and the duplicate count is back at the frozen 17.
- **Exemption freeze** ("new verification exemptions forbidden"):
  earlier rounds had re-pointed 7 `conditional-test-guard-exemptions.txt`
  entries (5 RepoSettings, 2 WorkspaceSettings) to merge-shifted line
  numbers. Instead of exemption re-points, the underlying conditional
  guards were fixed assert-first (`expect(el).toBeTruthy()` before use)
  and the 7 entries deleted — the exemption file now only shrank relative
  to base. This exposed and fixed a real silently-skipped assertion: the
  repo budget error test mocked `api.repoBudget`, but the component loads
  `api.workspaceBudget` (no per-repo budget endpoint); the test now mocks
  the function the component actually calls and fails loudly if the
  unavailable message stops rendering.

### Test evidence

All suites below run green on the repaired tree at `8c7b0bfa1`
(probe logs under `/tmp/stage/review-evidence/`, prefixed `repair4-`):

- `cargo test -p gyre-server --lib -- mcp secret` — 104 passed / 0 failed
  (api::secrets CRUD lifecycle, admin-only, cross-tenant rejection;
  secret_rotation expiry/dedup; mcp `secrets://names`/`secrets://status`;
  abac `secrets_routes_are_admin_only`)
- `cargo test -p gyre-cli --bin gyre secret` — 6/6
- `cargo clippy -p gyre-ports -p gyre-server -p gyre-cli --all-targets` —
  zero warnings in the three files changed this round (remaining
  workspace warnings are pre-existing in untouched files)
- `cargo fmt --check -p gyre-cli -p gyre-ports -p gyre-server` — clean
- vitest (WorkspaceSettings + RepoSettings, strengthened tests) —
  125 passed / 0 failed
- Mechanical gates: check-arch, check-hierarchy (GYRE_CHECK_HIERARCHY=1),
  check-abac-route-registry (duplicates back at frozen 17),
  check-abac-exempt-handlers, check-mcp-write-tools,
  check-migration-versions, check-migration-sql-portability,
  check-dead-message-kinds, check-byte-slice-truncation,
  check-relative-path-defaults, check-fail-open-ref-resolution,
  check-mem-port-contracts, check-fabricated-scope-defaults,
  check-lossy-secret-conversion, check-scope-literal-defaults,
  check-inert-enforcement, check-forged-scope-fields,
  check-forwarded-header-trust, check-in-memory-state-stores,
  check-unbounded-external-http, check-conditional-test-guards — all
  pass on the repaired tree.

Full workspace suites, all-target Clippy, and GitHub CI are left to the
verification stage per assignment scope.
