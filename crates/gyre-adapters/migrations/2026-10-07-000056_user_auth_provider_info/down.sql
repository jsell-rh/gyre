-- Revert TASK-209: drop the auth-provider columns.
-- SQLite 3.35+ supports DROP COLUMN.
ALTER TABLE users DROP COLUMN last_login_at;
ALTER TABLE users DROP COLUMN oidc_issuer;
