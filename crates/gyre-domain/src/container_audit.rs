//! Container audit record — domain type.

use serde::{Deserialize, Serialize};

/// Lifecycle record for a container that was spawned to run an agent process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerAuditRecord {
    /// The agent this container was spawned for.
    pub agent_id: String,
    /// Full container ID returned by `{runtime} run --detach`.
    pub container_id: String,
    /// Image that was used.
    pub image: String,
    /// SHA-256 image digest from `{runtime} inspect`. None when inspect fails.
    pub image_hash: Option<String>,
    /// Runtime that managed this container: `"docker"` or `"podman"`.
    pub runtime: String,
    /// Unix epoch when the container was started.
    pub started_at: u64,
    /// Unix epoch when the container was observed to have stopped.
    pub stopped_at: Option<u64>,
    /// Container exit code. `None` while still running.
    pub exit_code: Option<i32>,
}
