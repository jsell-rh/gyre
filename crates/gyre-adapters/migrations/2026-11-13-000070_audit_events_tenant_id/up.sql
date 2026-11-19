-- TASK-273 (admin-panel.md §Audit Log Viewer): tenant scoping for the
-- durable admin audit trail. tenant_id is derived server-side from the
-- acting identity at emit time — never client-supplied. Existing rows are
-- backfilled from their workspace scope (the workspaces table carries the
-- owning tenant); rows with no resolvable workspace keep NULL and are
-- visible only to unscoped internal reads, never to a tenant-scoped
-- admin query.
--
-- Portability: plain ALTER TABLE + a correlated UPDATE subquery that runs
-- identically on SQLite and PostgreSQL (no dialect-only functions).

ALTER TABLE audit_events ADD COLUMN tenant_id TEXT;

UPDATE audit_events
SET tenant_id = (
    SELECT w.tenant_id FROM workspaces w WHERE w.id = audit_events.workspace_id
)
WHERE tenant_id IS NULL AND workspace_id IS NOT NULL;

CREATE INDEX idx_audit_events_tenant ON audit_events (tenant_id);
