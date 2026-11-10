-- Revert TASK-110 review F2: drop the pending-scope partial unique
-- indexes. Terminal-status dedupe rows written by the up.sql safety net
-- are audit history and stay.

DROP INDEX IF EXISTS idx_workspace_invitations_pending_scope;
DROP INDEX IF EXISTS idx_tenant_invitations_pending_scope;
