-- Migration 000056: Notification delivery channel preferences (task-112,
-- user-management.md §Delivery Channels). One row per user holding the
-- serialized NotificationChannels config (email / webhook / slack).
CREATE TABLE IF NOT EXISTS user_channel_preferences (
    user_id TEXT PRIMARY KEY NOT NULL,
    channels TEXT NOT NULL,
    updated_at BIGINT NOT NULL
);
