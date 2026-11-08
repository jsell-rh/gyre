-- Revert: drop the per-level enforcement action override.

ALTER TABLE budget_configs DROP COLUMN enforcement_action;
