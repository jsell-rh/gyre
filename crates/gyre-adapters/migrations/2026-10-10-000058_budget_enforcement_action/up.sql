-- task-191: persist the per-level budget enforcement action
-- (platform-model.md §5 Enforcement Behavior).
--
-- NULL (the default) keeps the spec default behavior: 80% of a limit → Warn,
-- 100% → GracefulStop (BudgetExhausted message, 60s grace, then kill).
-- Non-NULL values are the snake_case names of the BudgetEnforcementAction
-- variants: warn, graceful_stop, hard_kill, queue_pause.
--
-- Portable SQL: plain ALTER TABLE ADD COLUMN runs identically on SQLite and
-- PostgreSQL (the shared embedded migrations dir executes on both backends).

ALTER TABLE budget_configs ADD COLUMN enforcement_action TEXT;
