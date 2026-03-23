use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::NetworkPeer;

#[async_trait]
pub trait NetworkPeerRepository: Send + Sync {
    async fn register(&self, peer: &NetworkPeer) -> Result<()>;
    async fn list(&self) -> Result<Vec<NetworkPeer>>;
    async fn find_by_agent(&self, agent_id: &Id) -> Result<Option<NetworkPeer>>;
    async fn update_last_seen(&self, id: &Id, now: u64) -> Result<()>;
    async fn update_endpoint(&self, id: &Id, endpoint: &str) -> Result<()>;
    /// Mark peers whose `last_seen` is older than `cutoff` (unix seconds) as stale.
    /// Peers with `last_seen = None` are also marked stale if they were registered
    /// before the cutoff.
    async fn mark_stale_older_than(&self, cutoff: u64) -> Result<usize>;
    async fn delete(&self, id: &Id) -> Result<()>;
}
