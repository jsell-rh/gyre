# Review: task-301 — Meta-spec API at specced routes with scope-level admin auth

**Candidate:** `6d7077c003ef998214afd72a691b741c6d072705` (base `f0f2ba5da6355cd7ed059f095de03848f3bada50`)
**Reviewer:** independent review agent (pi runtime)
**Verdict:** NOT APPROVED — 1 finding (cross-tenant Admin bypass in the new write gate)
**Date:** 2026-11-22

## Scope reviewed

Diff base..candidate: `crates/gyre-server/src/api/meta_specs.rs` (836 lines),
`api/mod.rs`, `abac_middleware.rs`, `rbac.rs`, `reconciliation.rs`, `web/src/lib/api.js`,
`scripts/e2e-flow.sh`, `docs/api-reference.md`, `specs/coverage/system/agent-runtime.md`,
`specs/system/meta-spec-reconciliation.md`, `specs/tasks/task-301.md`.

## What is correctly implemented (verified against source)

1. **Route contract (row 21).** All seven registry endpoints moved from
   `/api/v1/meta-specs-registry*` to the specced flat paths
   (GET/POST `/api/v1/meta-specs`, GET/PUT/DELETE `/:id`, `/:id/versions`,
   `/:id/versions/:version`) with handler semantics unchanged; ABAC
   `RouteResourceMapping` entries moved with them and the
   `meta-specs-registry` special case dropped from `prefix_entity_type`;
   rbac.rs / reconciliation.rs test URIs, the web client (`api.js`), the e2e
   script and the docs updated; the `:path/blast-radius` route stays. No
   silent alias (only code references to the old prefix are inside the
   `registry_old_prefix_removed_no_alias` test). `check-abac-route-registry.sh`
   green, exemption files untouched (they never listed meta-spec routes).
   Router-panic risk from `:id` vs `:path` at the same segment is ruled out:
   `workspaces/:id` + `workspaces/:workspace_id` and `users/:username` +
   `users/:user_id` already coexist on matchit 0.7.3.
2. **Scope-level write gate (row 15).** `authorize_registry_write` replaces
   `require_registry_admin` on POST and PUT; scope derived from the record
   (PUT) or validated body (POST), never trusted from the request on update;
   containment (`require_registry_scope`) runs before the grant; the
   workspace grant is a real `workspace_memberships.find_by_user_and_workspace`
   lookup (port implemented by mem/SQLite/Postgres adapters); tenant Admin
   bypass retained; the misleading "outer write gate" comment is replaced by
   an accurate one. PUT's missing-row handling mirrors DELETE (403 before 404
   for non-admin callers — no existence oracle).
3. **Middleware interaction (production wiring).** The new tests use
   `api_router()` directly, which does NOT layer `abac_middleware`
   (`build_router` in lib.rs:1046 does). I verified by source reading that
   the middleware does not veto the specced grant: `meta_spec` has no arm in
   `extract_entity_attributes`, so `tenant-isolation` (950, requires
   `resource.tenant_id` to exist) and `workspace-membership-required` (940)
   never fire for these routes, and `developer-write-access` (800) allows a
   global-Developer workspace Owner through to the handler. AC2/AC3 therefore
   hold end-to-end on the real server, not only in tests.
4. **Tests.** The three new tests are substantive and kill real bugs:
   Owner-PUTs-required (persists + injected via `assemble_prompt_set` with
   source `required-workspace`, which exists in prompt_assembly.rs:233-236),
   Owner-on-Global/Tenant 403 (flag not flipped), old-prefix-404 sweep, plus
   ws-Admin-allowed and plain-member-403 variants.
5. **Probes run** (see `/tmp/stage/review-evidence/`): `check-abac-route-registry.sh`
   exit 0; `cargo check -p gyre-server` exit 0 (7m58s); residue grep; matchit
   precedent; middleware policy trace. Server test-binary compilation was
   deferred to the verifier per the 4 GiB budget rule — required runs are
   listed in the probe log.

## Finding (blocking)

**Cross-tenant Admin can write other tenants' workspace meta-specs —
inconsistent with the same file's DELETE gate and §8 containment.**

- `authorize_registry_write` (api/meta_specs.rs, new) Workspace branch:
  `is_tenant_admin = auth.roles.contains(&UserRole::Admin)` with **no tenant
  match**, and the preceding `require_registry_scope` Workspace branch
  explicitly bypasses containment for Admins
  (`ws.tenant_id == auth.tenant_id || Admin`).
- Contrast, same file: `authorize_meta_spec_delete` Workspace branch rejects
  a cross-tenant caller unconditionally ("a caller scoped to another tenant
  must not even learn the meta-spec exists") — including Admins — and the
  existing test at meta_specs.rs:11721-11743 asserts an Admin of a different
  tenant gets 403 on DELETE ("the §8 containment rule").
  `authorize_registry_write`'s own Tenant branch also requires
  `auth.tenant_id == scope_tenant`.
- Consequence: an Admin of tenant A can PUT (any field, including
  `{"required":true}`) and POST workspace-scoped meta-specs inside tenant B's
  workspaces; flipping `required` injects the prompt into every agent tenant
  B spawns in that workspace. The same caller cannot DELETE the entry (403).
  The middleware does not block it (admin-all-operations at 900; tenant
  isolation never fires for meta_spec resources — see probe 5).
- Fix: bound the Admin bypass in the Workspace branch by
  `ws.tenant_id == auth.tenant_id`, mirroring `authorize_meta_spec_delete`
  and the function's own Tenant branch, and add a cross-admin PUT test
  mirroring the existing cross-admin DELETE test.

The acceptance criteria as literally written are satisfied (they do not
exercise the cross-tenant-Admin corner), but the task required "real
per-scope enforcement" replacing the old gate, and the freshly authored gate
leaves the Admin authority unscoped by tenant on a governance write path with
prompt-injection consequences — the missing-enforcement class this review is
chartered to catch.

## Non-blocking observations

- The workspace tree arrived with a pre-existing unstaged modification to
  `specs/tasks/task-301.md` (progress/commits reset to the assignment
  baseline, Shipped section removed). It predates this review and was not
  touched; the committed candidate carries `progress: ready-for-review`,
  `commits: [c5a157de]` (the pipeline's attribution machinery manages the
  frontmatter, including the later add/remove of the docs commit 0348121d).
- No generated-output concerns: `web/dist/` is untouched; only authored
  sources (`web/src/lib/api.js`) changed.

## Verifier must run

- `cargo test -p gyre-server registry_tests` (all new/updated registry tests)
- `cargo test -p gyre-server rbac`, `reconciliation`, `prompt_assembly`
- `cd web && npm ci && npm test`
