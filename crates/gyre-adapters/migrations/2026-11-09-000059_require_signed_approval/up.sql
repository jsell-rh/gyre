-- Forge enforcement policy `require_signed_approval` (agent-gates.md
-- §Forge Enforcement Policies): "Spec approvals must include Sigstore
-- signature." Opt-in per repo, default false.
--
-- INTEGER NOT NULL DEFAULT 0 runs on both SQLite and PostgreSQL
-- (scripts/check-migration-sql-portability.sh — shared embedded dir).

ALTER TABLE spec_policies ADD COLUMN require_signed_approval INTEGER NOT NULL DEFAULT 0;
