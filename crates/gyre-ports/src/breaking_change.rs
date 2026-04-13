//! Port trait for breaking change tracking.

use anyhow::Result;
use async_trait::async_trait;
use gyre_common::Id;
use gyre_domain::BreakingChange;

/// Port for persisting and querying breaking change records.
#[async_trait]
pub trait BreakingChangeRepository: Send + Sync {
    /// Persist a new breaking change record.
    async fn create(&self, bc: &BreakingChange) -> Result<()>;

    /// Fetch a single record by ID.
    async fn find_by_id(&self, id: &Id) -> Result<Option<BreakingChange>>;

    /// All unacknowledged breaking changes (tenant-wide).
    async fn list_unacknowledged(&self) -> Result<Vec<BreakingChange>>;

    /// All breaking changes for a specific target repo (the repo that introduced the change).
    async fn list_by_source_repo(&self, source_repo_id: &Id) -> Result<Vec<BreakingChange>>;

    /// Mark a breaking change as acknowledged.
    /// Returns `true` if the record existed and was updated.
    async fn acknowledge(&self, id: &Id, acknowledged_by: &str, at: u64) -> Result<bool>;
}
