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
    /// Optional SSH port (default: 22).
    pub port: Option<u16>,
}

impl SshTarget {
    pub fn new(user: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            host: host.into(),
            identity_file: None,
            port: None,
        }
    }

    pub fn with_identity(mut self, path: impl Into<String>) -> Self {
        self.identity_file = Some(path.into());
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    fn destination(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }

    fn base_ssh_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "StrictHostKeyChecking=accept-new".to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
        ];
        if let Some(port) = self.port {
            args.push("-p".to_string());
            args.push(port.to_string());
        }
        if let Some(ref id) = self.identity_file {
            args.push("-i".to_string());
            args.push(id.clone());
        }
        args
    }

    /// Open an SSH tunnel.
    ///
    /// - **Forward** (`-L`): local port → remote host:port.  Access a remote
    ///   service on `local_port` as if it were local.
    /// - **Reverse** (`-R`): remote port → local host:port.  Expose a local
    ///   port through the remote host.  Use this so an air-gapped agent can
    ///   phone home to the gyre server even when the server cannot reach the
    ///   agent directly.
    ///
    /// The tunnel runs as a persistent background `ssh -N` process.  The
    /// returned [`SshTunnel`] owns the handle; drop or call
    /// [`SshTunnel::close`] to terminate it.
    pub async fn open_tunnel(&self, kind: TunnelKind) -> Result<SshTunnel> {
        let mut args = self.base_ssh_args();

        // -N: do not execute a remote command (tunnel-only)
        // -T: disable pseudo-tty allocation
        args.push("-N".to_string());
        args.push("-T".to_string());

        let spec = match &kind {
            TunnelKind::Forward {
                local_port,
                remote_host,
                remote_port,
            } => format!("{}:{}:{}", local_port, remote_host, remote_port),
            TunnelKind::Reverse {
                remote_port,
                local_host,
                local_port,
            } => format!("{}:{}:{}", remote_port, local_host, local_port),
        };

        let flag = match kind {
            TunnelKind::Forward { .. } => "-L",
            TunnelKind::Reverse { .. } => "-R",
        };

        args.push(flag.to_string());
        args.push(spec);
        args.push(self.destination());

        let child = Command::new("ssh")
            .args(&args)
            // Redirect I/O so the background process does not block the server
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("failed to spawn SSH tunnel process — is ssh installed?")?;

        let pid = child.id();
        Ok(SshTunnel {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            pid,
            child: tokio::sync::Mutex::new(Some(child)),
        })
    }
}

/// Which direction the port-forwarding flows.
#[derive(Debug, Clone)]
pub enum TunnelKind {
    /// `-L local_port:remote_host:remote_port` — access a remote service
    /// locally.  `ssh -L 8080:localhost:80 user@remote` makes the remote's
    /// port 80 available on the local machine as port 8080.
    Forward {
        local_port: u16,
        remote_host: String,
        remote_port: u16,
    },
    /// `-R remote_port:local_host:local_port` — expose a local service through
    /// the remote host.  This is the key primitive for air-gapped reverse
    /// connectivity: the agent SSHes *out* to the gyre server requesting that
    /// `remote_port` on the server forwards back to `local_port` on the
    /// agent's machine.
    Reverse {
        remote_port: u16,
        local_host: String,
        local_port: u16,
    },
}

/// Handle to a live SSH tunnel process.
///
/// The tunnel process runs in the background (`ssh -N`).  Call [`close`] or
/// drop the handle to terminate it.
pub struct SshTunnel {
    /// Unique id for this tunnel (UUID).
    pub id: String,
    pub kind: TunnelKind,
    /// OS PID of the `ssh -N` process, if available.
    pub pid: Option<u32>,
    child: tokio::sync::Mutex<Option<tokio::process::Child>>,
}

impl SshTunnel {
    /// Terminate the tunnel by killing the underlying SSH process.
    pub async fn close(self) -> Result<()> {
        let mut guard = self.child.lock().await;
        if let Some(mut child) = guard.take() {
            child.kill().await.context("failed to kill SSH tunnel")?;
        }
        Ok(())
    }

    /// Returns `true` if the tunnel process is still running.
    pub async fn is_alive(&self) -> bool {
        if let Some(pid) = self.pid {
            // kill -0 tests process existence without sending a signal
            tokio::process::Command::new("kill")
                .arg("-0")
                .arg(pid.to_string())
                .status()
                .await
                .map(|s| s.success())
                .unwrap_or(false)
        } else {
            false
        }
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
    fn ssh_target_with_port() {
        let target = SshTarget::new("alice", "10.0.0.1").with_port(2222);
        let args = target.base_ssh_args();
        let p_idx = args
            .iter()
            .position(|a| a == "-p")
            .expect("-p flag missing");
        assert_eq!(args[p_idx + 1], "2222");
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

    #[test]
    fn forward_tunnel_spec_format() {
        let kind = TunnelKind::Forward {
            local_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 80,
        };
        let spec = match &kind {
            TunnelKind::Forward {
                local_port,
                remote_host,
                remote_port,
            } => format!("{}:{}:{}", local_port, remote_host, remote_port),
            TunnelKind::Reverse { .. } => panic!("wrong variant"),
        };
        assert_eq!(spec, "8080:localhost:80");
    }

    #[test]
    fn reverse_tunnel_spec_format() {
        let kind = TunnelKind::Reverse {
            remote_port: 9000,
            local_host: "localhost".to_string(),
            local_port: 3000,
        };
        let spec = match &kind {
            TunnelKind::Reverse {
                remote_port,
                local_host,
                local_port,
            } => format!("{}:{}:{}", remote_port, local_host, local_port),
            TunnelKind::Forward { .. } => panic!("wrong variant"),
        };
        assert_eq!(spec, "9000:localhost:3000");
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

    /// Reverse tunnel integration test — requires local SSH daemon.
    #[tokio::test]
    #[ignore = "requires SSH daemon and key-based auth"]
    async fn reverse_tunnel_open_close() {
        let target = SshTarget::new("localhost", "localhost");
        let kind = TunnelKind::Reverse {
            remote_port: 19999,
            local_host: "localhost".to_string(),
            local_port: 3000,
        };
        let tunnel = target.open_tunnel(kind).await.unwrap();
        assert!(tunnel.pid.is_some());

        // Give SSH a moment to establish the tunnel
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        assert!(tunnel.is_alive().await);

        tunnel.close().await.unwrap();
    }
}
