# Review — task-098 (Platform Model Secrets Admin API + MCP + CLI)

Spec: `platform-model.md` §7 "MCP Integration" + "Admin API" (incl. CLI + admin-panel UI clauses and the rotation-check background job from the implementation plan).
Candidate under review: `ee50bacb5a7eb924ebb1700852cdf9c340fc2690` (base `d81b0fe4`; task commits `d44ab767` recovered source + `1d4a5837` CI-gate repairs, plus docs/process commits `68968155`/`ee50bacb`).
Verdict: **approved**.

## Round 1 — independent verification

All probes run at candidate `ee50bacb` with `/tmp/stage/bin/cargo` (fresh `/tmp/gyre-target`, cold build ~16m). Logs under `/tmp/stage/review-evidence/`.

- `cargo test -p gyre-server --lib api::secrets` → **6/6 pass** (CRUD lifecycle incl. stored-value round-trip through the port proving real persistence, admin-only JWT denial on all four routes, cross-tenant scope_id → 403, unknown scope/target → 404/400, task-scope validation).
- `cargo test -p gyre-server --lib secret_rotation` → **3/3 pass**.
- `cargo test -p gyre-server --lib mcp` → **83/83 pass** (matches shipped claim exactly; includes `secrets://names` cascade names-only, `secrets://status` active/expiring/expired reporting, no-agent-scope error, resources/list registration).
- `cargo test -p gyre-server --lib secrets_routes_are_admin_only` → pass (Developer denied + Admin allowed on all four routes **through the real `abac_middleware` layer** with seeded builtin policies — not just the in-handler check).
- `cargo test -p gyre-adapters secret` → **17/17 pass**; `cargo test -p gyre-cli secret` → 6/6; `cargo test -p gyre-server --lib secret` (all secret-tagged) → 23/23, incl. task-097's spawn-injection and mem contract tests against the extended mock.
- Frontend: `npm ci` then `npx vitest run src/__tests__/SecretsPanel.test.js` → **9/9**; Workspace/Tenant/RepoSettings suites → **143/143**.
- Mechanical gates: `check-arch`, `check-abac-route-registry`, `check-mcp-write-tools`, `check-conditional-test-guards` → all pass. `conditional-test-guard-exemptions.txt` diff is line-number re-pointing only (21 entries before and after — no growth).

### What was verified as real

- **Admin API** (`api/secrets.rs`): all four routes registered in `api/mod.rs`; create is create-or-update on `(scope, scope_id, name)` within the tenant (update path via delete+re-create, so the mem/SQLite uniqueness contract is respected); `tenant_id` always from the auth context; caller-supplied `scope_id` validated against the caller's tenant for every scope level (workspace directly, repo/task through the owning workspace — cross-tenant → 403, unknown → 404). Responses are a dedicated `SecretResponse` metadata struct — the `Secret` domain type has no value field at all (task-097), so values are structurally unreachable in any response. Delete returns 404 (not silent no-op) when absent.
- **ABAC**: `secret` resource type + `RouteResourceMapping` for all four routes; `builtin-secret-admin-only` (priority 860, Deny on `subject.global_role != Admin`, actions `*`, resource `secret`) under `builtin-admin-all-operations` (900) — verified through the live middleware, and `require_admin` in-handler as defense in depth.
- **MCP resources**: `secrets://names` / `secrets://status` registered and implemented; scope resolved from agent JWT claims with agent-record/task fallback, tenant from the governing workspace record (no fabricated `"default"`); metadata cascade mirrors `resolve_for_agent` scoping (tenant → workspace → repo → task, nearest wins) without any decryption path — values never read.
- **CLI**: `gyre secret set/list/delete/rotate` with `--scope workspace|repo|tenant`, `--repo`/`--workspace-name` resolution, `--secret-type`, `--expires-at`; name-keyed delete/rotate resolve the id via list first. Client types cannot represent a value.
- **UI**: `SecretsPanel.svelte` tab wired into Workspace/Repo/Tenant settings; value input `type="password"`, list renders `••••••••`, `api.js` secrets methods metadata-only.
- **Rotation job**: `secret_rotation_check` registered (300s) in `jobs.rs` and spawned in `main.rs`; scans tenant/workspace/repo scopes, notifies Admin/Owner members of the owning workspace via `SecretExpiring` (priority 7, new `NotificationType` variant with roundtrip tests), dedup on active notification + 24h dismissal window keyed by secret id.
- **Port extension**: `find_by_id` added to `SecretRepository` with real implementations in mem + SQLite (tenant-filtered query) and the crate-conventional PgStorage `bail!` (same posture as the rest of task-097's PG secret surface, recorded in coverage row 44 — not a new gap from this task).

### Adversarial probe — vacuous test

`secret_rotation::tests::healthy_secrets_never_notify` seeds no tenant, and `run_once` walks `tenants.list()` which starts empty in the mem state — the committed test passes vacuously (it would pass even if `expiring()` were inverted). Probe: temporarily added the tenant creation to the test, re-ran → **still passes** (`/tmp/stage/review-evidence/healthy-secrets-probe.log`), confirming the production behavior (healthy secrets never notify) is real; source restored, tree clean. The behavior is also positively covered by `detects_expiring_secrets_and_notifies_admins_only`, which seeds no-expiry and +2h secrets inside a tenant-scanned state and asserts exactly 2 notifications. Non-blocking weak-test observation; a follow-up could seed the tenant in that test so it fails if the scan is ever broken.

### Non-blocking observations (no action required for approval)

- Shipped notes say the CLI supports `--scope ...|task` and flags `--type`/`--expires-in`; the actual flags are `--secret-type`/`--expires-at` and task scope is rejected at resolution time. The spec/implementation plan only requires `workspace|repo|tenant` for the CLI, so this is an inaccuracy in the shipped-note prose, not a spec divergence.
- `fmt_secret_epoch` renders epoch seconds rather than local dates — cosmetic, documented in-code.

Full workspace `cargo test --all`, all-target Clippy, and GitHub CI are deferred to the verification stage per assignment scope, as the shipped notes state.
