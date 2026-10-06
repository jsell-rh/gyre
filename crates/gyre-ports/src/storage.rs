use anyhow::Result;
use gyre_common::Id;

/// Storage port - abstracts persistence behind a trait.
/// Implementations (SQLite, PostgreSQL) live in gyre-adapters.
#[async_trait::async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait StoragePort: Send + Sync {
    async fn health_check(&self) -> Result<()>;
    /// Number of embedded migrations not yet applied to this database.
    /// 0 once startup migrations have completed; readiness probes gate on it.
    async fn migrations_pending(&self) -> Result<usize>;
}

/// Marker trait for repository ports.
pub trait Repository: Send + Sync {}

/// Generic CRUD port for aggregate roots.
#[async_trait::async_trait]
pub trait CrudPort<T>: Repository {
    async fn find_by_id(&self, id: &Id) -> Result<Option<T>>;
    async fn save(&self, entity: &T) -> Result<()>;
    async fn delete(&self, id: &Id) -> Result<()>;
}
