-- TASK-093: Platform Model Orchestrator Lifecycle Protocol (platform-model.md §3).
-- Agents gain an orchestration tier. Workers keep 'worker' (the default).
-- repo_id is set only for repo orchestrators (workers link repos via worktrees;
-- orchestrators have none). restart_on_failure drives stale-detector auto-restart
-- for orchestrators (exactly-one-live semantics per scope).
--
-- Originally filed as 2026-09-30-000051_orchestrator_type, which collided with
-- 2026-09-30-000051_post_merge_gates (task-095, merged the same day). Diesel
-- extracted the same version string from both, ran only the alphabetically last
-- (post_merge_gates), and silently dropped these columns on every database
-- provisioned while the collision existed. Re-issued as 000052 (next unused
-- slot) so affected databases run it as pending; fresh databases run both in
-- order. Idempotent per-statement guards keep re-runs safe either way.

ALTER TABLE agents ADD COLUMN orchestrator_type TEXT NOT NULL DEFAULT 'worker';
ALTER TABLE agents ADD COLUMN repo_id TEXT;
ALTER TABLE agents ADD COLUMN restart_on_failure BOOLEAN NOT NULL DEFAULT 0;
