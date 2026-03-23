use anyhow::Result;
use async_trait::async_trait;
use gyre_domain::BudgetConfig;

#[async_trait]
pub trait BudgetRepository: Send + Sync {
    /// Upsert budget config for the given entity key (e.g. "workspace:{id}" or "tenant:{id}").
    async fn set_config(&self, entity_key: &str, config: &BudgetConfig) -> Result<()>;
    /// Retrieve budget config. Returns None if no config is set (use defaults).
    async fn get_config(&self, entity_key: &str) -> Result<Option<BudgetConfig>>;
    /// Delete budget config (revert to defaults).
    async fn delete_config(&self, entity_key: &str) -> Result<()>;
    /// List all budget configs.
    async fn list_all(&self) -> Result<Vec<(String, BudgetConfig)>>;
}
