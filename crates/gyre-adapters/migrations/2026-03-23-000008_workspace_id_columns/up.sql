-- M29.2: Add workspace_id to core entity tables
ALTER TABLE tasks ADD COLUMN workspace_id TEXT;
ALTER TABLE agents ADD COLUMN workspace_id TEXT;
ALTER TABLE projects ADD COLUMN workspace_id TEXT;
ALTER TABLE repositories ADD COLUMN workspace_id TEXT;
ALTER TABLE merge_requests ADD COLUMN workspace_id TEXT;
