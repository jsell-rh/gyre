-- Add immutable flag to policies table.
-- Immutable Deny policies are evaluated before all priority-based evaluation
-- and cannot be overridden by any Allow policy regardless of priority.
-- Used for builtin:require-human-spec-approval (HSI §2).
ALTER TABLE policies ADD COLUMN immutable INTEGER NOT NULL DEFAULT 0;
