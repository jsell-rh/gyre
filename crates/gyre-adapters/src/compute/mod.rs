pub mod container;
pub mod docker;
pub mod kubernetes;
pub mod local;
pub mod ssh;

pub use container::ContainerTarget;
pub use docker::DockerTarget;
pub use kubernetes::KubernetesTarget;
pub use local::LocalTarget;
pub use ssh::{SshDockerTarget, SshTarget, SshTunnel, TunnelKind};

/// Whether a failed `<runtime> rm --force` stderr means the container does
/// not exist (already gone — a successful kill) as opposed to a real
/// failure (daemon unreachable, permission denied, wrong runtime, …).
/// `Error: No such container: abc123` is docker's/podman's already-gone
/// shape (review task-273: only this class may be treated as success;
/// every other non-zero exit must fail the kill so the caller does not
/// mark a live orphan terminated). Shared by the container and docker
/// targets.
pub(crate) fn container_already_gone(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    lower.contains("no such container")
}

#[cfg(test)]
mod tests {
    use super::container_already_gone;

    #[test]
    fn container_already_gone_matches_only_no_such_container() {
        // Already-gone shapes (successful kill).
        assert!(container_already_gone("Error: No such container: abc123"));
        assert!(container_already_gone(
            "Error response from daemon: No such container: deadbeef"
        ));
        // Real failures must NOT be treated as already-gone.
        assert!(!container_already_gone(
            "Cannot connect to the Docker daemon at unix:///var/run/docker.sock"
        ));
        assert!(!container_already_gone(
            "Error response from daemon: permission denied"
        ));
        assert!(!container_already_gone(""));
    }
}
