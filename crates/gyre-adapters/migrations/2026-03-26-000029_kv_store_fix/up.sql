-- Ensure kv_store and budget_usages tables exist on all deployments.
--
-- Previously filed as 2026-03-26-000026_kv_store_fix, which shared sequence number
-- 000026 with 2026-03-25-000026_interrogation_agents. Diesel's version_from_string
-- includes the date component, so these were technically distinct versions and both
-- ran — but having two migrations with the same NNNNNN sequence number is confusing
-- and error-prone. Re-issued as 000029 (next unused slot) for unambiguous ordering.
--
-- Idempotent: CREATE TABLE IF NOT EXISTS is safe to run on databases where
-- 000009_kv_store_and_budget_usage already created these tables.

CREATE TABLE IF NOT EXISTS kv_store (
    namespace   TEXT    NOT NULL,
    key         TEXT    NOT NULL,
    value_json  TEXT    NOT NULL,
    updated_at  BIGINT  NOT NULL,
    PRIMARY KEY (namespace, key)
);

CREATE TABLE IF NOT EXISTS budget_usages (
    entity_key          TEXT    NOT NULL PRIMARY KEY,
    entity_type         TEXT    NOT NULL,
    entity_id           TEXT    NOT NULL,
    tokens_used_today   BIGINT  NOT NULL DEFAULT 0,
    cost_today          DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    active_agents       INTEGER NOT NULL DEFAULT 0,
    period_start        BIGINT  NOT NULL,
    updated_at          BIGINT  NOT NULL
);
