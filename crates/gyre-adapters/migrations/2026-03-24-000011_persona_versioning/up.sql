-- M VISION-3: Add versioning + approval fields to personas table.
ALTER TABLE personas ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE personas ADD COLUMN content_hash TEXT NOT NULL DEFAULT '';
ALTER TABLE personas ADD COLUMN owner TEXT;
ALTER TABLE personas ADD COLUMN approval_status TEXT NOT NULL DEFAULT 'Pending';
ALTER TABLE personas ADD COLUMN approved_by TEXT;
ALTER TABLE personas ADD COLUMN approved_at INTEGER;
ALTER TABLE personas ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
