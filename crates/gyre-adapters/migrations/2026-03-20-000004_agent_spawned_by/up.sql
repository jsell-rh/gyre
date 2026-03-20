-- M13.2: Add spawned_by to agents for commit provenance tracking.
-- Records the agent/user identity that spawned each agent.
ALTER TABLE agents ADD COLUMN spawned_by TEXT;
