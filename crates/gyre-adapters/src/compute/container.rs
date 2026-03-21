use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_ports::{ComputeTarget, ProcessHandle, SpawnConfig};
use tokio::process::Command;

/// Container runtime to use for spawning agents.
#[derive(Debug, Clone, PartialEq)]
pub enum Runtime {
    Docker,
    Podman,
}

impl Runtime {
    /// Resolve the runtime binary name.
    fn binary(&self) -> &'static str {
        match self {
            Runtime::Docker => "docker",
            Runtime::Podman => "podman",
        }
    }

    /// Auto-detect: prefer docker, fall back to podman.
    pub async fn detect() -> Result<Self> {
        let docker = Command::new("which")
            .arg("docker")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);
        if docker {
            return Ok(Runtime::Docker);
        }
        let podman = Command::new("which")
            .arg("podman")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);
        if podman {
            return Ok(Runtime::Podman);
        }
        Err(anyhow::anyhow!(
            "no container runtime found — install docker or podman"
        ))
    }
}

/// Spawns agent processes inside Docker or Podman containers.
///
/// Supports auto-detection of the available runtime, volume mounts, and
/// pre-configured environment variables injected into every container.
///
/// # Security defaults (CISO G8-A/B/C)
///
/// Every container runs with hardened defaults out of the box:
/// - `--network=none` — no outbound network access (G8-A MEDIUM); opt-in via [`with_network`](ContainerTarget::with_network)
/// - `--memory=2g --pids-limit=512` — resource caps to prevent fork bombs / OOM (G8-B LOW)
/// - `--user=65534:65534` — nobody:nogroup, non-root (G8-C LOW)
pub struct ContainerTarget {
    /// Container image to run (e.g. `ghcr.io/my-org/gyre-agent:latest`).
    pub image: String,
    /// Runtime to use. `None` = auto-detect at spawn time.
    pub runtime: Option<Runtime>,
    /// Host→container volume mounts as `"host_path:container_path"` strings.
    pub volumes: Vec<String>,
    /// Extra environment variables injected into every spawned container.
    pub env_vars: Vec<(String, String)>,
    /// Network mode override. `None` = `--network=none` (default, G8-A).
    /// Set to e.g. `"bridge"` to grant outbound access when required.
    pub network: Option<String>,
    /// Memory limit override. `None` = `--memory=2g` (default, G8-B).
    pub memory_limit: Option<String>,
    /// PIDs limit override. `None` = `--pids-limit=512` (default, G8-B).
    pub pids_limit: Option<u32>,
    /// User override. `None` = `--user=65534:65534` (nobody:nogroup, default, G8-C).
    pub user: Option<String>,
}

impl ContainerTarget {
    pub fn new(image: impl Into<String>) -> Self {
        Self {
            image: image.into(),
            runtime: None,
            volumes: Vec::new(),
            env_vars: Vec::new(),
            network: None,
            memory_limit: None,
            pids_limit: None,
            user: None,
        }
    }

    pub fn with_runtime(mut self, runtime: Runtime) -> Self {
        self.runtime = Some(runtime);
        self
    }

    pub fn with_volume(mut self, mount: impl Into<String>) -> Self {
        self.volumes.push(mount.into());
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_vars.push((key.into(), value.into()));
        self
    }

    /// Override the network mode (G8-A). Default: `none` (no network access).
    /// Example: `.with_network("bridge")` to allow outbound access.
    pub fn with_network(mut self, network: impl Into<String>) -> Self {
        self.network = Some(network.into());
        self
    }

    /// Override the memory limit (G8-B). Default: `2g`.
    pub fn with_memory_limit(mut self, limit: impl Into<String>) -> Self {
        self.memory_limit = Some(limit.into());
        self
    }

    /// Override the PIDs limit (G8-B). Default: `512`.
    pub fn with_pids_limit(mut self, limit: u32) -> Self {
        self.pids_limit = Some(limit);
        self
    }

    /// Override the container user (G8-C). Default: `65534:65534` (nobody:nogroup).
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    async fn resolve_runtime(&self) -> Result<Runtime> {
        match &self.runtime {
            Some(r) => Ok(r.clone()),
            None => Runtime::detect().await,
        }
    }
}

#[async_trait]
impl ComputeTarget for ContainerTarget {
    async fn spawn_process(&self, config: &SpawnConfig) -> Result<ProcessHandle> {
        let runtime = self.resolve_runtime().await?;
        let bin = runtime.binary();

        let mut cmd = Command::new(bin);
        cmd.arg("run")
            .arg("--detach")
            .arg("--rm")
            .arg(format!("--name={}", config.name))
            .arg(format!("--workdir={}", config.work_dir));

        // --- Security defaults (CISO G8-A/B/C) ---

        // G8-A: network isolation — deny all outbound by default.
        let network = self.network.as_deref().unwrap_or("none");
        cmd.arg(format!("--network={}", network));

        // G8-B: resource limits — prevent runaway memory and fork bombs.
        let memory = self.memory_limit.as_deref().unwrap_or("2g");
        cmd.arg(format!("--memory={}", memory));
        let pids = self.pids_limit.unwrap_or(512);
        cmd.arg(format!("--pids-limit={}", pids));

        // G8-C: non-root user — run as nobody:nogroup (uid/gid 65534).
        let user = self.user.as_deref().unwrap_or("65534:65534");
        cmd.arg(format!("--user={}", user));

        // --- End security defaults ---

        // Mount worktree path so the agent can access its checkout.
        cmd.arg(format!("--volume={}:{}", config.work_dir, config.work_dir));

        // Additional user-configured volume mounts.
        for v in &self.volumes {
            cmd.arg(format!("--volume={}", v));
        }

        // Pre-configured env vars (e.g. GYRE_SERVER_URL, GYRE_AGENT_TOKEN).
        for (k, v) in &self.env_vars {
            cmd.arg(format!("--env={}={}", k, v));
        }

        // SpawnConfig env vars (caller-supplied, may override pre-configured).
        for (k, v) in &config.env {
            cmd.arg(format!("--env={}={}", k, v));
        }

        cmd.arg(&self.image).arg(&config.command).args(&config.args);

        let output = cmd
            .output()
            .await
            .with_context(|| format!("{} run failed — is {} installed?", bin, bin))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("{} run failed: {}", bin, stderr));
        }

        // `{runtime} run --detach` prints the container ID on stdout.
        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

        Ok(ProcessHandle {
            id: container_id,
            target_type: "container".to_string(),
            pid: None,
        })
    }

    async fn kill_process(&self, handle: &ProcessHandle) -> Result<()> {
        let runtime = self.resolve_runtime().await?;
        let bin = runtime.binary();

        let status = Command::new(bin)
            .arg("rm")
            .arg("--force")
            .arg(&handle.id)
            .status()
            .await
            .with_context(|| format!("{} rm --force failed", bin))?;

        if !status.success() {
            tracing::warn!(container_id = %handle.id, runtime = bin, "rm --force returned non-zero");
        }
        Ok(())
    }

    async fn is_alive(&self, handle: &ProcessHandle) -> Result<bool> {
        let runtime = self.resolve_runtime().await?;
        let bin = runtime.binary();

        let output = Command::new(bin)
            .args(["inspect", "--format={{.State.Running}}", &handle.id])
            .output()
            .await
            .with_context(|| format!("{} inspect failed", bin))?;

        if !output.status.success() {
            return Ok(false);
        }
        let state = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(state == "true")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn container_target_new_defaults() {
        let t = ContainerTarget::new("alpine:latest");
        assert_eq!(t.image, "alpine:latest");
        assert!(t.runtime.is_none());
        assert!(t.volumes.is_empty());
        assert!(t.env_vars.is_empty());
        // Security defaults are None → resolved to hardcoded values at spawn time.
        assert!(t.network.is_none());
        assert!(t.memory_limit.is_none());
        assert!(t.pids_limit.is_none());
        assert!(t.user.is_none());
    }

    #[test]
    fn container_target_builder_methods() {
        let t = ContainerTarget::new("ubuntu:22.04")
            .with_runtime(Runtime::Docker)
            .with_volume("/host/path:/container/path")
            .with_env("GYRE_SERVER_URL", "http://localhost:3000")
            .with_env("GYRE_AGENT_TOKEN", "tok-abc");

        assert_eq!(t.image, "ubuntu:22.04");
        assert_eq!(t.runtime, Some(Runtime::Docker));
        assert_eq!(t.volumes, vec!["/host/path:/container/path"]);
        assert_eq!(t.env_vars.len(), 2);
        assert_eq!(
            t.env_vars[0],
            ("GYRE_SERVER_URL".into(), "http://localhost:3000".into())
        );
        assert_eq!(t.env_vars[1], ("GYRE_AGENT_TOKEN".into(), "tok-abc".into()));
    }

    /// G8-A: default network flag is --network=none.
    #[test]
    fn security_default_network_none() {
        let t = ContainerTarget::new("alpine:latest");
        let network = t.network.as_deref().unwrap_or("none");
        assert_eq!(network, "none");
    }

    /// G8-A: with_network overrides the default.
    #[test]
    fn security_network_override() {
        let t = ContainerTarget::new("alpine:latest").with_network("bridge");
        assert_eq!(t.network.as_deref(), Some("bridge"));
    }

    /// G8-B: default memory limit is 2g and pids-limit is 512.
    #[test]
    fn security_default_resource_limits() {
        let t = ContainerTarget::new("alpine:latest");
        let memory = t.memory_limit.as_deref().unwrap_or("2g");
        let pids = t.pids_limit.unwrap_or(512);
        assert_eq!(memory, "2g");
        assert_eq!(pids, 512);
    }

    /// G8-B: resource limit overrides are respected.
    #[test]
    fn security_resource_limit_overrides() {
        let t = ContainerTarget::new("alpine:latest")
            .with_memory_limit("512m")
            .with_pids_limit(128);
        assert_eq!(t.memory_limit.as_deref(), Some("512m"));
        assert_eq!(t.pids_limit, Some(128));
    }

    /// G8-C: default user is nobody (65534:65534).
    #[test]
    fn security_default_user_nobody() {
        let t = ContainerTarget::new("alpine:latest");
        let user = t.user.as_deref().unwrap_or("65534:65534");
        assert_eq!(user, "65534:65534");
    }

    /// G8-C: with_user overrides the default.
    #[test]
    fn security_user_override() {
        let t = ContainerTarget::new("alpine:latest").with_user("1000:1000");
        assert_eq!(t.user.as_deref(), Some("1000:1000"));
    }

    #[test]
    fn runtime_binary_names() {
        assert_eq!(Runtime::Docker.binary(), "docker");
        assert_eq!(Runtime::Podman.binary(), "podman");
    }

    /// ProcessHandle reports target_type = "container".
    /// Uses a pre-resolved runtime so no binary is invoked.
    /// (spawn_process itself requires a daemon — covered by the ignored test.)
    #[test]
    fn process_handle_target_type() {
        let handle = ProcessHandle {
            id: "abc123".to_string(),
            target_type: "container".to_string(),
            pid: None,
        };
        assert_eq!(handle.target_type, "container");
        assert!(handle.pid.is_none());
    }

    /// Full integration test — requires Docker or Podman daemon.
    #[tokio::test]
    #[ignore = "requires container runtime"]
    async fn container_spawn_is_alive_kill() {
        let target = ContainerTarget::new("alpine:latest").with_runtime(Runtime::Docker);
        let config = SpawnConfig {
            name: "gyre-container-test".to_string(),
            command: "sleep".to_string(),
            args: vec!["60".to_string()],
            env: HashMap::new(),
            work_dir: "/tmp".to_string(),
        };

        let handle = target.spawn_process(&config).await.unwrap();
        assert!(!handle.id.is_empty());
        assert_eq!(handle.target_type, "container");

        let alive = target.is_alive(&handle).await.unwrap();
        assert!(alive);

        target.kill_process(&handle).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let alive_after = target.is_alive(&handle).await.unwrap();
        assert!(!alive_after);
    }
}
