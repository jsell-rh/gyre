-- Revert M34 Slice 3: drop repo_id, allow workspace_id to be nullable again.
CREATE TABLE tasks_old (
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
    workspace_id   TEXT,
    spec_path      TEXT
);

INSERT INTO tasks_old
    SELECT id, title, description, status, priority, assigned_to, parent_task_id,
           labels, branch, pr_link, created_at, updated_at, tenant_id,
           workspace_id, spec_path
    FROM tasks;

DROP TABLE tasks;
ALTER TABLE tasks_old RENAME TO tasks;
