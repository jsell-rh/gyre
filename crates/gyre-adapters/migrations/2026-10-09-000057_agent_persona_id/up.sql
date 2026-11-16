-- TASK-099 F2: record the persona attached to an agent at spawn time
-- (platform-model.md §3 step 8: "Spawn repo orchestrator agent with
-- repo-orchestrator persona"). Previously the persona slug was validated
-- against the wrong store and never persisted on the agent row.

ALTER TABLE agents ADD COLUMN persona_id TEXT;
