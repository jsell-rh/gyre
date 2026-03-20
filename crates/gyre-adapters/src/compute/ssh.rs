use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_ports::{ComputeTarget, ProcessHandle, SpawnConfig};
use tokio::process::Command;

/// Spawns processes on remote hosts via SSH.
pub struct SshTarget {
    pub user: String,
    pub host: String,
    /// Optional SSH identity file path.
    pub identity_file: Option<String>,
}

impl SshTarget {
    pub fn new(user: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            host: host.into(),
            identity_file: None,
        }
    }

    pub fn with_identity(mut self, path: impl Into<String>) -> Self {
        self.identity_file = Some(path.into());
        self
    }

    fn destination(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }

    fn base_ssh_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
        ];
        if let Some(ref id) = self.identity_file {
            args.push("-i".to_string());
            args.push(id.clone());
        }
        args
    }
}

#[async_trait]
impl ComputeTarget for SshTarget {
    async fn spawn_process(&self, config: &SpawnConfig) -> Result<ProcessHandle> {
        // Build the remote command: env K=V ... cmd args... &; echo $!
        let mut remote_parts: Vec<String> = vec![];

        for (k, v) in &config.env {
            remote_parts.push(format!("{}={}", k, shell_quote(v)));
        }
        remote_parts.push(shell_quote(&config.command));
        for arg in &config.args {
            remote_parts.push(shell_quote(arg));
        }
        // Run in background and print PID
        let remote_cmd = format!(
            "cd {} && {} & echo $!",
            shell_quote(&config.work_dir),
            remote_parts.join(" ")
        );

        let mut ssh_args = self.base_ssh_args();
        ssh_args.push(self.destination());
        ssh_args.push(remote_cmd);

        let output = Command::new("ssh")
            .args(&ssh_args)
            .output()
            .await
            .context("ssh command failed — is SSH installed?")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("ssh spawn failed: {}", stderr));
        }

        let pid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let pid: u32 = pid_str
            .parse()
            .with_context(|| format!("ssh returned non-numeric PID: '{}'", pid_str))?;

        Ok(ProcessHandle {
            id: format!("{}:{}", self.destination(), pid),
            target_type: "ssh".to_string(),
            pid: Some(pid),
        })
    }

    async fn kill_process(&self, handle: &ProcessHandle) -> Result<()> {
        if let Some(pid) = handle.pid {
            let remote_cmd = format!("kill -TERM {}", pid);
            let mut ssh_args = self.base_ssh_args();
            ssh_args.push(self.destination());
            ssh_args.push(remote_cmd);

            let status = Command::new("ssh")
                .args(&ssh_args)
                .status()
                .await
                .context("ssh kill failed")?;

            if !status.success() {
                tracing::debug!(pid, host = %self.host, "remote kill returned non-zero");
            }
        }
        Ok(())
    }

    async fn is_alive(&self, handle: &ProcessHandle) -> Result<bool> {
        if let Some(pid) = handle.pid {
            let remote_cmd = format!("kill -0 {}", pid);
            let mut ssh_args = self.base_ssh_args();
            ssh_args.push(self.destination());
            ssh_args.push(remote_cmd);

            let status = Command::new("ssh")
                .args(&ssh_args)
                .status()
                .await
                .context("ssh is_alive check failed")?;

            Ok(status.success())
        } else {
            Ok(false)
        }
    }
}

/// Minimal shell quoting: wrap in single quotes, escape embedded single quotes.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_target_config_parsing() {
        let target =
            SshTarget::new("ubuntu", "192.168.1.10").with_identity("/home/user/.ssh/id_rsa");
        assert_eq!(target.user, "ubuntu");
        assert_eq!(target.host, "192.168.1.10");
        assert_eq!(
            target.identity_file.as_deref(),
            Some("/home/user/.ssh/id_rsa")
        );
        assert_eq!(target.destination(), "ubuntu@192.168.1.10");
    }

    #[test]
    fn shell_quote_simple() {
        assert_eq!(shell_quote("hello"), "'hello'");
    }

    #[test]
    fn shell_quote_with_single_quote() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn ssh_base_args_include_batch_mode() {
        let target = SshTarget::new("user", "host");
        let args = target.base_ssh_args();
        assert!(args.contains(&"BatchMode=yes".to_string()));
    }

    /// Full SSH integration test — requires SSH access to localhost.
    #[tokio::test]
    #[ignore = "requires SSH daemon and key-based auth"]
    async fn ssh_spawn_is_alive_kill() {
        use std::collections::HashMap;
        let target = SshTarget::new("localhost", "localhost");
        let config = SpawnConfig {
            name: "gyre-ssh-test".to_string(),
            command: "sleep".to_string(),
            args: vec!["60".to_string()],
            env: HashMap::new(),
            work_dir: "/tmp".to_string(),
        };

        let handle = target.spawn_process(&config).await.unwrap();
        assert!(handle.pid.is_some());
        assert_eq!(handle.target_type, "ssh");

        let alive = target.is_alive(&handle).await.unwrap();
        assert!(alive);

        target.kill_process(&handle).await.unwrap();
    }
}
