use gyre_common::Id;
use serde::{Deserialize, Serialize};

/// An ephemeral preview environment provisioned from a merged merge request
/// (activity-dashboard.md §"Try it" links): a short-lived, isolated runtime
/// executing the exact commit that landed the MR's work, reachable by the
/// requesting user over the platform's WireGuard mesh.
///
/// This is the platform's realization of the spec's "NixOS + Tailscale"
/// ephemeral environment: the container image is built from the NixOS
/// flake-based image path (`docker/gyre-agent/flake.nix`) and network
/// reachability rides the WireGuard coordination plane (mesh IP allocation
/// + peer registration) instead of a third-party overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewEnvironment {
    pub id: Id,
    /// Owning tenant — derived server-side from the MR's workspace, never
    /// caller-supplied.
    pub tenant_id: Id,
    /// Workspace that governs the source MR (ABAC boundary).
    pub workspace_id: Id,
    /// Repository the MR landed in.
    pub repo_id: Id,
    /// The merged merge request this environment runs.
    pub mr_id: Id,
    /// The user (or agent) that requested the environment — the principal
    /// the mesh peer is provisioned for.
    pub requested_by: Id,
    /// Resolved commit SHA the environment checks out and runs (the MR's
    /// merge commit). Recorded at provisioning so the row is a durable
    /// statement of "exactly this version of the code".
    pub commit_sha: String,
    /// The branch the commit was resolved on (MR target branch) — carried
    /// so feed entries and teardown messages can reference it.
    pub branch: String,
    /// Lifecycle status: provisioning → active → expired/failed.
    pub status: PreviewEnvironmentStatus,
    /// Mesh IP allocated from the WireGuard CIDR pool for this environment.
    /// `None` when WireGuard is disabled on the deployment.
    pub mesh_ip: Option<String>,
    /// The registered `NetworkPeer` id (peer removal on teardown/expiry).
    pub peer_id: Option<Id>,
    /// The ephemeral agent row that owns the dispatched process (kill
    /// routing rides the standard process registry).
    pub agent_id: Id,
    /// Compute-target label the environment was dispatched to
    /// ("container"/"ssh"/"kubernetes"/"local").
    pub compute_target: String,
    /// Container image the environment runs (the NixOS flake-built image
    /// tag for container targets).
    pub image: Option<String>,
    /// Isolated scratch state dir — the environment's own temp data dir.
    /// It must never point at the production database/storage.
    pub data_dir: String,
    /// Port the environment serves the preview on.
    pub port: u16,
    /// Serialized [`gyre_ports::ProcessHandle`] JSON — the durable handle
    /// so teardown/TTL can kill the process even after a server restart
    /// emptied the in-memory process registry.
    pub process_handle: Option<String>,
    /// Unix seconds.
    pub created_at: u64,
    /// Unix seconds — TTL deadline; the expiry job tears the environment
    /// down at or after this point.
    pub expires_at: u64,
    /// Unix seconds — when the environment left the active states.
    pub ended_at: Option<u64>,
    /// Why the environment ended ("torn-down", "expired", "failed") or the
    /// provisioning failure reason.
    pub end_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewEnvironmentStatus {
    /// Provisioning dispatched; the process has not been confirmed running.
    Provisioning,
    /// Running on its compute target, reachable over the mesh.
    Active,
    /// Terminated (explicit teardown or TTL expiry).
    Expired,
    /// Provisioning failed or the process died before activation.
    Failed,
}

impl PreviewEnvironmentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            PreviewEnvironmentStatus::Provisioning => "provisioning",
            PreviewEnvironmentStatus::Active => "active",
            PreviewEnvironmentStatus::Expired => "expired",
            PreviewEnvironmentStatus::Failed => "failed",
        }
    }

    /// Whether the environment still holds live resources (a process and a
    /// mesh peer) that teardown must reclaim.
    pub fn is_live(self) -> bool {
        matches!(
            self,
            PreviewEnvironmentStatus::Provisioning | PreviewEnvironmentStatus::Active
        )
    }
}

impl PreviewEnvironment {
    #[allow(clippy::too_many_arguments)] // provisioning carries the full resolved scope
    pub fn new(
        id: Id,
        tenant_id: Id,
        workspace_id: Id,
        repo_id: Id,
        mr_id: Id,
        requested_by: Id,
        commit_sha: impl Into<String>,
        branch: impl Into<String>,
        agent_id: Id,
        compute_target: impl Into<String>,
        data_dir: impl Into<String>,
        port: u16,
        created_at: u64,
        expires_at: u64,
    ) -> Self {
        Self {
            id,
            tenant_id,
            workspace_id,
            repo_id,
            mr_id,
            requested_by,
            commit_sha: commit_sha.into(),
            branch: branch.into(),
            status: PreviewEnvironmentStatus::Provisioning,
            mesh_ip: None,
            peer_id: None,
            agent_id,
            compute_target: compute_target.into(),
            image: None,
            data_dir: data_dir.into(),
            port,
            process_handle: None,
            created_at,
            expires_at,
            ended_at: None,
            end_reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_env() -> PreviewEnvironment {
        PreviewEnvironment::new(
            Id::new("pe-1"),
            Id::new("t-1"),
            Id::new("ws-1"),
            Id::new("repo-1"),
            Id::new("mr-1"),
            Id::new("user-1"),
            "0123456789abcdef0123456789abcdef01234567",
            "main",
            Id::new("agent-1"),
            "container",
            "/tmp/gyre-preview-pe-1",
            18421,
            1000,
            4600,
        )
    }

    #[test]
    fn new_env_starts_provisioning() {
        let e = make_env();
        assert_eq!(e.status, PreviewEnvironmentStatus::Provisioning);
        assert!(e.status.is_live());
        assert!(e.mesh_ip.is_none());
        assert!(e.peer_id.is_none());
        assert!(e.ended_at.is_none());
    }

    #[test]
    fn expired_and_failed_are_not_live() {
        assert!(!PreviewEnvironmentStatus::Expired.is_live());
        assert!(!PreviewEnvironmentStatus::Failed.is_live());
        assert!(PreviewEnvironmentStatus::Active.is_live());
    }

    #[test]
    fn status_strings_are_snake_case() {
        assert_eq!(
            PreviewEnvironmentStatus::Provisioning.as_str(),
            "provisioning"
        );
        assert_eq!(PreviewEnvironmentStatus::Active.as_str(), "active");
        assert_eq!(PreviewEnvironmentStatus::Expired.as_str(), "expired");
        assert_eq!(PreviewEnvironmentStatus::Failed.as_str(), "failed");
    }
}
