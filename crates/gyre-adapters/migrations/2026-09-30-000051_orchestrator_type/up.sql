-- TASK-093: Platform Model Orchestrator Lifecycle Protocol (platform-model.md §3).
-- Agents gain an orchestration tier. Workers keep 'worker' (the default).
-- repo_id is set only for repo orchestrators (workers link repos via worktrees;
-- orchestrators have none). restart_on_failure drives stale-detector auto-restart
-- for orchestrators (exactly-one-live semantics per scope).
ALTER TABLE agents ADD COLUMN orchestrator_type TEXT NOT NULL DEFAULT 'worker';
ALTER TABLE agents ADD COLUMN repo_id TEXT;
ALTER TABLE agents ADD COLUMN restart_on_failure BOOLEAN NOT NULL DEFAULT 0;
