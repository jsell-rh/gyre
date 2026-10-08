-- Revert TASK-120: drop the user-entity profile columns and username index.

DROP INDEX IF EXISTS idx_users_username;
ALTER TABLE users DROP COLUMN username;
ALTER TABLE users DROP COLUMN avatar_url;
ALTER TABLE users DROP COLUMN preferences;
ALTER TABLE users DROP COLUMN last_login_at;
ALTER TABLE users DROP COLUMN tenant_id;
