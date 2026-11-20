---
title: "SCIM groups and group-to-workspace role mapping"
spec_ref: "Spec gap spec-31d3ae72e12e22c52442e309 / scim-groups-workspace-mapping / b60edce5bc49"
depends_on: [task-265]
progress: not-started
commits: ["eee34c67807e51536f21fd3bc3b8ec91604bf9a9"]
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
