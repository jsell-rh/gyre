-- TASK-273 (admin-panel.md §Audit Log Viewer): tenant scoping for the
-- durable admin audit trail. tenant_id is derived server-side from the
-- acting identity at emit time — never client-supplied. Existing rows are
-- backfilled from their workspace scope (the workspaces table carries the
-- owning tenant); rows scoped solely by user (e.g. AuthSuccess/
-- AuthFailure events emitted without a workspace) are backfilled from
-- the users table, which carries the same tenant column the record-time
-- derivation uses — otherwise those legacy rows would stay NULL forever
-- and silently vanish from every tenant-scoped admin query. Rows whose
-- only principal is an agent (agent_id set, no workspace/user match) are
-- backfilled from the agents table, which carries tenant_id directly
-- (review task-273: without this third UPDATE those legacy rows would
-- stay NULL forever although record-time derivation now attributes
-- them). Rows with no resolvable workspace, user OR agent keep NULL and
-- are visible only to unscoped internal reads, never to a tenant-scoped
-- admin query.
--
-- Portability: plain ALTER TABLE + correlated UPDATE subqueries that run
-- identically on SQLite and PostgreSQL (no dialect-only functions).

ALTER TABLE audit_events ADD COLUMN tenant_id TEXT;

UPDATE audit_events
SET tenant_id = (
    SELECT w.tenant_id FROM workspaces w WHERE w.id = audit_events.workspace_id
)
WHERE tenant_id IS NULL AND workspace_id IS NOT NULL;

UPDATE audit_events
SET tenant_id = (
    SELECT u.tenant_id FROM users u WHERE u.id = audit_events.user_id
)
WHERE tenant_id IS NULL AND user_id IS NOT NULL;

UPDATE audit_events
SET tenant_id = (
    SELECT a.tenant_id FROM agents a WHERE a.id = audit_events.agent_id
)
WHERE tenant_id IS NULL AND agent_id IS NOT NULL;

CREATE INDEX idx_audit_events_tenant ON audit_events (tenant_id);
