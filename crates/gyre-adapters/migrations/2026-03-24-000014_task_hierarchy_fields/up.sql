-- M34 Slice 3: Non-optional hierarchy fields on Task, Agent, and MergeRequest.
--   - tasks:          workspace_id NOT NULL, add repo_id NOT NULL DEFAULT ''
--   - agents:         workspace_id NOT NULL DEFAULT 'default'
--   - merge_requests: workspace_id NOT NULL DEFAULT 'default'
--
-- SQLite doesn't support ALTER COLUMN, so we recreate the tables.

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

-- ── Agents ────────────────────────────────────────────────────────────────────

-- Step 3: Backfill agents.
UPDATE agents SET workspace_id = 'default' WHERE workspace_id IS NULL;

-- Step 4: Recreate agents with workspace_id NOT NULL.
CREATE TABLE agents_new (
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
    workspace_id          TEXT   NOT NULL DEFAULT 'default'
);

INSERT INTO agents_new
    SELECT id, name, status, parent_id, current_task_id, lifetime_budget_secs,
           spawned_at, last_heartbeat, tenant_id, spawned_by,
           COALESCE(workspace_id, 'default')
    FROM agents;

DROP TABLE agents;
ALTER TABLE agents_new RENAME TO agents;

-- ── MergeRequests ─────────────────────────────────────────────────────────────

-- Step 5: Backfill merge_requests.
UPDATE merge_requests SET workspace_id = 'default' WHERE workspace_id IS NULL;

-- Step 6: Recreate merge_requests with workspace_id NOT NULL.
CREATE TABLE merge_requests_new (
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
    workspace_id        TEXT   NOT NULL DEFAULT 'default'
);

INSERT INTO merge_requests_new
    SELECT id, repository_id, title, source_branch, target_branch, status,
           author_agent_id, reviewers, created_at, updated_at,
           diff_files_changed, diff_insertions, diff_deletions, has_conflicts,
           tenant_id, depends_on, atomic_group,
           COALESCE(workspace_id, 'default')
    FROM merge_requests;

DROP TABLE merge_requests;
ALTER TABLE merge_requests_new RENAME TO merge_requests;
