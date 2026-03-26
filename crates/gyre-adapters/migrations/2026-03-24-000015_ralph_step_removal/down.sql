-- Restore the ralph_step column (for rollback).
CREATE TABLE agent_commits_old (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    branch TEXT NOT NULL,
    timestamp BIGINT NOT NULL,
    task_id TEXT,
    ralph_step TEXT,
    spawned_by_user_id TEXT,
    parent_agent_id TEXT,
    model_context TEXT,
    attestation_level TEXT
);

INSERT INTO agent_commits_old
    (id, agent_id, repository_id, commit_sha, branch, timestamp,
     task_id, spawned_by_user_id, parent_agent_id, model_context, attestation_level)
SELECT id, agent_id, repository_id, commit_sha, branch, timestamp,
       task_id, spawned_by_user_id, parent_agent_id, model_context, attestation_level
FROM agent_commits;

DROP TABLE agent_commits;
ALTER TABLE agent_commits_old RENAME TO agent_commits;
