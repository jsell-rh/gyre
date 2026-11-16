-- Revert TASK-209: drop the auth-provider column.
-- SQLite 3.35+ supports DROP COLUMN.
-- `last_login_at` is owned by 2026-10-08-000058_user_entity_profile_fields
-- (task-120) and is dropped by that migration's down.sql, not here.
ALTER TABLE users DROP COLUMN oidc_issuer;
