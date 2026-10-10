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
    fn name(&self) -> &str {
        "ssh"
    }

    fn target_type(&self) -> &'static str {
        "ssh"
    }

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

// ---------------------------------------------------------------------------
// Remote Docker execution (agent-runtime.md §3 Supported Backends — SSH)
// ---------------------------------------------------------------------------

/// Spawns agent containers on a remote host via SSH: `ssh user@host docker run`.
///
/// The spec's SSH backend is "SSH to remote host, `docker run` there" — the
/// container runs on the remote machine, orchestrated from the server over
/// SSH. Unlike [`SshTarget`] (bare remote processes) this tracks the remote
/// *container*, not the short-lived `docker` client process: `--detach`
/// returns once the container starts, and the container ID printed on stdout
/// becomes the [`ProcessHandle`] id. `kill_process` runs `docker rm --force`
/// on the remote host; `is_alive` runs `docker inspect` there.
///
/// Security defaults match the container backend (spec §3):
/// `--network=none`, `--memory=2g`, `--pids-limit=512`, `--user=65534:65534`.
pub struct SshDockerTarget {
    /// SSH user@host connection (credentials live here).
    pub ssh: SshTarget,
    /// Agent image to run on the remote host.
    pub image: String,
    /// Network mode override. `None` = `--network=none` (spec default).
    pub network: Option<String>,
    /// Memory limit override. `None` = `--memory=2g` (spec default).
    pub memory_limit: Option<String>,
    /// PIDs limit override. `None` = `--pids-limit=512` (spec default).
    pub pids_limit: Option<u32>,
    /// User override. `None` = `--user=65534:65534` (spec default).
    pub user: Option<String>,
    /// Remote docker binary (default `docker`).
    pub docker_binary: String,
}

impl SshDockerTarget {
    pub fn new(ssh: SshTarget, image: impl Into<String>) -> Self {
        Self {
            ssh,
            image: image.into(),
            network: None,
            memory_limit: None,
            pids_limit: None,
            user: None,
            docker_binary: "docker".to_string(),
        }
    }

    pub fn with_network(mut self, network: impl Into<String>) -> Self {
        self.network = Some(network.into());
        self
    }

    pub fn with_memory_limit(mut self, limit: impl Into<String>) -> Self {
        self.memory_limit = Some(limit.into());
        self
    }

    pub fn with_pids_limit(mut self, limit: u32) -> Self {
        self.pids_limit = Some(limit);
        self
    }

    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    pub fn with_docker_binary(mut self, bin: impl Into<String>) -> Self {
        self.docker_binary = bin.into();
        self
    }

    /// Build the `docker run` argument list (server-side, no shell).
    ///
    /// Kept as a separate function so tests can assert the security
    /// defaults and injection safety.
    fn docker_run_args(&self, config: &SpawnConfig) -> Vec<String> {
        let mut args = vec![
            "run".to_string(),
            "--detach".to_string(),
            "--rm".to_string(),
            format!("--name={}", config.name),
            // Spec §3 security defaults (same as ContainerTarget).
            format!("--network={}", self.network.as_deref().unwrap_or("none")),
            format!("--memory={}", self.memory_limit.as_deref().unwrap_or("2g")),
            format!("--pids-limit={}", self.pids_limit.unwrap_or(512)),
            format!("--user={}", self.user.as_deref().unwrap_or("65534:65534")),
        ];
        for (k, v) in &config.env {
            args.push(format!("--env={}={}", k, v));
        }
        args.push(format!("--workdir={}", config.work_dir));
        args.push(self.image.clone());
        args.push(config.command.clone());
        args.extend(config.args.iter().cloned());
        args
    }

    /// Run the remote docker binary with the given subcommand args over SSH.
    /// Each argument is shell-quoted individually — no remote shell
    /// interpolation of user-controlled strings.
    async fn remote_docker(&self, docker_args: &[String]) -> Result<String> {
        let quoted: Vec<String> = docker_args
            .iter()
            .map(|a| shell_quote(a))
            .collect();
        let remote_cmd = format!("{} {}", self.docker_binary, quoted.join(" "));
        let mut ssh_args = self.ssh.base_ssh_args();
        ssh_args.push(self.ssh.destination());
        ssh_args.push(remote_cmd);
        let output = Command::new("ssh")
            .args(&ssh_args)
            .output()
            .await
            .with_context(|| {
                format!("ssh to {} failed — is ssh installed?", self.ssh.destination())
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "remote docker on {} failed: {}",
                self.ssh.destination(),
                stderr
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Resolve the image digest on the remote host
    /// (`docker image inspect --format {{.Id}}`).
    ///
    /// Best-effort: returns `Err` when the remote docker cannot resolve the
    /// image; callers use it for the `wl_image_hash` claim and must tolerate
    /// absence.
    pub async fn remote_image_digest(&self) -> Result<String> {
        self.remote_docker(&[
            "image".to_string(),
            "inspect".to_string(),
            "--format={{.Id}}".to_string(),
            self.image.clone(),
        ])
        .await
        .and_then(|out| {
            let digest = out.trim().to_string();
            if digest.is_empty() {
                Err(anyhow::anyhow!(
                    "remote docker returned empty digest for {}",
                    self.image
                ))
            } else {
                Ok(digest)
            }
        })
    }
}

#[async_trait]
impl ComputeTarget for SshDockerTarget {
    fn name(&self) -> &str {
        "ssh"
    }

    fn target_type(&self) -> &'static str {
        "ssh"
    }

    async fn spawn_process(&self, config: &SpawnConfig) -> Result<ProcessHandle> {
        let container_id = self.remote_docker(&self.docker_run_args(config)).await?;
        if container_id.is_empty() {
            return Err(anyhow::anyhow!(
                "remote docker run on {} returned no container id",
                self.ssh.destination()
            ));
        }
        Ok(ProcessHandle {
            id: container_id,
            target_type: "ssh".to_string(),
            pid: None,
        })
    }

    async fn kill_process(&self, handle: &ProcessHandle) -> Result<()> {
        let _ = self
            .remote_docker(&[
                "rm".to_string(),
                "--force".to_string(),
                handle.id.clone(),
            ])
            .await;
        // Already-gone containers are a successful kill.
        Ok(())
    }

    async fn is_alive(&self, handle: &ProcessHandle) -> Result<bool> {
        let out = self
            .remote_docker(&[
                "inspect".to_string(),
                "--format={{.State.Running}}".to_string(),
                handle.id.clone(),
            ])
            .await
            .unwrap_or_default();
        Ok(out.trim() == "true")
    }
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

    fn ssh_docker_config(name: &str) -> SpawnConfig {
        let mut env = std::collections::HashMap::new();
        env.insert("GYRE_AGENT_ID".to_string(), "a-1".to_string());
        SpawnConfig {
            name: name.to_string(),
            command: "/gyre/entrypoint.sh".to_string(),
            args: vec![],
            env,
            work_dir: "/workspace".to_string(),
        }
    }

    #[test]
    fn ssh_docker_run_args_enforce_security_defaults() {
        let t = SshDockerTarget::new(SshTarget::new("user", "host"), "gyre-agent:latest");
        let args = t.docker_run_args(&ssh_docker_config("agent-1"));
        // Spec §3 container security defaults, applied on the remote host.
        assert!(args.contains(&"--network=none".to_string()));
        assert!(args.contains(&"--memory=2g".to_string()));
        assert!(args.contains(&"--pids-limit=512".to_string()));
        assert!(args.contains(&"--user=65534:65534".to_string()));
        assert!(args.contains(&"--detach".to_string()));
        assert!(args.contains(&"--name=agent-1".to_string()));
        // Env vars forwarded so the remote container can authenticate.
        assert!(args.contains(&"--env=GYRE_AGENT_ID=a-1".to_string()));
        assert!(args.contains(&"gyre-agent:latest".to_string()));
    }

    #[test]
    fn ssh_docker_run_args_apply_overrides() {
        let t = SshDockerTarget::new(SshTarget::new("user", "host"), "gyre-agent:latest")
            .with_network("bridge")
            .with_memory_limit("4g")
            .with_pids_limit(256)
            .with_user("1000:1000");
        let args = t.docker_run_args(&ssh_docker_config("agent-2"));
        assert!(args.contains(&"--network=bridge".to_string()));
        assert!(args.contains(&"--memory=4g".to_string()));
        assert!(args.contains(&"--pids-limit=256".to_string()));
        assert!(args.contains(&"--user=1000:1000".to_string()));
        // Defaults must not linger alongside overrides.
        assert!(!args.contains(&"--network=none".to_string()));
        assert!(!args.contains(&"--memory=2g".to_string()));
    }

    #[test]
    fn ssh_docker_remote_command_shell_quotes_every_arg() {
        // An image tag (or env value) carrying shell metacharacters must be
        // quoted as a single argv element, never interpolated by the remote
        // shell.
        let t = SshDockerTarget::new(SshTarget::new("user", "host"), "img; rm -rf /");
        let args = t.docker_run_args(&ssh_docker_config("agent-3"));
        let quoted: Vec<String> = args.iter().map(|a| shell_quote(a)).collect();
        let remote = format!("docker {}", quoted.join(" "));
        // The hostile image tag appears exactly once, quoted.
        assert_eq!(remote.matches("rm -rf /").count(), 1);
        assert!(remote.contains("'img; rm -rf /'"));
    }

    /// SshDockerTarget must fail fast when ssh is absent — proves the
    /// backend shells out to a real binary and propagates failure.
    #[tokio::test]
    async fn ssh_docker_spawn_without_ssh_errors() {
        if which_exists("ssh") {
            return; // environment has ssh; failure path untestable here
        }
        let t = SshDockerTarget::new(SshTarget::new("user", "host"), "gyre-agent:latest");
        let res = t.spawn_process(&ssh_docker_config("agent-x")).await;
        assert!(res.is_err(), "spawn must fail when ssh is missing");
    }

    fn which_exists(bin: &str) -> bool {
        std::env::var_os("PATH")
            .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).exists()))
            .unwrap_or(false)
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
