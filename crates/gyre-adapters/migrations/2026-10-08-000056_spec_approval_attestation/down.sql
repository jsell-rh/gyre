-- Revert TASK-198: drop agent attestation capture from approval events.

ALTER TABLE spec_approval_events DROP COLUMN stack_hash;
ALTER TABLE spec_approval_events DROP COLUMN attestation_level;
