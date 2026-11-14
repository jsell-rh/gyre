-- Renumber note: renumbered from 20261108000061 to 20261113000064 at the
-- rebase onto main head f764bc00 (upstream spec_links owns version
-- 20261108000061). See up.sql.

-- Revert TASK-110 review F2: drop the pending-scope partial unique
-- indexes. Terminal-status dedupe rows written by the up.sql safety net
-- are audit history and stay.

DROP INDEX IF EXISTS idx_workspace_invitations_pending_scope;
DROP INDEX IF EXISTS idx_tenant_invitations_pending_scope;
