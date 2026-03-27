-- Spec approvals rejection fields
ALTER TABLE spec_approvals ADD COLUMN rejected_at INTEGER;
ALTER TABLE spec_approvals ADD COLUMN rejected_reason TEXT;
ALTER TABLE spec_approvals ADD COLUMN rejected_by TEXT;

-- Task cancellation fields
ALTER TABLE tasks ADD COLUMN cancelled_at INTEGER;
ALTER TABLE tasks ADD COLUMN cancelled_reason TEXT;

-- MR revert fields
ALTER TABLE merge_requests ADD COLUMN reverted_at INTEGER;
ALTER TABLE merge_requests ADD COLUMN revert_mr_id TEXT;
