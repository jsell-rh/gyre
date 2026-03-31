-- Add task_type, order, and depends_on columns for the spec→orchestrator→task→agent signal chain.
-- task_type: NULL for pre-approval push-hook tasks, 'Implementation'/'Delegation'/'Coordination' for signal-chain tasks.
-- order: execution priority (lower = first). Tasks with the same order can run in parallel.
-- depends_on: JSON array of task IDs that must complete before this task starts.
ALTER TABLE tasks ADD COLUMN task_type TEXT;
ALTER TABLE tasks ADD COLUMN "order" INTEGER;
ALTER TABLE tasks ADD COLUMN depends_on TEXT NOT NULL DEFAULT '[]';
