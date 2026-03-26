-- Revert persona versioning columns (SQLite doesn't support DROP COLUMN in older versions;
-- recreate the table without the added columns).
CREATE TABLE personas_old AS SELECT id, name, slug, scope, system_prompt, capabilities, protocols, model, temperature, max_tokens, budget, created_at FROM personas;
DROP TABLE personas;
ALTER TABLE personas_old RENAME TO personas;
