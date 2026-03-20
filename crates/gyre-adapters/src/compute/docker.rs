use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_ports::{ComputeTarget, ProcessHandle, SpawnConfig};
use tokio::process::Command;

/// Spawns processes inside Docker containers via the `docker run` CLI.
pub struct DockerTarget {
    /// Docker image to use when spawning containers.
    pub image: String,
}

impl DockerTarget {
    pub fn new(image: impl Into<String>) -> Self {
        Self {
            image: image.into(),
        }
    }
}

#[async_trait]
impl ComputeTarget for DockerTarget {
    async fn spawn_process(&self, config: &SpawnConfig) -> Result<ProcessHandle> {
        let mut cmd = Command::new("docker");
        cmd.arg("run")
            .arg("--detach")
            .arg("--rm")
            .arg(format!("--name={}", config.name))
            .arg(format!("--workdir={}", config.work_dir));

        for (k, v) in &config.env {
            cmd.arg(format!("--env={}={}", k, v));
        }

        cmd.arg(&self.image).arg(&config.command).args(&config.args);

        let output = cmd
            .output()
            .await
            .context("docker run failed — is Docker installed?")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("docker run failed: {}", stderr));
        }

        // docker run --detach prints the container ID on stdout
        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

        Ok(ProcessHandle {
            id: container_id,
            target_type: "docker".to_string(),
            pid: None,
        })
    }

    async fn kill_process(&self, handle: &ProcessHandle) -> Result<()> {
        let status = Command::new("docker")
            .arg("rm")
            .arg("--force")
            .arg(&handle.id)
            .status()
            .await
            .context("docker rm --force failed")?;

        if !status.success() {
            tracing::warn!(container_id = %handle.id, "docker rm --force returned non-zero");
        }
        Ok(())
    }

    async fn is_alive(&self, handle: &ProcessHandle) -> Result<bool> {
        let output = Command::new("docker")
            .args(["inspect", "--format={{.State.Running}}", &handle.id])
            .output()
            .await
            .context("docker inspect failed")?;

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

    /// Verify DockerTarget config parsing — does not require Docker in CI.
    #[test]
    fn docker_target_config_parsing() {
        let target = DockerTarget::new("ubuntu:22.04");
        assert_eq!(target.image, "ubuntu:22.04");
    }

    /// Full Docker integration test — requires Docker daemon.
    #[tokio::test]
    #[ignore = "requires Docker daemon"]
    async fn docker_spawn_is_alive_kill() {
        let target = DockerTarget::new("alpine:latest");
        let config = SpawnConfig {
            name: "gyre-test-container".to_string(),
            command: "sleep".to_string(),
            args: vec!["60".to_string()],
            env: HashMap::new(),
            work_dir: "/tmp".to_string(),
        };

        let handle = target.spawn_process(&config).await.unwrap();
        assert!(!handle.id.is_empty());
        assert_eq!(handle.target_type, "docker");

        let alive = target.is_alive(&handle).await.unwrap();
        assert!(alive);

        target.kill_process(&handle).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let alive_after = target.is_alive(&handle).await.unwrap();
        assert!(!alive_after);
    }
}
