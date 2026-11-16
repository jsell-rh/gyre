-- TASK-165: store the attestation level (supply-chain.md §Attestation
-- Levels) resolved from the agent's execution context at push time:
--   3 = Gyre-managed container (server-verified),
--   2 = registered stack (self-reported fingerprint),
--   1 = no stack attestation (raw push).
-- NULL until the agent's first push resolves it.

ALTER TABLE agents ADD COLUMN attestation_level INTEGER;
