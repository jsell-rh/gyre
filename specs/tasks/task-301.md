---
title: "Meta-spec API at specced routes with scope-level admin auth"
spec_ref: "Spec gap spec-41a92c20354c6a108e6074e3 / meta-spec-api-contract / 4a3f940495a8"
depends_on: []
progress: ready-for-review
commits: ["c5a157de6c437f3f3c7f30156d88c46545fea89b", "0348121d23235e8e7ea58568dec921a0308f094b"]
---

## Required behavior

Per agent-runtime.md §2 API table and §2 Required vs Optional. (1) Route contract: serve the registry CRUD at the specced paths — GET/POST /api/v1/meta-specs, GET/PUT/DELETE /api/v1/meta-specs/:id, GET /api/v1/meta-specs/:id/versions, GET /api/v1/meta-specs/:id/versions/:version — by moving the existing handlers from /api/v1/meta-specs-registry with their semantics unchanged (list filters ?scope=&scope_id=&kind=&required=; create from body scope/scope_id; update bumps version with history retained; delete 409 when spec bindings reference it; version endpoints). The /api/v1/meta-specs/:path/blast-radius route stays (no structural conflict). ABAC RouteResourceMapping entries, RBAC tests, internal callers (reconciliation.rs), and the web API client are updated to the new paths; the old /meta-specs-registry paths are removed with no silent alias, and scripts/check-abac-route-registry.sh stays green with the exemption file only shrinking. (2) Scope-level authorization: POST and PUT on registry entries (including flipping required in either direction) are authorized by the meta-spec's own scope — Global/tenant-scope meta-specs writable only by tenant Admin; workspace-scope meta-specs writable by that workspace's Owner/Admin (tenant Admin retains bypass). A workspace Owner/Admin writing a tenant-scope meta-spec (including required) is rejected 403 — the privilege-escalation direction the spec warns about stays impossible; the currently missing workspace-admin authority on workspace meta-specs now exists and the misleading 'outer write gate' comment (api/meta_specs.rs:8993) is replaced by real per-scope enforcement. DELETE keeps its existing scope-aware gate.

## Acceptance Criteria

- [ ] GET /api/v1/meta-specs?scope=workspace&scope_id={id}&kind=meta:persona returns the filtered list (at the base this path 404s — only /meta-specs-registry serves it); PUT /api/v1/meta-specs/:id bumps the version and retains history; DELETE /api/v1/meta-specs/:id returns 409 while a spec binding references it; GET /api/v1/meta-specs/:id/versions/:version returns that version's content.
- [ ] A workspace Owner (not tenant Admin) successfully PUTs required=true on a workspace-scope meta-spec; the flag persists and the meta-spec is injected into agents spawned in that workspace — at the base this is rejected because the write gate is tenant-Admin for every scope.
- [ ] The same workspace Owner PUT (any field, including required) on a Global-scope meta-spec returns 403.

## Verification intent

Authenticate as a workspace Owner and PUT /api/v1/meta-specs/{id of a workspace-scope meta-spec} with {"required": true} — at the base the registry write gate requires the platform tenant Admin role for every scope (the 'outer write gate already enforces this for every PUT' comment at api/meta_specs.rs:8993 describes a gate that is tenant-Admin for all scopes), so the specced workspace-admin authority returns 403 and the probe fails; after the change it must succeed and newly spawned agents in that workspace must receive the meta-spec in their required band. At the same base, GET /api/v1/meta-specs?scope=workspace&... returns 404 because the CRUD surface lives only at /api/v1/meta-specs-registry (api/mod.rs:977-991).

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/agent-runtime.md`. Source: `678a8ac3e3bfeafeb66cdb47e897b44e8ecda2ce`; plan: `6a0d0dc0bb142af9f5ac4d1be1c142f795b9b34555ddd9a114ef1f10f0a59e6d`.

[
  {
    "id": "54a514da8df9bf4d236e09ff",
    "decision": "gap",
    "requirement_quote": "**Who can set `required`:** Only admins at the meta-spec's own scope level — tenant admins for tenant meta-specs, workspace admins (Owner/Admin role) for workspace meta-specs.",
    "detail": "The injection semantics are implemented: required meta-specs are always injected (both tenant and workspace bands filter on required: Some(true)), cannot be opted out, and bypass nearest-scope resolution; Repo scope structurally cannot be required (the scope parser accepts only Global/Tenant/Workspace, and the Persona entity has no required flag at all); same-name/different-id collisions inject both, required first, as separate prompt sections. GAP: the scope-level grant is not implemented as specced — creating and updating registry entries (including flipping the required flag) is gated to the platform tenant Admin role for EVERY scope, so a workspace Owner/Admin cannot set required on a workspace-scoped meta-spec; only DELETE carries a full scope-aware authorization gate. The code comment claims 'The outer write gate already enforces this for every PUT', but that outer gate is tenant-Admin for all scopes — stricter than the spec for workspace meta-specs, i.e. the specced workspace-admin authority simply does not exist. (The privilege-escalation direction the spec warns about — a workspace admin marking a tenant meta-spec required — is prevented, but by over-restriction, not by scope-level enforcement.)",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/meta_specs.rs",
        "quote": "\"only Admin role may modify the meta-spec registry\".to_string(),",
        "detail": "The require_registry_admin helper gates POST and PUT on the tenant Admin role regardless of the meta-spec's scope — workspace Owner/Admin members cannot set required on workspace meta-specs, so the specced scope-level grant is absent for the update/create path."
      },
      {
        "file": "crates/gyre-server/src/api/meta_specs.rs",
        "quote": "// The outer write gate already enforces this for every PUT.",
        "detail": "The required-flag handling cites an outer scope-level gate that does not exist as described: the outer gate is the tenant-Admin-only role check, so the comment's claim is inaccurate for workspace-scoped meta-specs."
      },
      {
        "file": "crates/gyre-server/src/prompt_assembly.rs",
        "quote": "required: Some(true),",
        "detail": "Positive wiring: required meta-specs are unconditionally injected in both the tenant band (Global + caller's Tenant) and the workspace band — the always-injected, cannot-opt-out semantics are real."
      }
    ]
  },
  {
    "id": "917eb99ea432ee472c349487",
    "decision": "gap",
    "requirement_quote": "| `GET /api/v1/meta-specs` | GET | List meta-specs (filtered by `?scope=`, `?scope_id=`, `?kind=`, `?required=`). ABAC: scope_id extracted from query param for tenant/workspace authorization. |",
    "detail": "Every specced endpoint semantics exists with real wiring: list with scope/scope_id/kind/required filters, create, get-with-content, update (new version, old retained), delete (409 when spec bindings reference it), versions list, and specific-version content — all with ABAC resolver entries and per-handler role gates. However, none of them is served at the specced path: the CRUD surface lives at /api/v1/meta-specs-registry (the /api/v1/meta-specs prefix is occupied by the blast-radius route /api/v1/meta-specs/:path/blast-radius), so a client written against this spec's API table receives 404 on every listed route. The flat-route rationale in the spec's note is honored (single entity type, scope via query/body), but the route names themselves — part of the API contract — are not.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "\"/api/v1/meta-specs-registry\",",
        "detail": "The meta-spec CRUD endpoints (list/create, get/put/delete, versions) are registered under the meta-specs-registry prefix, not the specced /api/v1/meta-specs path."
      },
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "\"/api/v1/meta-specs/:path/blast-radius\",",
        "detail": "The specced /api/v1/meta-specs prefix is occupied by a different route (blast-radius), so GET /api/v1/meta-specs — the spec's list endpoint — is not served at all."
      },
      {
        "file": "crates/gyre-server/src/api/meta_specs.rs",
        "quote": "\"cannot delete meta-spec '{id}': active bindings reference it\"",
        "detail": "Positive wiring: DELETE fails (409) when spec bindings reference the meta-spec, one of the specced endpoint behaviors."
      },
      {
        "file": "web/src/lib/api.js",
        "quote": "return request(`/meta-specs-registry${qs ? '?' + qs : ''}`);",
        "detail": "The web client also targets the renamed prefix with the specced query filters (scope/scope_id/kind/required), confirming the functional parity of the surface at the divergent path."
      }
    ]
  }
]

## Shipped

Commit `c5a157de` implements both halves of the task; this commit adds
coverage-row updates, the stale cross-reference fix in
`specs/system/meta-spec-reconciliation.md`, and this bookkeeping.

### Route contract (agent-runtime §2 API table)

The registry CRUD moved from `/api/v1/meta-specs-registry*` to the specced
flat routes with semantics unchanged: GET/POST `/api/v1/meta-specs`
(list filters `?scope=&scope_id=&kind=&required=`, case-insensitive
scope/kind parsing at the API edge), GET/PUT/DELETE
`/api/v1/meta-specs/:id` (content PUT bumps the version with prior
versions retained and resets approval to Pending; DELETE returns 409 while
spec bindings reference the entry), and the `/versions` +
`/versions/:version` endpoints (api/mod.rs:980-996). The old prefix is
removed with no silent alias — test `registry_old_prefix_removed_no_alias`
asserts every old verb 404s and that the `:path/blast-radius` route still
resolves under the shared prefix (matchit 0.7.3 normalizes param names, so
`:id` and `:path` coexist; the static `preview` segments stay prioritized).
ABAC `RouteResourceMapping` entries moved to the new patterns
(abac_middleware.rs:782-796) and the `meta-specs-registry` special case was
dropped from `prefix_entity_type` (mechanical singularization now covers
`meta-specs` → `meta_spec`). Internal callers (reconciliation.rs tests,
rbac.rs tests), the web client (web/src/lib/api.js), scripts/e2e-flow.sh,
and docs/api-reference.md all target the new paths;
`scripts/check-abac-route-registry.sh` is green and the exemption files
never listed a meta-spec route (the mappings were resolver entries at the
base too — nothing was grandfathered).

### Scope-level authorization (§2 Required vs Optional)

`require_registry_admin` (tenant-Admin for every scope) was replaced by
`authorize_registry_write`, which authorizes POST and PUT — including
flipping `required` in either direction — by the meta-spec's OWN scope:
Global/Tenant entries are writable only by a tenant Admin (Tenant
additionally requires the caller to be an Admin of the scope tenant);
Workspace entries are writable by that workspace's Owner/Admin members via
a real `workspace_memberships` lookup, with the tenant Admin bypass
retained. `require_registry_scope` (containment) runs first so a
membership-based grant can never cross tenants. The PUT handler now loads
the record and derives scope from it (never from the request), with 403
before 404 for unprivileged callers on missing ids (same ordering as
DELETE). The misleading "outer write gate already enforces this for every
PUT" comment is replaced by a description of the real per-scope gate. A
workspace Owner/Admin PUT on a Global or Tenant entry — the
privilege-escalation direction the spec warns about — returns 403 without
flipping the flag. DELETE keeps its existing scope-aware gate, and the §8
force flag still requires an Admin of the scope tenant.

### Test evidence

New tests in `registry_tests` (api/meta_specs.rs):
`workspace_owner_sets_required_on_workspace_meta_spec` (AC2 — Owner, not
tenant Admin, PUTs `required=true` on a Workspace-scoped entry at the new
path: 200, flag persists in the store, `assemble_prompt_set` injects it as
a `required-workspace` section; a workspace Admin member is also allowed;
a plain Developer member stays 403; also asserts the AC1 filtered
listing), `workspace_owner_cannot_write_global_or_tenant_meta_spec` (AC3 —
the same Owner on a Global entry and on a Tenant entry of the caller's own
tenant: 403, flag unchanged), and `registry_old_prefix_removed_no_alias`.
Existing semantics tests (version bump/history, DELETE 409, version
content, membership/delete gates, force gate, rbac-denied audit,
reconciliation triggers) were retargeted to the new paths.

In this sandbox the build wrapper permits `cargo check` only:
`SKIP_WEB_BUILD=1 cargo check -p gyre-server --lib --tests` exits 0 (all
test code type-checks), and the abac-route-registry,
abac-exempt-handlers, and arch script gates pass (outputs saved under
/tmp/stage/review-evidence). gyre-server test execution exceeds the 4 GiB
sandbox budget and is deferred to the independent verifier:
`SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib registry_tests` (plus
`--lib rbac` and the prompt_assembly suite) — full list in
/tmp/stage/review-evidence/task-301-probes.md.
