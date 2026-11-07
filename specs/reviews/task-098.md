# Review — task-098 (Platform Model Secrets Admin API + MCP + CLI)

Spec: `platform-model.md` §7 "MCP Integration" + "Admin API" (+ CLI/UI clauses).
Candidate: `dfa7e3dc1` (base `4d6759a0b`; behavior checkpoint `010df9cd3`, rebase
merge `dce3f8d1b`, process commits for task-file bookkeeping only).
Verdict: **approved** — real implementations, no fakery found; enforcement and
value-isolation claims independently mutation-tested.

## Probes (logs in `/tmp/stage/review-evidence/`)

All on the exact candidate tree, `SKIP_WEB_BUILD=1`, `/tmp/stage/bin/cargo`:

- `cargo test -p gyre-server --lib -- secret` → **23 passed / 0 failed**:
  api::secrets (CRUD lifecycle incl. stored-value round-trip via port,
  cross-tenant scope_id → 403, unknown scope target → 404, non-admin JWT → 403
  on all four routes, admin tenant-scope create, task-scope validation,
  invalid scope/type → 400), abac `secrets_routes_are_admin_only` (real
  middleware stack), secret_rotation (expiry detect, dedup on rerun,
  healthy-never-notify), mcp secrets resources (names cascade names-only,
  status expiry reporting, no-agent-scope error, resources/list registration),
  spawn secret-delivery tests, mem port contracts.
- `cargo test -p gyre-cli --bin gyre secret` → **6/6** (set repo/tenant+type+expiry,
  list, delete+rotate, unknown-scope rejection, missing value).
- `cargo test -p gyre-adapters secret` → **17/17** (encryption at rest, tamper/
  wrong-key failures, cascade, tenant isolation, rotate, find_by_id paths).
- vitest focused: SecretsPanel, WorkspaceSettings, TenantSettings, RepoSettings
  → **4 files, 170 passed / 0 failed** (after `npm ci`).
- Mechanical gates on the candidate: check-arch, check-abac-route-registry,
  check-mem-port-contracts, check-in-memory-state-stores,
  check-conditional-test-guards, check-mcp-write-tools, check-migration-versions,
  check-dead-message-kinds, check-forged-scope-fields, check-scope-literal-defaults,
  check-fabricated-scope-defaults, check-lossy-secret-conversion,
  check-inert-enforcement, check-task-commit-attribution — **all rc=0**.

### Mutation probes (each restored; `git status` clean after every restore)

1. Neutered in-handler `require_admin` (always Ok) →
   `api::secrets::tests::non_admin_jwt_is_forbidden` **fails** (201 vs 403).
   Handler-level admin gate is load-bearing and tested.
2. Removed `secret` from `builtin-secret-admin-only` resource_types →
   `abac_middleware::tests::secrets_routes_are_admin_only` **fails**
   (Developer gets 200 — the 800 developer policy allows `*` resources, so the
   860 deny policy is genuinely what makes the routes admin-only).
3. Ignored the 1h expiry window in the rotation job filter →
   `secret_rotation` dedup/detect tests **fail** (3 vs 2 notifications).
   The job's expiry logic is load-bearing and tested.

## Verified real

- **Admin API** (`api/secrets.rs`, routes in `api/mod.rs`): all four endpoints.
  Create is create-or-update on (scope, scope_id, name) within tenant,
  preserving id; list/delete/rotate resolve by id with tenant filter. Tenant
  always from auth context; caller-supplied `scope_id` validated against the
  caller's tenant for every scope kind (workspace/repo via owning workspace,
  task via governing workspace) — cross-tenant → 403, unknown → 404 (tested).
  Values never returned: `Secret` domain type has no value field and responses
  use a dedicated metadata struct — value leakage is structurally unreachable,
  and tests additionally assert the plaintext never appears in any response.
- **ABAC**: `secret` resource type, RouteResourceMapping for all four routes
  (registry gate passes), deny policy at 860 under the admin allow at 900.
- **MCP** (`mcp.rs`): `secrets://names` / `secrets://status` registered in
  `resources/list` and dispatched through the real `resources/read` path
  (`handle_resource_read`). Scope from agent JWT claims with agent-record/task
  fallback; tenant derived from the governing workspace record, never
  fabricated. Metadata cascade (tenant→workspace→repo→task, nearest wins)
  mirrors `resolve_for_agent` without any decryption call — values unreachable.
  Cross-workspace secrets invisible (tested); user tokens without agent scope
  get an error, not a fallback scope.
- **CLI**: `gyre secret set/list/delete/rotate` with `--scope
  workspace|repo|tenant`, `--repo`/`--workspace-name` resolution reusing the
  budget machinery, name→id resolution for delete/rotate (id-keyed API),
  `--type`, `--expires_at`.
- **UI**: `SecretsPanel.svelte` tab in Workspace/Repo/Tenant settings;
  metadata table + create/delete/rotate actions; value field write-only
  (test asserts the placeholder renders even with a fresh API response).
- **Rotation job**: `secret_rotation_check` registered (300s) in `jobs.rs` and
  spawned in `main.rs`; notifies Admin/Owner members of the owning workspace
  (tenant-scoped secrets fan out to every workspace's admins); duplicate
  suppression keyed on (user, workspace, secret id) with 24h dismissal window.
- **Port**: `find_by_id` added with tenant filter, implemented in mem +
  sqlite (Diesel, tenant-scoped query); spawn.rs failing-mock updated. PG stub
  matches the pre-existing whole-port PG stub from task-097 (documented
  residual in coverage row 44, not this task's regression).

## Notes for the verifier (non-blocking)

- `web/dist` in the candidate is byte-identical to the assignment base — the
  committed bundle does not yet contain the Secrets tab. This matches the repo
  convention (verified asset regeneration is a separate
  `build(web): regenerate verified assets` commit performed by the pipeline's
  build/publication stage); confirm that stage rebuilds dist from this branch's
  `web/src` so the UI ships.
- Full workspace `cargo test --all`, all-target Clippy, and GitHub CI are left
  to verification per assignment scope; this review ran the focused suites
  above plus all mechanical gates.
