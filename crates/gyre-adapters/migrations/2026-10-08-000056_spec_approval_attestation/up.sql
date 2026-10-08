-- TASK-198 (spec-registry.md §9 Approval Status Resolution): capture the
-- agent's attestation level and stack fingerprint on each approval event so
-- the mode-based resolver can evaluate agent-approval validity
-- (attestation_level >= min_attestation_level, stack_hash exact match).
-- Null for human approvals.

ALTER TABLE spec_approval_events ADD COLUMN attestation_level INTEGER;
ALTER TABLE spec_approval_events ADD COLUMN stack_hash TEXT;
