-- This down migration intentionally does NOT drop kv_store or budget_usages
-- because the original 000009_kv_store_and_budget_usage migration may have
-- created them. Dropping would destroy data.
SELECT 1;
