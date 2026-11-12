-- task-135 (agent-gates.md §Gate Chains, §Gate Agent Lifecycle):
--
-- 1. Ordered gate chains: repos configure gate chains as ORDERED policies.
--    `position` stores each gate's place in the repo's chain. Existing rows
--    default to 0 and keep their creation-order tiebreak (readers sort by
--    position, then created_at), so pre-existing chains are unchanged.
--
-- 2. Review attribution: a gate agent's review must show which gate persona
--    produced it. `reviews.reviewer_persona` carries the gate's configured
--    persona and `reviews.gate_id` binds the review to the gate chain entry
--    that spawned the reviewer. Both nullable — human/agent reviews that
--    were not produced by a gate have no attribution.
--
-- Portability: this dir is embedded once and executed by BOTH SQLite and
-- PostgreSQL, so every statement is standard SQL. ADD COLUMN with DEFAULT
-- is portable on both backends; no dialect-only functions are used.

ALTER TABLE quality_gates ADD COLUMN position INTEGER NOT NULL DEFAULT 0;

ALTER TABLE reviews ADD COLUMN reviewer_persona TEXT;
ALTER TABLE reviews ADD COLUMN gate_id TEXT;
