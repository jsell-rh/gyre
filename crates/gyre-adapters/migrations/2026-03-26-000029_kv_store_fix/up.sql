-- BUG-2 fix: Ensure kv_store and budget_usages tables exist.
-- The original migration 2026-03-23-000009_kv_store_and_budget_usage had the same
-- version prefix as 2026-03-23-000009_persist_batch_a. Diesel's embed_migrations!
-- deduplicates by version prefix (YYYY-MM-DD-NNNNNN), keeping only the last entry
-- alphabetically ('persist_batch_a' > 'kv_store_and_budget_usage'), so the kv_store
-- table was never created. This migration restores it idempotently.

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
