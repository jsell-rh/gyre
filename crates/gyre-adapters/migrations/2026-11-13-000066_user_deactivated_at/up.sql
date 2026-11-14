-- Renumber note (task-110 rebase onto base 5984177): this migration
-- originally held version 20261113000063, which the task-110 candidate
-- branch also assigned to its `invitations` migration. Diesel runs only
-- one migration per version (alphabetically first wins), so on a fresh
-- database `users.deactivated_at` would silently never be created. The
-- task-110 version was already reconciled against earlier upstream
-- collisions (63/64/65), so this migration was renumbered to the next
-- unused sequence, 000066. Databases that already ran the pre-renumber
-- version are unaffected: Diesel records versions, not file contents.

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
