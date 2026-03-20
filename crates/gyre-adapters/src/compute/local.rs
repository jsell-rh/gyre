use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_ports::{ComputeTarget, ProcessHandle, SpawnConfig};
use tokio::process::Command;

/// Spawns processes on the local machine using tokio::process::Command.
pub struct LocalTarget;

#[async_trait]
impl ComputeTarget for LocalTarget {
    async fn spawn_process(&self, config: &SpawnConfig) -> Result<ProcessHandle> {
        let child = Command::new(&config.command)
            .args(&config.args)
            .current_dir(&config.work_dir)
            .envs(&config.env)
            .spawn()
            .with_context(|| format!("failed to spawn '{}'", config.command))?;

        let pid = child.id();
        // Detach — caller manages lifecycle via kill_process / is_alive
        // We use pid as handle id since we no longer hold the Child.
        let id = pid
            .map(|p| p.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        Ok(ProcessHandle {
            id,
            target_type: "local".to_string(),
            pid,
        })
    }

    async fn kill_process(&self, handle: &ProcessHandle) -> Result<()> {
        if let Some(pid) = handle.pid {
            // Send SIGTERM via kill(1)
            let status = Command::new("kill")
                .arg("-TERM")
                .arg(pid.to_string())
                .status()
                .await
                .context("kill command failed")?;
            if !status.success() {
                // Process may have already exited; treat as success
                tracing::debug!(pid, "kill returned non-zero (process may have exited)");
            }
        }
        Ok(())
    }

    async fn is_alive(&self, handle: &ProcessHandle) -> Result<bool> {
        if let Some(pid) = handle.pid {
            // kill -0 checks process existence without sending a signal
            let status = Command::new("kill")
                .arg("-0")
                .arg(pid.to_string())
                .status()
                .await
                .context("kill -0 failed")?;
            Ok(status.success())
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn local_spawn_sleep_is_alive_kill() {
        let target = LocalTarget;
        let config = SpawnConfig {
            name: "test-sleep".to_string(),
            command: "sleep".to_string(),
            args: vec!["60".to_string()],
            env: HashMap::new(),
            work_dir: "/tmp".to_string(),
        };
        let handle = target.spawn_process(&config).await.unwrap();
        assert!(handle.pid.is_some());
        assert_eq!(handle.target_type, "local");

        let alive = target.is_alive(&handle).await.unwrap();
        assert!(alive, "process should be alive after spawn");

        target.kill_process(&handle).await.unwrap();

        // Give the OS a moment to reap
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let alive_after_kill = target.is_alive(&handle).await.unwrap();
        assert!(!alive_after_kill, "process should be dead after kill");
    }

    #[tokio::test]
    async fn local_spawn_short_lived_process() {
        let target = LocalTarget;
        let config = SpawnConfig {
            name: "echo-test".to_string(),
            command: "echo".to_string(),
            args: vec!["hello".to_string()],
            env: HashMap::new(),
            work_dir: "/tmp".to_string(),
        };
        let handle = target.spawn_process(&config).await.unwrap();
        assert_eq!(handle.target_type, "local");
        // No assertion on is_alive — process exits immediately, that's fine.
    }

    #[tokio::test]
    async fn local_spawn_bad_command_returns_error() {
        let target = LocalTarget;
        let config = SpawnConfig {
            name: "bad".to_string(),
            command: "/nonexistent/binary".to_string(),
            args: vec![],
            env: HashMap::new(),
            work_dir: "/tmp".to_string(),
        };
        let result = target.spawn_process(&config).await;
        assert!(result.is_err(), "spawning nonexistent binary should fail");
    }
}
