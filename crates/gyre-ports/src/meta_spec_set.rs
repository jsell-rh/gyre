use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;

/// Repository for workspace meta-spec sets (M34 Slice 5).
///
/// Replaces the in-memory `Arc<Mutex<HashMap<...>>>` in `AppState` with a
/// proper persisted port, matching the persistence model of all other domain
/// entities.
///
/// The JSON payload is stored and retrieved as an opaque string — serialization
/// to `MetaSpecSet` happens in the server layer.
#[async_trait]
pub trait MetaSpecSetRepository: Send + Sync {
    /// Get the raw JSON for a workspace's meta-spec set, or `None` if not set.
    async fn get(&self, workspace_id: &Id) -> Result<Option<String>>;

    /// Upsert (insert or replace) the meta-spec set for a workspace.
    async fn upsert(&self, workspace_id: &Id, json: &str) -> Result<()>;

    /// Delete the meta-spec set for a workspace.
    async fn delete(&self, workspace_id: &Id) -> Result<()>;
}
