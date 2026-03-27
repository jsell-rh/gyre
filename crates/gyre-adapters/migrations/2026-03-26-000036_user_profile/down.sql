-- Revert migration 000036.
DROP TABLE IF EXISTS user_notification_preferences;
DROP TABLE IF EXISTS user_tokens;
-- SQLite supports DROP COLUMN since 3.35.0 (2021-03-12).
ALTER TABLE users DROP COLUMN display_name;
ALTER TABLE users DROP COLUMN timezone;
ALTER TABLE users DROP COLUMN locale;
