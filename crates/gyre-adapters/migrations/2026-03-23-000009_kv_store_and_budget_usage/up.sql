-- M29.5B: Generic key-value JSON store + budget usage persistence
-- Replaces in-memory HashMap stores for: abac_policies, compute_targets,
-- agent_stacks, repo_stack_policies, workload_attestations, agent_cards,
-- agent_tokens, agent_messages, workspace_repos.

CREATE TABLE IF NOT EXISTS kv_store (
    namespace   TEXT    NOT NULL,
    key         TEXT    NOT NULL,
    value_json  TEXT    NOT NULL,
    updated_at  BIGINT  NOT NULL,
    PRIMARY KEY (namespace, key)
);

-- Budget usage snapshots (real-time counters, reset daily).
-- Keyed by entity_key e.g. "workspace:{id}" or "tenant:default".
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
