//! Port trait for spec ledger entry persistence.

use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::SpecLedgerEntry;

#[async_trait]
pub trait SpecLedgerRepository: Send + Sync {
    async fn find_by_path(&self, path: &str) -> Result<Option<SpecLedgerEntry>>;
    async fn list_all(&self) -> Result<Vec<SpecLedgerEntry>>;
    async fn save(&self, entry: &SpecLedgerEntry) -> Result<()>;
    async fn delete_by_path(&self, path: &str) -> Result<()>;
}
