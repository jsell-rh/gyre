-- Renumber note: renumbered from 20261010000058 to 20261113000063 at the
-- rebase onto main head f764bc00 (upstream budget_enforcement_action
-- owns version 20261010000058). See up.sql.

-- Revert TASK-110: drop the invitation tables.

DROP TABLE IF EXISTS workspace_invitations;
DROP TABLE IF EXISTS tenant_invitations;
