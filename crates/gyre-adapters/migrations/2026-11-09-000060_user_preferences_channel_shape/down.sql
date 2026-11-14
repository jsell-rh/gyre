-- Revert migration 000060: rewrite nested notification_channels back to
-- the pre-task-112 flat shape. Only rows whose webhook/slack configs are
-- absent (null) are reversible -- a configured webhook/slack has no flat
-- representation and is left in the nested shape rather than silently
-- dropped. min_priority has no flat representation and is dropped on the
-- rows that do convert.

UPDATE users
SET preferences = REPLACE(preferences,
  '"email":{"enabled":false,"digest":"Immediate","min_priority":"Medium"},"webhook":null,"slack":null}',
  '"email_enabled":false,"email_digest":"Immediate"}')
WHERE preferences LIKE '%"email":%';

UPDATE users
SET preferences = REPLACE(preferences,
  '"email":{"enabled":false,"digest":"Hourly","min_priority":"Medium"},"webhook":null,"slack":null}',
  '"email_enabled":false,"email_digest":"Hourly"}')
WHERE preferences LIKE '%"email":%';

UPDATE users
SET preferences = REPLACE(preferences,
  '"email":{"enabled":false,"digest":"Daily","min_priority":"Medium"},"webhook":null,"slack":null}',
  '"email_enabled":false,"email_digest":"Daily"}')
WHERE preferences LIKE '%"email":%';

UPDATE users
SET preferences = REPLACE(preferences,
  '"email":{"enabled":false,"digest":"Off","min_priority":"Medium"},"webhook":null,"slack":null}',
  '"email_enabled":false,"email_digest":"Off"}')
WHERE preferences LIKE '%"email":%';

UPDATE users
SET preferences = REPLACE(preferences,
  '"email":{"enabled":true,"digest":"Immediate","min_priority":"Medium"},"webhook":null,"slack":null}',
  '"email_enabled":true,"email_digest":"Immediate"}')
WHERE preferences LIKE '%"email":%';

UPDATE users
SET preferences = REPLACE(preferences,
  '"email":{"enabled":true,"digest":"Hourly","min_priority":"Medium"},"webhook":null,"slack":null}',
  '"email_enabled":true,"email_digest":"Hourly"}')
WHERE preferences LIKE '%"email":%';

UPDATE users
SET preferences = REPLACE(preferences,
  '"email":{"enabled":true,"digest":"Daily","min_priority":"Medium"},"webhook":null,"slack":null}',
  '"email_enabled":true,"email_digest":"Daily"}')
WHERE preferences LIKE '%"email":%';

UPDATE users
SET preferences = REPLACE(preferences,
  '"email":{"enabled":true,"digest":"Off","min_priority":"Medium"},"webhook":null,"slack":null}',
  '"email_enabled":true,"email_digest":"Off"}')
WHERE preferences LIKE '%"email":%';
