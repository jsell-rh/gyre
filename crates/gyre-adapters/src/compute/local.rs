use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_ports::{ComputeTarget, ProcessHandle, SpawnConfig};
use std::collections::HashMap;
use tokio::process::Command;

/// Spawns processes on the local machine using tokio::process::Command.
pub struct LocalTarget;

#[async_trait]
impl ComputeTarget for LocalTarget {
    fn name(&self) -> &str {
        "local"
    }

    fn target_type(&self) -> &'static str {
        "local"
    }
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
            options: HashMap::new(),
        })
    }

    async fn kill_process(&self, handle: &ProcessHandle) -> Result<()> {
        let Some(pid) = handle.pid else {
            return Ok(());
        };
        // Already gone? A dead pid is a successful kill (idempotent —
        // the instance record is simply marked terminated).
        if !self.is_alive(handle).await? {
            return Ok(());
        }
        // SIGTERM, wait for exit, then SIGKILL, then VERIFY: the kill's
        // success is the process actually being gone — a non-zero kill
        // exit status or an ignored signal must never be reported as
        // success (review task-273: a failed kill treated as success let
        // "killed" instances keep running).
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status()
            .await; // verified below — a racing exit shows up in the poll
        if wait_for_exit(pid, std::time::Duration::from_secs(10)).await {
            return Ok(());
        }
        let status = Command::new("kill")
            .arg("-KILL")
            .arg(pid.to_string())
            .status()
            .await
            .with_context(|| format!("kill -KILL of {pid} failed to run"))?;
        if !status.success() && self.is_alive(handle).await? {
            anyhow::bail!("kill -KILL of {pid} failed");
        }
        if wait_for_exit(pid, std::time::Duration::from_secs(10)).await {
            return Ok(());
        }
        anyhow::bail!("process {pid} survived SIGTERM and SIGKILL — kill did not take effect");
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

/// Whether `pid` is gone (exited or an unreaped zombie — a zombie owns no
/// resources and never runs again, so it is dead for kill purposes).
/// Polls for up to `timeout`.
async fn wait_for_exit(pid: u32, timeout: std::time::Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if process_gone(pid) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

fn process_gone(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => {
            // State is field 3; comm (field 2) may contain ')' — parse
            // after the last one. 'Z' = zombie = dead for our purposes
            // (init reaps it; it will never run again).
            match stat.rsplit_once(')') {
                Some((_, after)) => after
                    .split_whitespace()
                    .next()
                    .map(|state| state.starts_with('Z'))
                    .unwrap_or(false),
                None => false,
            }
        }
        // /proc is unavailable (non-Linux): fall back to kill -0 via the
        // shell's exit status through `kill` (best-effort portability).
        Err(_) => {
            let gone = std::process::Command::new("kill")
                .arg("-0")
                .arg(pid.to_string())
                .status()
                .map(|s| !s.success())
                .unwrap_or(false);
            gone
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
    async fn local_kill_of_already_exited_pid_is_ok() {
        // Idempotent kill: a pid that already exited is a successful kill
        // (the caller marks the instance record terminated), never an
        // error and never a signal to a recycled pid — kill_process checks
        // liveness first (review task-273).
        let target = LocalTarget;
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id().expect("spawned child has a pid");
        let status = child.wait().await.unwrap();
        assert!(status.success());
        let handle = ProcessHandle {
            id: pid.to_string(),
            target_type: "local".to_string(),
            pid: Some(pid),
        };
        target.kill_process(&handle).await.unwrap();
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
