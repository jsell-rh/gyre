-- Migration 000060: converge stored users.preferences notification_channels
-- from the pre-task-112 flat shape to the user-management.md §Delivery
-- Channels nested shape.
--
-- task-112 changed NotificationChannels from
--   {"in_app":B,"email_enabled":B,"email_digest":"D"}
-- to
--   {"in_app":B,"email":{"enabled":B,"digest":"D","min_priority":"Medium"},
--    "webhook":null,"slack":null}
-- Rows written before that change -- including the 000058 default backfill
-- literal and every preferences update saved by pre-task-112 servers --
-- hold the flat shape. The domain type deserializes both shapes (see the
-- custom NotificationChannels Deserialize impl), but the stored rows are
-- converged here so the persisted shape matches the current wire shape.
--
-- Portability: plain REPLACE/LIKE string functions only -- this directory
-- is embedded once and executed by BOTH SQLite and PostgreSQL, and JSON1 /
-- jsonb functions are dialect-only (see
-- scripts/check-migration-sql-portability.sh).
--
-- The needle keeps the stored email_enabled / email_digest values (all 2x4
-- combinations; serde serializes in declaration order, so the fragment
-- '"email_enabled":B,"email_digest":"D"}' is exactly what every stored flat
-- row contains). min_priority did not exist in the flat shape, so migrated
-- rows take the EmailConfig default band (Medium). in_app and every sibling
-- preferences field are outside the needle and pass through untouched.
-- Each statement is a no-op on rows already in the nested shape (REPLACE
-- only rewrites when the needle matches; the LIKE guard skips them).

UPDATE users
SET preferences = REPLACE(preferences,
  '"email_enabled":false,"email_digest":"Immediate"}',
  '"email":{"enabled":false,"digest":"Immediate","min_priority":"Medium"},"webhook":null,"slack":null}')
WHERE preferences LIKE '%email_enabled%';

UPDATE users
SET preferences = REPLACE(preferences,
  '"email_enabled":false,"email_digest":"Hourly"}',
  '"email":{"enabled":false,"digest":"Hourly","min_priority":"Medium"},"webhook":null,"slack":null}')
WHERE preferences LIKE '%email_enabled%';

UPDATE users
SET preferences = REPLACE(preferences,
  '"email_enabled":false,"email_digest":"Daily"}',
  '"email":{"enabled":false,"digest":"Daily","min_priority":"Medium"},"webhook":null,"slack":null}')
WHERE preferences LIKE '%email_enabled%';

UPDATE users
SET preferences = REPLACE(preferences,
  '"email_enabled":false,"email_digest":"Off"}',
  '"email":{"enabled":false,"digest":"Off","min_priority":"Medium"},"webhook":null,"slack":null}')
WHERE preferences LIKE '%email_enabled%';

UPDATE users
SET preferences = REPLACE(preferences,
  '"email_enabled":true,"email_digest":"Immediate"}',
  '"email":{"enabled":true,"digest":"Immediate","min_priority":"Medium"},"webhook":null,"slack":null}')
WHERE preferences LIKE '%email_enabled%';

UPDATE users
SET preferences = REPLACE(preferences,
  '"email_enabled":true,"email_digest":"Hourly"}',
  '"email":{"enabled":true,"digest":"Hourly","min_priority":"Medium"},"webhook":null,"slack":null}')
WHERE preferences LIKE '%email_enabled%';

UPDATE users
SET preferences = REPLACE(preferences,
  '"email_enabled":true,"email_digest":"Daily"}',
  '"email":{"enabled":true,"digest":"Daily","min_priority":"Medium"},"webhook":null,"slack":null}')
WHERE preferences LIKE '%email_enabled%';

UPDATE users
SET preferences = REPLACE(preferences,
  '"email_enabled":true,"email_digest":"Off"}',
  '"email":{"enabled":true,"digest":"Off","min_priority":"Medium"},"webhook":null,"slack":null}')
WHERE preferences LIKE '%email_enabled%';
