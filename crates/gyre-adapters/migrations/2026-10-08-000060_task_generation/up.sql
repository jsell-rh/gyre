-- TASK-188: Task.generation — monotonic deployment counter for SignedInput
-- context binding (authorization-provenance.md §2.4).
--
-- `SignedInput.expected_generation` optionally pins an authorization root to a
-- specific generation of the task's deployment (assignment). The counter must
-- be persisted so verification compares against the task's CURRENT generation,
-- incremented on every reassignment — otherwise expected_generation can never
-- be enforced and an old authorization stays replayable after reassignment.

ALTER TABLE tasks ADD COLUMN generation INTEGER NOT NULL DEFAULT 1;
