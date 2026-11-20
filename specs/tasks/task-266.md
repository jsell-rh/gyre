---
title: "SCIM groups and group-to-workspace role mapping"
spec_ref: "Spec gap spec-31d3ae72e12e22c52442e309 / scim-groups-workspace-mapping / b60edce5bc49"
depends_on: [task-265]
progress: ready-for-review
commits: ["70e1fa7b3b05b3f11546942a694f1e2226d9e968", "eee34c67807e51536f21fd3bc3b8ec91604bf9a9"]
---

## Required behavior

Implement the SCIM Groups surface and the admin-configured group-to-workspace mapping so IdP group pushes drive Gyre workspace membership. (1) All six /scim/v2/Groups endpoints (GET list with startIndex/count pagination, GET {id}, POST create, PUT replace membership, PATCH partial update add/remove members, DELETE) per RFC 7643/7644 Group schema (schemas, id, externalId, displayName, members with value/display), authenticated by the same dedicated GYRE_SCIM_TOKEN bearer (fail-closed), persisted via a gyre-ports trait + SQLite/Postgres adapters with the next unused 6-digit migration sequence (no in-memory AppState store — restart-orphaned state is prohibited by the standing invariants). (2) PUT /api/v1/workspaces/{id}/scim-mappings (workspace-admin authorized, registered in the ABAC RouteResourceMapping) storing per-workspace mappings [{idp_group, workspace_role}] durably. (3) Mapping effects: when a SCIM group push (PUT/PATCH /scim/v2/Groups or group create with members) adds a user to a group that a workspace maps to a role, Gyre automatically creates the WorkspaceMembership record with that role; when the user is removed from the group, the membership is revoked. Unmapped groups have no membership effect. Idempotent: re-pushing the same membership does not duplicate records.

## Acceptance Criteria

- [ ] All six /scim/v2/Groups endpoints behave per RFC 7644 through the production router: list pagination, 404 on unknown id, create/replace/patch/delete with the Group schema, SCIM-token auth enforced on each.
- [ ] PUT /api/v1/workspaces/{id}/scim-mappings persists mappings [{idp_group, workspace_role}] (survive a server restart) and is rejected for non-admin callers.
- [ ] Adding user U to group G (via group PUT or PATCH) where G is mapped to role Developer in workspace W creates U's WorkspaceMembership in W with the Developer role — observable by U's subsequent workspace access at that role; removing U from G revokes that membership (the role-gated access subsequently fails).
- [ ] Group state (groups and memberships) is persisted in real storage behind a port with SQLite and PostgreSQL adapters, using the next unused migration sequence number.
- [ ] Adding a user to an unmapped group creates no WorkspaceMembership anywhere.

## Verification intent

Map group G to Developer in workspace W, then SCIM-push U into G and back out: after removal, U's role-gated workspace action (the one that succeeded while a member) must be denied — proving the 'membership is revoked' half of the behavior, not just the create half. Control case: push U into an unmapped group and assert no WorkspaceMembership row appears anywhere for U — proving mapping-driven effects rather than blanket membership creation. Pre-task, both probes trivially fail in the 'no effect' direction because no Group endpoint, mapping store, or membership effect exists at all (grep scim-mappings/ScimGroup: zero matches).

Update the relevant coverage rows only after implementing the behavior, with concrete source evidence. Do not mark unrelated rows complete or claim independent verification before it occurs.

## Discovery evidence

Canonical spec: `specs/system/identity-security.md`. Source: `fb1424fbac55cd23cb08606f4cd667a96f4dbc43`; plan: `8e132f42330e3bc1b1fb491a64d1396c0f3190a1457ecee8ef489f18b26cea9e`.

[
  {
    "id": "6273edccbcca379f21b54f97",
    "decision": "gap",
    "requirement_quote": "PUT /api/v1/workspaces/{id}/scim-mappings",
    "detail": "This feature is entirely absent. grep for 'scim-mappings', 'scim_mappings', and 'ScimMapping' across all of crates/ returns zero matches: no route registration, no request type, no handler, no mapping store. Consequently no IdP group push can create or revoke WorkspaceMembership records — there are also no SCIM Group endpoints to push to (see SCIM Endpoints section), so both halves of the behavior ('when a SCIM group push adds a user ... Gyre automatically creates a WorkspaceMembership record with Developer role' and 'when the user is removed from the group, the membership is revoked') have no implementation. The only IdP-to-role translation that exists is JWT-claim based: roles_from_claims maps Keycloak realm_access.roles to global UserRole values at login — global roles, not per-workspace membership driven by admin-configured group mappings.",
    "evidence": [
      {
        "file": "crates/gyre-server/src/api/mod.rs",
        "quote": "\"/api/v1/workspaces/:id/teams/:team_id/members/:user_id\",",
        "detail": "Absence evidence: the workspace route surface in api/mod.rs contains team-membership routes and every other workspace sub-resource, but no '/scim-mappings' route anywhere (verified by reading the full router and by zero grep hits for scim-mappings/ScimMapping across the repository). The admin-configurable per-workspace IdP-group-to-role mapping store and its automatic WorkspaceMembership effects do not exist."
      }
    ]
  }
]

## Shipped

Both halves of the gap are implemented in production code: checkpoint commit `eee34c67` (all six Groups endpoints, ports + SQLite/Postgres adapters, migration 000068, mapping surface, effect logic, integration test suite) completed by `70e1fa7b` (formatting, SQLite contract-test defect fix, adapter contract tests, coverage row 10, api-reference docs). This assignment rebased that source onto the task-263 completion base `00669baa` (merge commit `d007a8af`, resolving the identity-security.md coverage-file conflict: row 10 keeps this branch's task-266 implemented state, row 4 takes main's task-263 implemented state) and re-verified the result — evidence in /tmp/stage/review-evidence/task-266-rebase-evidence.md.

### SCIM Groups surface (requirement 1)

All six `/scim/v2/Groups` endpoints are registered in the production router (api/mod.rs) and implemented in api/scim.rs per RFC 7643 §4.2 / RFC 7644: GET list with `startIndex`/`count` pagination (1-based) and `displayName eq` / `externalId eq` filters; GET `{id}` with 404 on unknown id; POST create (idempotent on `externalId` — re-push returns the stored resource, never a duplicate row); PUT replace (`displayName` + the membership set exactly; `externalId` is the immutable join key); PATCH partial update (`add`/`remove`/`replace` on `members` including the `members[value eq "x"]` path-filter form, value lists, whole-attribute remove, and the simplified bare-body form); DELETE (runs the mapping-revocation effect for each member, then removes the group and membership rows, 204). Every endpoint enforces the dedicated `GYRE_SCIM_TOKEN` bearer via `check_scim_auth` (fail-closed 403 on unset token, missing header, or wrong bearer). Member `value` resolves to a Gyre user by user id, then externalId, then username; unresolvable members are skipped, never fabricated.

### Durable storage (requirement 4)

`gyre-ports::ScimGroupRepository` + `ScimMappingRepository`, implemented by BOTH the SQLite and PostgreSQL adapters over migration `2026-11-14-000068_scim_groups_and_mappings` (next unused 6-digit sequence; portable SQL — partial unique index on `scim_groups.external_id`, composite keys `(group_id, user_id)` and `(workspace_id, idp_group)`). Wired through `build_state`'s `store!` pattern — no in-memory AppState store; the mem-mode adapter guards the duplicate-external_id port contract in code.

### Mapping surface (requirement 2)

`PUT/GET /api/v1/workspaces/{id}/scim-mappings` (api/scim_mappings.rs): whole-set PUT validated before anything persists (unknown role, empty or duplicate `idp_group` → 400, never a partially-applied set), durable in `scim_workspace_mappings`. Authorization: registered in the ABAC `RouteResourceMapping` (resource `scim_mapping`) with the `builtin:scim-mapping-management` policy (workspace Owner/Admin), plus a per-handler `can_manage` guard — a plain Developer member gets 403, cross-tenant callers contained by `load_workspace`.

### Mapping effects (requirement 3 + control case)

`apply_member_added` / `apply_member_removed` (api/scim.rs) run from every membership-changing push (create with members, PUT, PATCH add/remove/replace, DELETE). Adding user U to a group whose `externalId` (else `displayName`) matches a workspace's mapping creates U's `WorkspaceMembership` with the mapped role — accepted on creation, tenant-contained, idempotent (never duplicated or rewritten). Removing U revokes the mapped membership — only the membership the mapping governs (a different-role grant is left intact). Unmapped groups create no membership anywhere. Every effect emits an audit event (`workspace_membership_created`/`workspace_membership_revoked`, `source: "scim"`).

### Test evidence

- This sandbox (post-rebase, /tmp/stage/review-evidence/task-266-rebase-evidence.md): `cargo check -p gyre-server --tests` clean — type-checks the merged task-263 auth changes alongside the full SCIM surface and both scim integration-test suites; `cargo test -p gyre-adapters sqlite::scim_group` — 5 passed; `cargo test -p gyre-domain --lib scim_group` — 1 passed; `cargo test -p gyre-spiffe` — 36 passed (crate touched by the rebase); `cargo fmt --all --check` clean; standing-invariant scripts pass (check-arch, check-abac-route-registry, check-abac-exempt-handlers, check-in-memory-state-stores, check-mem-port-contracts, check-migration-sql-portability, check-migration-versions).
- `tests/scim_groups.rs` (6 tests) drives every request through the full production `build_router()` stack over a real SQLite temp-file database: RFC 7644 CRUD + pagination + filters + 404s; SCIM-token enforcement on every endpoint; the verification-intent scenario (map G→Developer in W, push U into G — membership created and U's authenticated workspace access succeeds; remove U — the same access is denied); the unmapped control case (no WorkspaceMembership row anywhere); restart survival over the same db file; displayName-keyed mapping and cross-tenant containment.
- gyre-server test EXECUTION is deferred to the independent verifier (sandbox 4 GiB build cap blocks gyre-server codegen; the build wrapper records the restriction): the verifier must run `cargo test -p gyre-server --test scim_groups`, `cargo test -p gyre-server --test scim_lifecycle`, plus the adapter/domain suites above (both green here), and the full `cargo test --all`.
