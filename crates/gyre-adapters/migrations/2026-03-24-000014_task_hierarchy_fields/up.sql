-- M34 Slice 3: Make workspace_id NOT NULL and add repo_id to tasks.
--
-- SQLite doesn't support ALTER COLUMN, so we recreate the table.

-- Step 1: Backfill workspace_id for tasks that have none.
UPDATE tasks SET workspace_id = 'default' WHERE workspace_id IS NULL;

-- Step 2: Recreate tasks with workspace_id NOT NULL and new repo_id column.
CREATE TABLE tasks_new (
    id             TEXT   NOT NULL PRIMARY KEY,
    title          TEXT   NOT NULL,
    description    TEXT,
    status         TEXT   NOT NULL DEFAULT 'Backlog',
    priority       TEXT   NOT NULL DEFAULT 'Medium',
    assigned_to    TEXT,
    parent_task_id TEXT,
    labels         TEXT   NOT NULL DEFAULT '[]',
    branch         TEXT,
    pr_link        TEXT,
    created_at     BIGINT NOT NULL,
    updated_at     BIGINT NOT NULL,
    tenant_id      TEXT   NOT NULL DEFAULT 'default',
    workspace_id   TEXT   NOT NULL DEFAULT 'default',
    spec_path      TEXT,
    repo_id        TEXT   NOT NULL DEFAULT ''
);

INSERT INTO tasks_new
    SELECT id, title, description, status, priority, assigned_to, parent_task_id,
           labels, branch, pr_link, created_at, updated_at, tenant_id,
           COALESCE(workspace_id, 'default'), spec_path, ''
    FROM tasks;

DROP TABLE tasks;
ALTER TABLE tasks_new RENAME TO tasks;
