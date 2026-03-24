-- Revert M34 Slice 3: drop repo_id, allow workspace_id to be nullable again on all entities.
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

-- Revert agents
CREATE TABLE agents_old (
    id                    TEXT   NOT NULL PRIMARY KEY,
    name                  TEXT   NOT NULL,
    status                TEXT   NOT NULL DEFAULT 'Idle',
    parent_id             TEXT,
    current_task_id       TEXT,
    lifetime_budget_secs  BIGINT,
    spawned_at            BIGINT NOT NULL,
    last_heartbeat        BIGINT,
    tenant_id             TEXT   NOT NULL DEFAULT 'default',
    spawned_by            TEXT,
    workspace_id          TEXT
);

INSERT INTO agents_old
    SELECT id, name, status, parent_id, current_task_id, lifetime_budget_secs,
           spawned_at, last_heartbeat, tenant_id, spawned_by, workspace_id
    FROM agents;

DROP TABLE agents;
ALTER TABLE agents_old RENAME TO agents;

-- Revert merge_requests
CREATE TABLE merge_requests_old (
    id                  TEXT   NOT NULL PRIMARY KEY,
    repository_id       TEXT   NOT NULL,
    title               TEXT   NOT NULL,
    source_branch       TEXT   NOT NULL,
    target_branch       TEXT   NOT NULL,
    status              TEXT   NOT NULL DEFAULT 'Open',
    author_agent_id     TEXT,
    reviewers           TEXT   NOT NULL DEFAULT '[]',
    created_at          BIGINT NOT NULL,
    updated_at          BIGINT NOT NULL,
    diff_files_changed  BIGINT,
    diff_insertions     BIGINT,
    diff_deletions      BIGINT,
    has_conflicts       INTEGER,
    tenant_id           TEXT   NOT NULL DEFAULT 'default',
    depends_on          TEXT   NOT NULL DEFAULT '[]',
    atomic_group        TEXT,
    workspace_id        TEXT
);

INSERT INTO merge_requests_old
    SELECT id, repository_id, title, source_branch, target_branch, status,
           author_agent_id, reviewers, created_at, updated_at,
           diff_files_changed, diff_insertions, diff_deletions, has_conflicts,
           tenant_id, depends_on, atomic_group, workspace_id
    FROM merge_requests;

DROP TABLE merge_requests;
ALTER TABLE merge_requests_old RENAME TO merge_requests;
