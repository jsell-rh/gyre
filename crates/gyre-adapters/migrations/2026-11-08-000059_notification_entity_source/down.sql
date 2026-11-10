-- Revert TASK-123: drop the notification entity source/read columns.

ALTER TABLE notifications DROP COLUMN entity_type;
ALTER TABLE notifications DROP COLUMN action_url;
ALTER TABLE notifications DROP COLUMN read;
ALTER TABLE notifications DROP COLUMN read_at;
