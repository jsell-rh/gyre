-- Revert TASK-165: drop the push-resolved attestation level.

ALTER TABLE agents DROP COLUMN attestation_level;
