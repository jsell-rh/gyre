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
    async fn delete(&self, id: &Id) -> Result<()>;
}
