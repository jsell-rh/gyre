-- TASK-123: notification entity per user-management.md §Notification Entity.
--
-- Adds the structured source discriminator (`entity_type` — "agent", "mr",
-- "spec", "gate", "budget", "workspace", "system"; `entity_ref`/`repo_id`
-- already store the source entity id and repo scope), the deep-link
-- `action_url`, and the in-app read state (`read`, `read_at`).
--
-- Portability: this dir is embedded once and executed by BOTH SQLite and
-- PostgreSQL, so every statement below is standard SQL. ADD COLUMN with
-- DEFAULT and plain UPDATE are portable on both backends; no dialect-only
-- functions are used. (`read` is not a reserved word in either dialect, and
-- the query builders quote it regardless.)
--
-- `entity_type` stays NULL for rows written before this migration: their
-- source entity id survives in `entity_ref`, but its kind was never
-- recorded, and fabricating one would mis-target deep links. Readers map
-- NULL to the generic "system" source kind.
--
-- Rows already acted on (resolved or dismissed) were seen by their user:
-- backfill the read state from the action timestamps.

ALTER TABLE notifications ADD COLUMN entity_type TEXT;
ALTER TABLE notifications ADD COLUMN action_url TEXT;
ALTER TABLE notifications ADD COLUMN read INTEGER NOT NULL DEFAULT 0;
ALTER TABLE notifications ADD COLUMN read_at BIGINT;

UPDATE notifications
   SET read = 1,
       read_at = COALESCE(resolved_at, dismissed_at)
 WHERE (resolved_at IS NOT NULL OR dismissed_at IS NOT NULL)
   AND read = 0;
