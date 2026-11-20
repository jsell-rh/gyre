use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Durable record of a spawned agent instance on a compute target
/// (admin-panel.md §Background Jobs: "Remote agent cleanup
/// (expired/orphaned instances)").
///
/// Written at spawn with the backend descriptor and process handle, marked
/// terminated on clean exit/kill. The in-memory process_registry /
/// spawned_backends maps remain the fast path; this store is the source of
/// truth for orphan sweeps across server restarts — the
/// `remote_agent_cleanup` job kills still-recorded-running instances whose
/// agent is Dead/Stopped or absent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInstanceRecord {
    /// Agent the instance runs (row key: one live instance per agent).
    pub agent_id: String,
    /// Backend descriptor the instance was spawned on ("container",
    /// "ssh", "kubernetes", "local") — selects the kill path.
    pub backend: String,
    /// `ProcessHandle.target_type` recorded at spawn.
    pub target_type: String,
    /// `ProcessHandle.id` recorded at spawn (container id / pod name /
    /// process id string).
    pub handle_id: String,
    /// OS pid when known (local/SSH spawns).
    pub pid: Option<u32>,
    /// Unix epoch seconds of spawn.
    pub spawned_at: u64,
    /// Kill-reconstruction details for restart-orphans, as JSON
    /// (review task-273): the backend-specific connection facts needed to
    /// address the instance's process/container/pod again after the
    /// in-memory registries are gone — e.g. `{"runtime":"docker"}` for
    /// the container backend, `{"namespace":…,"context":…,
    /// "kubectl_path":…}` for Kubernetes. `None` when the backend needs
    /// nothing beyond the handle (local) or cannot be reconstructed (ssh:
    /// connection details are not persisted).
    #[serde(default)]
    pub backend_config: Option<String>,
    /// Unix epoch seconds the instance was marked terminated; `None` while
    /// recorded-running.
    pub terminated_at: Option<u64>,
    /// Why the instance was marked terminated ("clean_exit", "admin_kill",
    /// "remote_agent_cleanup", …).
    pub terminated_reason: Option<String>,
}

/// Durable agent-instance store backing the remote-agent-cleanup sweep.
#[async_trait]
#[allow(clippy::double_must_use)] // async_trait generates a must_use future for each method.
pub trait AgentInstanceRepository: Send + Sync {
    /// Record (or replace, for a respawn) the instance handle for `agent_id`.
    async fn record_spawned(&self, record: &AgentInstanceRecord) -> Result<()>;

    /// Mark the agent's instance terminated with `reason`. Fails if no
    /// record exists.
    async fn mark_terminated(&self, agent_id: &str, reason: &str) -> Result<()>;

    /// The agent's current instance record, if any.
    async fn find_by_agent(&self, agent_id: &str) -> Result<Option<AgentInstanceRecord>>;

    /// All instances not yet marked terminated (the sweep candidate set,
    /// including orphans left by a server restart).
    async fn list_running(&self) -> Result<Vec<AgentInstanceRecord>>;
}
