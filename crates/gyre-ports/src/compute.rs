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
}

/// Port for spawning and managing processes on compute targets
/// (agent-runtime.md §3 Compute Target Model).
///
/// Implementations: LocalTarget (tokio::process), ContainerTarget
/// (docker/podman CLI), SshTarget (ssh CLI), KubernetesTarget (kubectl CLI).
#[async_trait]
pub trait ComputeTarget: Send + Sync {
    /// Human-readable backend name (used in logs and error messages).
    fn name(&self) -> &str;
    /// Backend type identifier: "local", "container", "ssh", "kubernetes".
    fn target_type(&self) -> &'static str;
    async fn spawn_process(&self, config: &SpawnConfig) -> Result<ProcessHandle>;
    async fn kill_process(&self, handle: &ProcessHandle) -> Result<()>;
    async fn is_alive(&self, handle: &ProcessHandle) -> Result<bool>;
}
