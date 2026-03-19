use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::{MergeQueueEntry, MergeQueueEntryStatus};

#[async_trait]
pub trait MergeQueueRepository: Send + Sync {
    async fn enqueue(&self, entry: &MergeQueueEntry) -> Result<()>;
    /// Return the highest-priority, oldest-enqueued pending entry.
    async fn next_pending(&self) -> Result<Option<MergeQueueEntry>>;
    async fn update_status(
        &self,
        id: &Id,
        status: MergeQueueEntryStatus,
        error: Option<String>,
    ) -> Result<()>;
    /// List all non-terminal entries ordered by priority desc, enqueued_at asc.
    async fn list_queue(&self) -> Result<Vec<MergeQueueEntry>>;
    async fn cancel(&self, id: &Id) -> Result<()>;
    async fn find_by_id(&self, id: &Id) -> Result<Option<MergeQueueEntry>>;
}
