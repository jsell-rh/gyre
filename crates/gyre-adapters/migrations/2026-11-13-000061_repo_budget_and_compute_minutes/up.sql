-- TASK-119 (agent-runtime.md §4 Budget Enforcement):
--
-- 1. `repositories.budget_json` — repo-level budget override
--    (repo-lifecycle.md §3 "Budget | BudgetConfig | Inherited from workspace |
--    Repo-level budget override (cannot exceed workspace)"). Stored as a JSON
--    document of the BudgetConfig struct; NULL → inherit the workspace limit.
--
-- 2. `budget_usages.compute_minutes` — agent wall-clock compute time charged
--    to the entity, in whole minutes (spawn → complete/kill). Tracking-only:
--    no limit field consumes it, the budget API surfaces it.
--
-- Portability: this dir is embedded once and executed by BOTH SQLite and
-- PostgreSQL, so every statement is standard SQL. ADD COLUMN of a nullable
-- TEXT and of a NOT NULL INTEGER with a constant default are portable on
-- both backends; existing rows keep NULL (inherit) / 0 minutes.

ALTER TABLE repositories ADD COLUMN budget_json TEXT;

ALTER TABLE budget_usages ADD COLUMN compute_minutes INTEGER NOT NULL DEFAULT 0;
