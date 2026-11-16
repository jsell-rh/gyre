-- TASK-118 (agent-runtime.md §1 Phase 4 / Phase 7, repo-lifecycle.md §3):
-- per-repo agent execution limits.
--
-- `max_agents` — max concurrent worker agents in this repo. NULL falls back
-- to `workspaces.max_agents_per_repo`; both NULL → no limit. Validated on
-- PUT /api/v1/repos/:id so it can never exceed the workspace limit.
-- `max_agent_iterations` — Ralph-loop iteration ceiling for agents working
-- this repo (Phase 7 `max_iterations`). NULL → server default of 10.
--
-- Portability: this dir is embedded once and executed by BOTH SQLite and
-- PostgreSQL, so every statement is standard SQL. ADD COLUMN of a nullable
-- INTEGER with no default is portable on both backends; existing rows keep
-- NULL (inherit), which is the documented fallback semantics.

ALTER TABLE repositories ADD COLUMN max_agents INTEGER;
ALTER TABLE repositories ADD COLUMN max_agent_iterations INTEGER;
