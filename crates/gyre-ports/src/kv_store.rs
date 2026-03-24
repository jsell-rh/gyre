use anyhow::Result;
use async_trait::async_trait;

/// Generic key-value JSON store port.
///
/// Stores arbitrary JSON-serialised values keyed by a (namespace, key) pair.
/// Used to persist server-internal HashMap stores (abac_policies, compute_targets,
/// agent_stacks, repo_stack_policies, workload_attestations, agent_cards,
/// agent_tokens, agent_messages, workspace_repos) to a backing database.
///
/// Values are stored as serialised JSON strings; callers handle
/// serde_json::to_string / from_str at the boundary.
#[async_trait]
pub trait KvJsonStore: Send + Sync {
    /// Insert or update `value` for `(namespace, key)`.
    async fn kv_set(&self, namespace: &str, key: &str, value: String) -> Result<()>;

    /// Retrieve the value for `(namespace, key)`. Returns `None` if absent.
    async fn kv_get(&self, namespace: &str, key: &str) -> Result<Option<String>>;

    /// Remove `(namespace, key)`. No-op if absent.
    async fn kv_remove(&self, namespace: &str, key: &str) -> Result<()>;

    /// List all `(key, value)` pairs in `namespace`.
    async fn kv_list(&self, namespace: &str) -> Result<Vec<(String, String)>>;

    /// Remove all entries in `namespace`.
    async fn kv_clear(&self, namespace: &str) -> Result<()>;
}
