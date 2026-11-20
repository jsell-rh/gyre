-- Revert TASK-266 repair: drop the member display column.
-- SQLite >= 3.35 supports DROP COLUMN; the bundled system SQLite qualifies
-- (same note as migration 000062).

ALTER TABLE scim_group_members DROP COLUMN display;
