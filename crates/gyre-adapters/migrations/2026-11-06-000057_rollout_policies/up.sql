-- Per-workspace rollout policies (meta-spec-reconciliation.md §7).
--
-- A workspace's rollout policy governs how reconciliation proceeds after a
-- meta-spec change (strategy: immediate / rolling / background, batch size,
-- reconciliation priority, auto_merge). Same persistence model as
-- meta_spec_sets: one JSON row per workspace; a missing row means the spec
-- default policy (Rolling / Normal / auto_merge false).
--
-- Portable SQL: TEXT / BIGINT with IF NOT EXISTS run on both SQLite and
-- PostgreSQL (the shared embedded migrations dir executes on both backends).

CREATE TABLE IF NOT EXISTS rollout_policies (
    workspace_id TEXT   NOT NULL PRIMARY KEY,
    json         TEXT   NOT NULL,
    updated_at   BIGINT NOT NULL
);
