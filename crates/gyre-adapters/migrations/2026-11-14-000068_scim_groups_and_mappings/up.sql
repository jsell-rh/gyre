-- TASK-266: SCIM groups and group-to-workspace role mapping
-- (specs/system/identity-security.md §SCIM Provisioning /
-- §Group-to-Workspace Mapping).
--
-- IdP group pushes (POST/PUT/PATCH /scim/v2/Groups) are persisted here,
-- and the admin-configured per-workspace mapping
-- (PUT /api/v1/workspaces/{id}/scim-mappings) lives in
-- scim_workspace_mappings, keyed by (workspace_id, idp_group). When a
-- pushed group's membership changes, the server reconciles
-- WorkspaceMembership rows in every workspace that maps the group.
--
-- scim_groups.external_id is the RFC 7644 externalId (the opaque IdP
-- group identifier, e.g. a group DN) and the mapping join key. It is
-- nullable because some IdPs push no externalId — those groups join
-- mappings by displayName. The unique index is partial
-- (WHERE external_id IS NOT NULL) so any number of externalId-less
-- groups may coexist while IdP-keyed groups stay unique.
--
-- scim_group_members is keyed by (group_id, user_id): re-pushing the
-- same membership can never duplicate a row (task-266 idempotency).
--
-- scim_workspace_mappings is keyed by (workspace_id, idp_group): one
-- role per group per workspace; PUT replaces the workspace's whole set.
--
-- Portability: plain CREATE TABLE / CREATE INDEX with no dialect-only
-- syntax (partial indexes run on both SQLite and PostgreSQL), runnable
-- by both embedded-migration runners.

CREATE TABLE scim_groups (
    id TEXT PRIMARY KEY NOT NULL,
    external_id TEXT,
    display_name TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE UNIQUE INDEX idx_scim_groups_external_id
    ON scim_groups (external_id)
    WHERE external_id IS NOT NULL;

CREATE TABLE scim_group_members (
    group_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    added_at BIGINT NOT NULL,
    PRIMARY KEY (group_id, user_id)
);

CREATE TABLE scim_workspace_mappings (
    workspace_id TEXT NOT NULL,
    idp_group TEXT NOT NULL,
    workspace_role TEXT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (workspace_id, idp_group)
);
