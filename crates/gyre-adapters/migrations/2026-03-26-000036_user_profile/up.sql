-- Migration 000036: User Profile — notification preferences, API tokens, and profile fields.

CREATE TABLE IF NOT EXISTS user_notification_preferences (
    user_id TEXT NOT NULL,
    notification_type TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (user_id, notification_type)
);

CREATE TABLE IF NOT EXISTS user_tokens (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER,
    expires_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_user_tokens_user_id ON user_tokens(user_id);

-- Add profile fields to users table (SQLite: no IF NOT EXISTS on ADD COLUMN).
-- These columns are added individually; if migrating a fresh DB they are new.
ALTER TABLE users ADD COLUMN display_name TEXT;
ALTER TABLE users ADD COLUMN timezone TEXT;
ALTER TABLE users ADD COLUMN locale TEXT;
