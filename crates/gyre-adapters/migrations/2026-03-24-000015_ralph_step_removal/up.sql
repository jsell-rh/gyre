-- Remove the ralph_step column from agent_commits.
-- The RalphStep enum (Spec|Implement|Review|Merge) is incompatible with the Ralph loop
-- as defined in specs/system/agent-runtime.md. Provenance is now tracked via task_id +
-- task status history. See specs/system/agent-runtime.md §1.

-- SQLite does not support DROP COLUMN directly in older versions.
-- We recreate the table without the ralph_step column.
CREATE TABLE agent_commits_new (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    branch TEXT NOT NULL,
    timestamp BIGINT NOT NULL,
    task_id TEXT,
    spawned_by_user_id TEXT,
    parent_agent_id TEXT,
    model_context TEXT,
    attestation_level TEXT
);

INSERT INTO agent_commits_new
    (id, agent_id, repository_id, commit_sha, branch, timestamp,
     task_id, spawned_by_user_id, parent_agent_id, model_context, attestation_level)
SELECT id, agent_id, repository_id, commit_sha, branch, timestamp,
       task_id, spawned_by_user_id, parent_agent_id, model_context, attestation_level
FROM agent_commits;

DROP TABLE agent_commits;
ALTER TABLE agent_commits_new RENAME TO agent_commits;
