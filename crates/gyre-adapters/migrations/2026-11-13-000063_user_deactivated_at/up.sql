-- TASK-124: user deactivation (user-management.md §API > User Management:
-- PUT /api/v1/users/{id}/deactivate, TenantAdmin only).
--
-- deactivated_at is NULL while the account is active and holds the Unix
-- second of the deactivation afterwards. Soft deactivation (not row
-- deletion) so the account, its audit trail, and its lingering workspace
-- memberships survive — workspace_ownership treats a deactivated Owner as
-- a departed Owner (the ownerless flow) precisely because the row remains.
--
-- Portability: plain ALTER TABLE ADD COLUMN with no dialect-only syntax,
-- runnable by both the SQLite and PostgreSQL embedded-migration runners.

ALTER TABLE users ADD COLUMN deactivated_at BIGINT;
