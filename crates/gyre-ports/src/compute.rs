use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

/// Configuration for spawning a process on a compute target.
#[derive(Debug, Clone)]
pub struct SpawnConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub work_dir: String,
}

/// Handle to a running process on a compute target.
#[derive(Debug, Clone)]
pub struct ProcessHandle {
    pub id: String,
    pub target_type: String,
    pub pid: Option<u32>,
    /// Target-specific connection details needed to reconstruct the compute
    /// target for out-of-band operations after the spawning target object is
    /// gone (e.g. killing the process from a registry lookup). SSH targets
    /// record `identity_file` and `port` here; other targets leave it empty.
    pub options: HashMap<String, String>,
}

/// Port for spawning and managing processes on compute targets.
///
/// Implementations: LocalTarget (tokio::process), DockerTarget (docker CLI),
/// SshTarget (ssh CLI).
#[async_trait]
pub trait ComputeTarget: Send + Sync {
    async fn spawn_process(&self, config: &SpawnConfig) -> Result<ProcessHandle>;
    async fn kill_process(&self, handle: &ProcessHandle) -> Result<()>;
    async fn is_alive(&self, handle: &ProcessHandle) -> Result<bool>;
}
