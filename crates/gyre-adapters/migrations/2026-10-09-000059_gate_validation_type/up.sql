-- task-134 (agent-gates.md §Gate Types): AgentValidation gates carry a
-- domain-specific check identifier (implementation plan item 2: config
-- `{ validation_type, persona, required }`). Nullable — other gate types
-- and generic validators have none.
ALTER TABLE quality_gates ADD COLUMN validation_type TEXT;
