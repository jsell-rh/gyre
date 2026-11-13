-- task-135 (agent-gates.md §Gate Chains, §Gate Agent Lifecycle):
--
-- 1. Ordered gate chains: repos configure gate chains as ORDERED policies.
--    `position` stores each gate's place in the repo's chain. The column
--    is created by the earlier task-168 migration
--    2026-10-08-000062_gate_position (version 20261008000062 runs before
--    this one), which also backfills existing rows to 0 with
--    (position, created_at) read ordering — pre-existing chains are
--    unchanged. This migration must NOT re-add the column: Diesel runs
--    every version exactly once, and a second ADD COLUMN fails on fresh
--    databases (duplicate column) while single-branch test databases that
--    never ran the task-168 migration stay green.
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

ALTER TABLE reviews ADD COLUMN reviewer_persona TEXT;
ALTER TABLE reviews ADD COLUMN gate_id TEXT;
