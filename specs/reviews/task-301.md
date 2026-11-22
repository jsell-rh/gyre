# Review — task-301 (Meta-spec API at specced routes with scope-level admin auth)

Spec: `specs/system/agent-runtime.md` §2 API table + §2 Required vs Optional
(gap ids 917eb99ea432ee472c349487, 54a514da8df9bf4d236e09ff).
Candidate: `d4a4fa29` (base `2b5189b9`); product commits `c5a157de` (route move
+ scope-level write gate) and `d280cff1` (cross-tenant Admin write-containment
repair, fixing round-1 finding).
Verdict: **approved** — no blocking findings; all runtime probes deferred to
the independent verifier because this review sandbox has no network egress,
no crate cache, and no prebuilt target (evidence:
`/tmp/stage/review-evidence/`, this session).

## What was reviewed

Full diff `2b5189b..d4a4fa29` (11 files): route move in `api/mod.rs`,
`authorize_registry_write` in `api/meta_specs.rs`, ABAC resolver remapping in
`abac_middleware.rs`, rbac/reconciliation test updates, `web/src/lib/api.js`,
`scripts/e2e-flow.sh`, docs, coverage rows 15/21, task file.

## Verified at source level

- **Route contract (AC1).** All seven registry endpoints serve at the specced
  flat paths (`api/mod.rs:980-996`); the old `/meta-specs-registry` prefix is
  gone everywhere except the no-alias regression test. Shared-prefix coexistence
  with `/:path/blast-radius` and `/preview/*` is safe: matchit 0.7.3 supports
  different param names at one position (the base tree already ships
  `/api/v1/users/:username/*` beside `/api/v1/users/:user_id/*`), static
  segments outrank params, and `versions`/`blast-radius` are disjoint static
  children. ABAC `RouteResourceMapping` entries moved with the routes and the
  `meta-specs-registry` special case was dropped from `prefix_entity_type`;
  `scripts/check-abac-route-registry.sh` green, exemption files never listed a
  meta-spec route (base or candidate). List filters, version-bump-with-history,
  DELETE-409-while-bound, and version-content semantics are the unchanged
  handlers on new paths; internal callers (rbac.rs, reconciliation.rs tests),
  the web client, e2e script, and docs all target the new paths.
- **Scope-level authorization (AC2/AC3).** `require_registry_admin`
  (tenant-Admin for every scope) is replaced by `authorize_registry_write`,
  which authorizes POST and PUT — including `required` in either direction —
  by the entry's own scope: Global → tenant Admin (parity with the DELETE
  gate); Tenant → Admin of the scope tenant (§8, missing scope_id fails
  closed); Workspace → that workspace's Owner/Admin members via a real
  `workspace_memberships` lookup (tenant Admin bypass retained). Containment
  (`require_registry_scope`) runs first, and the Workspace branch carries its
  own unconditional `ws.tenant_id == auth.tenant_id` check, so a membership
  grant can never cross tenants and the tenant-Admin bypass is bounded to the
  workspace's own tenant. PUT derives scope from the loaded record, never the
  request body, and hides row existence from non-Admin callers (403 before
  404). The misleading "outer write gate" comment is replaced by the real
  gate. The privilege-escalation direction (workspace Owner/Admin writing
  Global/Tenant entries, incl. `required`) returns 403 with the flag
  unchanged.
- **Round-1 durable finding is fixed.** The cross-tenant Admin write hole
  (Admin of tenant A flipping `required` inside tenant B's workspaces) is
  closed by the unconditional tenant bound in the Workspace branch, mirroring
  `authorize_meta_spec_delete`, and
  `cross_tenant_admin_cannot_write_workspace_meta_spec` kills exactly that
  bug (PUT and POST → 403, flag unchanged; positive control: the scope
  tenant's Admin still succeeds and flips `required`).
- **Tests are real guards.** `workspace_owner_sets_required_on_workspace_meta_spec`
  drives the full production auth path (RS256 JWT → user resolution →
  membership lookup) and asserts 200 + persistence + injection into the
  required-workspace band via `assemble_prompt_set` (the same function
  `spawn.rs:1432` uses); it fails on the base tree's tenant-Admin gate by
  construction. Plain workspace members stay 403.
  `registry_old_prefix_removed_no_alias` fails if any old route is
  reintroduced. Pre-existing semantics tests were moved, not weakened.
- **Repo hygiene.** `check-arch`, `check-abac-exempt-handlers`,
  `check-forged-scope-fields`, `check-inert-enforcement`,
  `check-scope-literal-defaults`, `check-mem-port-contracts`,
  `check-mcp-write-tools`, `check-dead-message-kinds`,
  `check-relative-path-defaults`, `check-lossy-secret-conversion`,
  `check-unbounded-external-http` all green on the candidate. Coverage rows
  15/21 marked implemented with concrete source evidence and honest
  deferral notes; no unrelated rows touched.

## Non-blocking observation (verifier-owned generated output)

`web/dist` still references `/meta-specs-registry` — the bundle was not
regenerated in this checkpoint. Authored frontend source (`api.js`) is
correct; the deterministic verifier rebuilds and binds `web/dist` to the
tested tree.

## Deferred verifier probes

This sandbox cannot compile (no network, empty CARGO_HOME/target, and the
build wrapper forbids gyre-server test codegen in review stage). The verifier
must run:

1. `cargo test -p gyre-server registry_tests`
2. `cargo test -p gyre-server uninstrumented_role_guards_record_rbac_denied_audit_event`
3. `cargo test -p gyre-server reconciliation`
4. `cargo check -p gyre-server`
5. `cd web && npm ci && npx vitest run`
6. `bash scripts/check-task-commit-attribution.sh` (timed out here — needs
   full git history fetch)
7. `cargo fmt --check` and focused clippy on the changed files
