# Review — task-098 (Platform Model Secrets Admin API + MCP + CLI)

Spec: `specs/system/platform-model.md` §7 "MCP Integration" + "Admin API" (secret names/status MCP resources; POST/GET/DELETE `/api/v1/secrets`, `POST /api/v1/secrets/{id}/rotate`; CLI `gyre secret set/list/rotate`; admin-panel UI that never shows values; rotation/expiry surfacing).
Candidate under review: `b56ddc8f1` (assignment base `72ad2a12`; product surface recovered as checkpoint `010df9cd3`, then two additive rebase merges `dce3f8d1b` / `3ed1b9a36` with process commits). Depends on task-097 (`SecretRepository` port + SQLite/mem adapters).
Verdict: **approved**.

## Probes (logs under /tmp/stage/review-evidence/)

All on the merged tree at `b56ddc8f1`, `/tmp/stage/bin/cargo`, `SKIP_WEB_BUILD=1`:

- `cargo test -p gyre-server --lib secret` — 23/23 (api::secrets full CRUD lifecycle incl. stored-value round-trip through the port, admin-only denial on all four routes, cross-tenant scope_id rejection, task-scope validation; secret_rotation expiry window/dedup; mcp secrets resources incl. names-only cascade and cross-workspace invisibility; mem port contract tests).
- `cargo test -p gyre-server --lib -- mcp secret` — 104/104; `secrets_routes_are_admin_only` (ABAC layer) 1/1; `jobs` 11/11.
- `cargo test -p gyre-adapters secret` — 17/17; `cargo test -p gyre-cli --bin gyre secret` — 6/6.
- `npm ci` + `npx vitest run` on SecretsPanel/WorkspaceSettings/TenantSettings/RepoSettings — 175/175.
- Mechanical gates all pass: check-arch, check-abac-route-registry, check-mcp-write-tools, check-migration-versions, check-dead-message-kinds, check-conditional-test-guards, check-task-commit-attribution, check-non-atomic-creation, check-inert-enforcement, check-forged-scope-fields, check-scope-literal-defaults, check-fabricated-scope-defaults, check-lossy-secret-conversion, check-mem-port-contracts.
- Mutation probes (source restored, tree verified clean afterwards):
  - Neutralizing in-handler `require_admin` → `non_admin_jwt_is_forbidden` FAILS (probe-mutation-require-admin.log). The admin gate is load-bearing.
  - Removing the workspace-tenant check in `validate_scope_target` → `cross_tenant_scope_id_is_rejected` FAILS (probe-mutation-cross-tenant.log). The anti-forgery scoping is load-bearing.

## Verified working

- **Admin API** (`api/secrets.rs`): all four endpoints registered in `api/mod.rs`. `tenant_id` always from the auth context; caller-supplied `scope_id` validated against the caller's tenant for every scope level (workspace directly; repo/task through the owning workspace; tenant scope pins to the caller's tenant) — cross-tenant → 403, unknown → 404. Responses are a dedicated `SecretResponse` metadata struct with no value field — a value leak is structurally impossible, and tests additionally assert the plaintext never appears in any response body while round-tripping the stored value through `get_value` to prove real persistence. Create is create-or-update on `(scope, scope_id, name)` within the tenant (spec's "Create/update").
- **ABAC**: `secret` resource type with `RouteResourceMapping` for all four routes (registry gate green, no exemption growth); `builtin-secret-admin-only` (priority 860, Deny on non-Admin) plus in-handler `require_admin` — defense in depth. Test covers Developer denial and Admin allow on all four routes through the real middleware.
- **MCP resources**: `secrets://names` / `secrets://status` registered in `resources/list`; scope resolved from the agent JWT claims with agent-record/task fallback, tenant always from the governing workspace record (no fabricated default); the metadata cascade mirrors `resolve_for_agent` scoping (coarse→fine, nearest wins — observable in the test via a repo secret shadowing a workspace name) without any decryption path. `secrets://status` reports `active|expiring|expired` with correct 1h window semantics; user tokens without an agent scope get an explicit error, not a fallback scope.
- **CLI**: `gyre secret set/list/delete/rotate` with `--scope workspace|repo|tenant`, `--repo`/`--workspace-name` resolution (reuses the budget workspace resolver), name→id resolution for delete/rotate against the id-keyed API. Value is sent on set/rotate and never rendered; the client response type cannot represent a value.
- **UI**: `SecretsPanel.svelte` tab wired into Workspace/Repo/Tenant settings; table shows name/scope/type/created/expires/last-rotated; value field write-only, rendered `••••••••`; create/delete/rotate actions call the real API client methods.
- **Rotation job**: `secret_rotation_check` registered (300s) in `jobs.rs` and spawned in `main.rs`; scans tenant/workspace/repo scopes per tenant, notifies Admin/Owner members of the owning workspace for secrets expiring within 1h (already-expired included), with active-notification and 24h-post-dismissal dedup keyed on the secret id. Task-scoped secrets are not scanned by the job — consistent with the task plan, which scopes the job to admin-relevant scopes.
- **Merge integrity**: rebase conflicts resolved additively; WorkspaceSettings TAB_IDS carries both upstream `repos` and task-098 `secrets` (10 tabs, test count corrected 9→10); locales carry both keys; `conditional-test-guard-exemptions.txt` re-points the same pre-existing guards with zero new entries; no `web/dist` changes.
- **Coverage matrix** rows 45/46 updated from Hollow to Landed with accurate descriptions of the landed code (verified against the tree); row 55 UI clause updated to note the secrets UI pending merge.

## Minor notes (non-blocking, recorded for completeness)

- `secret_rotation::tests::healthy_secrets_never_notify` is vacuous: it never creates a tenant record, and `run_once` walks `tenants.list()` (empty in that state), so the test would pass even with a broken expiry filter. Not a coverage hole — the sibling `detects_expiring_secrets_and_notifies_admins_only` genuinely covers the window in both directions (LATER_KEY at +2h and no-expiry DATABASE_URL excluded; +30min and already-expired included) — but the test itself proves nothing and could be deleted or seeded with a tenant.
- The Shipped narrative says the CLI accepts `--scope ...|task`, but `resolve_secret_scope` bails on `task` ("expected workspace, repo, or tenant"). The spec's CLI line only requires workspace|repo|tenant, so this is an overstatement in the task notes, not a spec violation; the server API does accept task scope.
- `PgStorage`'s `SecretRepository` remains a `bail!` stub — pre-existing from task-097 (the whole trait was already stubbed at the assignment base; this task only added `find_by_id` matching the pattern) and disclosed in coverage row 44. Not a task-098 regression; PostgreSQL secret storage remains an open surface for a later task.
- Hardening note: `agent_secret_scope`'s agent-record fallback keys on `auth.agent_id` even for non-agent JWTs; for user tokens that field is the display name, so exploitation requires a display name colliding with an agent id (a UUID) and would leak names-only metadata, never values. Marginal; a `is_agent_jwt` gate on the fallback would close it.

Full workspace suites, all-target Clippy, and GitHub CI left to the verification stage per assignment scope.
