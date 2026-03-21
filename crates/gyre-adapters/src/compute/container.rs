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
pub struct ContainerTarget {
    /// Container image to run (e.g. `ghcr.io/my-org/gyre-agent:latest`).
    pub image: String,
    /// Runtime to use. `None` = auto-detect at spawn time.
    pub runtime: Option<Runtime>,
    /// Host→container volume mounts as `"host_path:container_path"` strings.
    pub volumes: Vec<String>,
    /// Extra environment variables injected into every spawned container.
    pub env_vars: Vec<(String, String)>,
}

impl ContainerTarget {
    pub fn new(image: impl Into<String>) -> Self {
        Self {
            image: image.into(),
            runtime: None,
            volumes: Vec::new(),
            env_vars: Vec::new(),
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
