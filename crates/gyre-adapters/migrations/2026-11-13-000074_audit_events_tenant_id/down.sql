-- Revert TASK-273: drop audit-event tenant scoping.

DROP INDEX IF EXISTS idx_audit_events_tenant;
ALTER TABLE audit_events DROP COLUMN tenant_id;
