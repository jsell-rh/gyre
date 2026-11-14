-- Renumber note (rebase onto main head f764bc00): this migration
-- originally held version 20261108000061, which upstream main also
-- assigned to spec_links; Diesel would have silently dropped this index
-- migration on fresh databases. Renumbered to the next unused sequence,
-- 000064 — it still sorts after 000063 (the invitations tables it
-- indexes). Databases that already ran the pre-renumber version are
-- unaffected: Diesel records versions, not file contents.

-- TASK-110 review F2: the "one PENDING invitation per (tenant, email) /
-- (workspace, user)" port contract was enforced as check-then-insert in
-- the SQLite and PostgreSQL adapters, so two concurrent invites for the
-- same target could both pass the check and both insert, yielding two
-- pending invitations with distinct valid tokens. These partial unique
-- indexes make the database the arbiter: a second Pending row for the
-- same scope cannot exist regardless of adapter timing, and the adapters
-- map the resulting unique violation to the port-contract error.
-- Non-Pending (terminal) rows never conflict with the partial index, so
-- re-invite after Revoked/Expired/Declined keeps working with a fresh
-- token (user-management.md §Invitation Expiry).
--
-- Plain portable SQL only (task-102 F1): correlated EXISTS + UPDATE ...
-- AS alias run identically on SQLite and PostgreSQL.

-- Safety net for deployments that already raced past the check-then-insert
-- gap: mark every superseded duplicate Pending row (all but the newest
-- per scope, newest decided by created_at then id) Expired, so index
-- creation cannot fail on pre-existing duplicates.
UPDATE tenant_invitations AS t
SET status = 'Expired'
WHERE t.status = 'Pending'
  AND EXISTS (
      SELECT 1 FROM tenant_invitations AS newer
      WHERE newer.status = 'Pending'
        AND newer.tenant_id = t.tenant_id
        AND newer.email = t.email
        AND (newer.created_at > t.created_at
             OR (newer.created_at = t.created_at AND newer.id > t.id))
  );

UPDATE workspace_invitations AS t
SET status = 'Expired'
WHERE t.status = 'Pending'
  AND EXISTS (
      SELECT 1 FROM workspace_invitations AS newer
      WHERE newer.status = 'Pending'
        AND newer.workspace_id = t.workspace_id
        AND newer.user_id = t.user_id
        AND (newer.created_at > t.created_at
             OR (newer.created_at = t.created_at AND newer.id > t.id))
  );

CREATE UNIQUE INDEX idx_tenant_invitations_pending_scope
    ON tenant_invitations (tenant_id, email)
    WHERE status = 'Pending';

CREATE UNIQUE INDEX idx_workspace_invitations_pending_scope
    ON workspace_invitations (workspace_id, user_id)
    WHERE status = 'Pending';
