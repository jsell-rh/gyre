use anyhow::{Context, Result};
use async_trait::async_trait;
use gyre_ports::{ComputeTarget, ProcessHandle, SpawnConfig};
use std::collections::HashMap;
use std::time::Duration;
use tokio::process::Command;

/// Total wall-clock bound on a single ssh-invoked kill round trip.
/// `ConnectTimeout` bounds only connection *setup*; a packet-dropped
/// established session or a stalled remote would otherwise stall the
/// caller indefinitely — and the sequential remote_agent_cleanup /
/// admin-kill paths run these kills one after another, so one hung
/// target freezes the whole sweep (review task-273: only LocalTarget
/// bounded its kill wait). Shortened under test so timeout paths are
/// assertable without sleeping for the production bound.
#[cfg(not(test))]
const SSH_KILL_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(test)]
const SSH_KILL_TIMEOUT: Duration = Duration::from_millis(750);

/// How long `kill_process` waits for the remote process to exit after a
/// signal before escalating / failing — mirrors LocalTarget's 10 s.
#[cfg(not(test))]
const REMOTE_EXIT_WAIT: Duration = Duration::from_secs(10);
#[cfg(test)]
const REMOTE_EXIT_WAIT: Duration = Duration::from_millis(400);

/// Spawns processes on remote hosts via SSH.
pub struct SshTarget {
    pub user: String,
    pub host: String,
    /// Optional SSH identity file path.
    pub identity_file: Option<String>,
    /// Optional SSH port (default: 22).
    pub port: Option<u16>,
    /// Optional local ssh client binary (default: `ssh` from PATH).
    /// Deployment knob for pinned or non-standard client locations —
    /// mirrors `kubectl_path` on the Kubernetes target. Applies to every
    /// client invocation this target makes (tunnels, bare-process
    /// spawn/kill/liveness, and the docker-over-SSH commands of
    /// [`SshDockerTarget`]).
    pub ssh_path: Option<String>,
}

impl SshTarget {
    pub fn new(user: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            user: user.into(),
            host: host.into(),
            identity_file: None,
            port: None,
            ssh_path: None,
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

    /// Pin the local ssh client binary (see [`SshTarget::ssh_path`]).
    pub fn with_ssh_path(mut self, path: impl Into<String>) -> Self {
        self.ssh_path = Some(path.into());
        self
    }

    /// The ssh client binary to invoke: the pinned `ssh_path` when set,
    /// else `ssh` resolved from PATH.
    fn ssh_bin(&self) -> &str {
        self.ssh_path.as_deref().unwrap_or("ssh")
    }

    /// Reconstruct a target from a recorded handle so out-of-band
    /// operations (kill, liveness) reach the RIGHT host. `handle.id` is
    /// `user@host:pid` (see `spawn_process`); the identity file and port
    /// recorded in `handle.options` at spawn time are restored so a
    /// non-default SSH config still authenticates.
    pub fn from_handle(handle: &ProcessHandle) -> Option<Self> {
        // `user@host:pid` — split off the trailing pid, then user from host.
        // Never slice at fixed byte indexes: user/host are arbitrary UTF-8.
        let (destination, _pid) = handle.id.rsplit_once(':')?;
        let (user, host) = destination.split_once('@')?;
        if user.is_empty() || host.is_empty() {
            return None;
        }
        let mut target = Self::new(user, host);
        if let Some(identity_file) = handle.options.get("identity_file") {
            target = target.with_identity(identity_file.clone());
        }
        if let Some(port) = handle
            .options
            .get("port")
            .and_then(|p| p.parse::<u16>().ok())
        {
            target = target.with_port(port);
        }
        Some(target)
    }

    fn destination(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }

    /// Run `kill <args>` on the remote host and classify the outcome.
    ///
    /// SSH transport failures and unexpected remote failures are ERRORS:
    /// callers (`kill_and_mark`, shared by remote_agent_cleanup and the
    /// admin kill path) mark an instance terminated only after a verified
    /// kill, so a swallowed failure here would permanently hide a live
    /// orphan behind a "reaped" record (review task-273). Only the
    /// remote's "No such process" is an already-gone success. Bounded by
    /// [`SSH_KILL_TIMEOUT`].
    async fn remote_kill(&self, args: &str) -> Result<RemoteSignal> {
        let remote_cmd = format!("kill {args}");
        let mut ssh_args = self.base_ssh_args();
        ssh_args.push(self.destination());
        ssh_args.push(remote_cmd);

        let output = tokio::time::timeout(
            SSH_KILL_TIMEOUT,
            Command::new(self.ssh_bin())
                .kill_on_drop(true)
                .args(&ssh_args)
                .output(),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "ssh kill to {} timed out after {:?} — remote unreachable or stalled",
                self.destination(),
                SSH_KILL_TIMEOUT
            )
        })?
        .context("ssh kill failed")?;

        if output.status.success() {
            return Ok(RemoteSignal::Signalled);
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if ssh_transport_failed(&stderr) {
            return Err(anyhow::anyhow!(
                "ssh kill to {} failed: {}",
                self.destination(),
                stderr.trim()
            ));
        }
        // Every remote `kill` implementation (bash/dash builtins,
        // util-linux, busybox) words a missing target this way — a
        // confirmed exit, not a failure.
        if stderr.contains("No such process") {
            return Ok(RemoteSignal::Gone);
        }
        Err(anyhow::anyhow!(
            "remote kill on {} failed: {}",
            self.destination(),
            stderr.trim()
        ))
    }

    /// Poll `kill -0 <pid>` on the remote host until the process is gone
    /// or `timeout` elapses (the ssh twin of LocalTarget's
    /// `wait_for_exit`). Returns `true` when the process exited.
    async fn wait_remote_exit(&self, pid: u32, timeout: Duration) -> Result<bool> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if matches!(
                self.remote_kill(&format!("-0 {pid}")).await?,
                RemoteSignal::Gone
            ) {
                return Ok(true);
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(false);
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    fn base_ssh_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            "StrictHostKeyChecking=accept-new".to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            // Bound the connection setup: without this a blackholed host
            // stalls the caller for the OS TCP timeout (~2min), which
            // blocks kill/is_alive probes far beyond the exit monitor's
            // poll interval.
            "-o".to_string(),
            "ConnectTimeout=10".to_string(),
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

        let child = Command::new(self.ssh_bin())
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

        let output = Command::new(self.ssh_bin())
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
            // Connection details for later out-of-band kills: the registry
            // stores only the handle, and reconstructing the target from
            // `user@host:pid` alone would drop a configured identity file
            // or non-default port.
            options: {
                let mut options = HashMap::new();
                if let Some(ref id) = self.identity_file {
                    options.insert("identity_file".to_string(), id.clone());
                }
                if let Some(port) = self.port {
                    options.insert("port".to_string(), port.to_string());
                }
                options
            },
        })
    }

    async fn kill_process(&self, handle: &ProcessHandle) -> Result<()> {
        let Some(pid) = handle.pid else {
            return Ok(());
        };
        // SIGTERM first; a racing exit shows up in the poll below. "No
        // such process" on the FIRST signal means the process was
        // already gone — an idempotent success. Every other failure
        // (ssh transport error, permission denied) PROPAGATES: callers
        // mark the instance terminated only after a verified kill, and
        // returning Ok here would permanently hide a still-running
        // orphan behind a "reaped" record (review task-273 — this path
        // previously debug-logged non-zero exits and returned Ok).
        if matches!(
            self.remote_kill(&format!("-TERM {pid}")).await?,
            RemoteSignal::Gone
        ) {
            return Ok(());
        }
        if self.wait_remote_exit(pid, REMOTE_EXIT_WAIT).await? {
            return Ok(());
        }
        // Escalate to SIGKILL and VERIFY — the kill's success is the
        // process actually being gone, mirroring LocalTarget::kill_process
        // (a remote process that traps SIGTERM must not be recorded as
        // successfully killed while it keeps running).
        if matches!(
            self.remote_kill(&format!("-KILL {pid}")).await?,
            RemoteSignal::Gone
        ) {
            return Ok(());
        }
        if self.wait_remote_exit(pid, REMOTE_EXIT_WAIT).await? {
            return Ok(());
        }
        anyhow::bail!(
            "remote process {pid} on {} survived SIGTERM and SIGKILL — kill did not take effect",
            self.destination()
        );
    }

    async fn is_alive(&self, handle: &ProcessHandle) -> Result<bool> {
        if let Some(pid) = handle.pid {
            let remote_cmd = format!("kill -0 {}", pid);
            let mut ssh_args = self.base_ssh_args();
            ssh_args.push(self.destination());
            ssh_args.push(remote_cmd);

            let output = Command::new(self.ssh_bin())
                .args(&ssh_args)
                .output()
                .await
                .context("ssh is_alive check failed")?;

            if output.status.success() {
                return Ok(true);
            }
            // A failed remote `kill -0` (process gone, or not ours) exits
            // non-zero with EMPTY stderr — that is a confirmed exit. SSH
            // transport failures (connection refused, timeout, auth) also
            // exit non-zero but print an "ssh: ..." diagnostic to stderr;
            // those are probe failures, not evidence of death, and must not
            // trigger the exit monitor's destructive teardown.
            let stderr = String::from_utf8_lossy(&output.stderr);
            if ssh_transport_failed(&stderr) {
                return Err(anyhow::anyhow!(
                    "ssh is_alive probe to {} failed: {}",
                    self.destination(),
                    stderr.trim()
                ));
            }
            Ok(false)
        } else {
            Ok(false)
        }
    }
}

/// Minimal shell quoting: wrap in single quotes, escape embedded single quotes.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Classified outcome of a remote `kill` invocation (see
/// [`SshTarget::remote_kill`]).
enum RemoteSignal {
    /// The remote kill command ran and signalled (or probed) the process.
    Signalled,
    /// The remote reported the process does not exist — already gone.
    Gone,
}

/// True when non-zero-exit ssh stderr shows an ssh CLIENT failure ("ssh: ..."
/// diagnostics: connection refused, timeout, auth). A remote command that
/// merely exits non-zero prints nothing to stderr — that is a confirmed
/// remote result, not a transport failure.
fn ssh_transport_failed(stderr: &str) -> bool {
    stderr.lines().any(|l| l.starts_with("ssh:"))
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
        let quoted: Vec<String> = docker_args.iter().map(|a| shell_quote(a)).collect();
        let remote_cmd = format!("{} {}", self.docker_binary, quoted.join(" "));
        let mut ssh_args = self.ssh.base_ssh_args();
        ssh_args.push(self.ssh.destination());
        ssh_args.push(remote_cmd);
        let output = Command::new(self.ssh.ssh_bin())
            .kill_on_drop(true)
            .args(&ssh_args)
            .output()
            .await
            .with_context(|| {
                format!(
                    "ssh to {} failed — is ssh installed?",
                    self.ssh.destination()
                )
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
            // Connection details for out-of-band operations. The handle id
            // is the remote container id (not `user@host:pid`), so
            // `SshTarget::from_handle` refuses to reconstruct a bare-ssh
            // target from it — kills must go through the recorded backend
            // (`spawned_backends`), which knows this is a remote container.
            options: {
                let mut options = HashMap::new();
                if let Some(ref id) = self.ssh.identity_file {
                    options.insert("identity_file".to_string(), id.clone());
                }
                if let Some(port) = self.ssh.port {
                    options.insert("port".to_string(), port.to_string());
                }
                options
            },
        })
    }

    async fn kill_process(&self, handle: &ProcessHandle) -> Result<()> {
        // A failed remote `docker rm --force` (ssh transport failure,
        // daemon down, permission denial) is a REAL failure that must
        // surface: callers mark the instance terminated only after a
        // verified kill, and swallowing the error here (as this path
        // previously did with `let _ =`) would permanently hide a live
        // remote container behind a "reaped" record (review task-273).
        // Only "No such object" is a success — the container is already
        // gone, mirroring `container_already_gone` on the local
        // container target. Bounded: one hung remote must not stall the
        // sequential cleanup sweep.
        let killed = tokio::time::timeout(
            SSH_KILL_TIMEOUT,
            self.remote_docker(&["rm".to_string(), "--force".to_string(), handle.id.clone()]),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "docker rm --force on {} timed out after {:?} — remote unreachable or stalled",
                self.ssh.destination(),
                SSH_KILL_TIMEOUT
            )
        })?;
        match killed {
            Ok(_) => Ok(()),
            Err(e) if remote_container_is_missing(&e) => {
                tracing::info!(container_id = %handle.id, "remote container already gone");
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    async fn is_alive(&self, handle: &ProcessHandle) -> Result<bool> {
        match self
            .remote_docker(&[
                "inspect".to_string(),
                "--format={{.State.Running}}".to_string(),
                handle.id.clone(),
            ])
            .await
        {
            Ok(out) => Ok(out.trim() == "true"),
            // `docker run --rm` auto-removes an exited container, so "No
            // such object" is the CONFIRMED-exit signal on this backend —
            // not a probe failure.
            Err(e) if remote_container_is_missing(&e) => Ok(false),
            // SSH/daemon/transport failure: the container's state is
            // UNKNOWN. Reporting `Ok(false)` here would make one transient
            // probe failure look like a confirmed exit, and the exit
            // monitor destroys preview work (token, worktree, budget slot)
            // unrecoverably on that signal. Propagate the error so the
            // monitor keeps polling instead of tearing down.
            Err(e) => Err(e),
        }
    }
}

/// True when a failed remote-docker command failed because the container
/// does not exist (docker's "No such object" stderr) — a confirmed exit on
/// this backend, since containers run with `--rm` are auto-removed when
/// they stop. Anything else (SSH connection refused, daemon down) is a
/// probe failure, not evidence of death.
fn remote_container_is_missing(err: &anyhow::Error) -> bool {
    format!("{err:#}").contains("No such object")
}

/// Build an [`SshDockerTarget`] from a compute-target config blob
/// (agent-runtime.md §3 Supported Backends — SSH).
///
/// Recognized keys: `user` (required in practice; defaults `root`),
/// `host`, `port`, `identity_file`, `ssh_path` (local ssh client binary;
/// defaults to `ssh` from PATH), `image`, `network`, `memory_limit`,
/// `pids_limit`, `user_as` (container `--user`; named `user_as` to avoid
/// colliding with the SSH login user), `docker_binary`.
/// Absent security keys fall back to the spec defaults
/// (`--network=none`, `--memory=2g`, `--pids-limit=512`,
/// `--user=65534:65534`).
pub fn ssh_docker_target_from_config(config: &serde_json::Value) -> SshDockerTarget {
    let mut ssh = SshTarget::new(
        config
            .get("user")
            .and_then(|v| v.as_str())
            .unwrap_or("root"),
        config
            .get("host")
            .and_then(|v| v.as_str())
            .unwrap_or("localhost"),
    );
    if let Some(idf) = config.get("identity_file").and_then(|v| v.as_str()) {
        ssh = ssh.with_identity(idf);
    }
    if let Some(port) = config.get("port").and_then(|v| v.as_u64()) {
        ssh = ssh.with_port(port as u16);
    }
    if let Some(bin) = config.get("ssh_path").and_then(|v| v.as_str()) {
        ssh = ssh.with_ssh_path(bin);
    }
    let mut t = SshDockerTarget::new(
        ssh,
        config
            .get("image")
            .and_then(|v| v.as_str())
            .unwrap_or("gyre-agent:latest"),
    );
    if let Some(n) = config.get("network").and_then(|v| v.as_str()) {
        t = t.with_network(n);
    }
    if let Some(m) = config.get("memory_limit").and_then(|v| v.as_str()) {
        t = t.with_memory_limit(m);
    }
    if let Some(p) = config.get("pids_limit").and_then(|v| v.as_u64()) {
        t = t.with_pids_limit(p as u32);
    }
    if let Some(u) = config.get("user_as").and_then(|v| v.as_str()) {
        t = t.with_user(u);
    }
    if let Some(b) = config.get("docker_binary").and_then(|v| v.as_str()) {
        t = t.with_docker_binary(b);
    }
    t
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

    /// `from_handle` must round-trip the destination recorded in
    /// `user@host:pid` — an out-of-band kill depends on reaching the RIGHT
    /// host, and a mis-parse must fail loudly (None), never guess.
    #[test]
    fn from_handle_parses_destination_and_pid() {
        let handle = ProcessHandle {
            id: "ubuntu@192.168.1.10:4242".to_string(),
            target_type: "ssh".to_string(),
            pid: Some(4242),
            options: HashMap::new(),
        };
        let target = SshTarget::from_handle(&handle).expect("valid handle must parse");
        assert_eq!(target.user, "ubuntu");
        assert_eq!(target.host, "192.168.1.10");
        assert_eq!(target.identity_file, None);
        assert_eq!(target.port, None);
    }

    /// Connection options recorded at spawn time (identity file, non-default
    /// port) must survive the round trip — dropping them would make
    /// reconstructed kills fail authentication against configured targets.
    #[test]
    fn from_handle_restores_connection_options() {
        let mut options = HashMap::new();
        options.insert("identity_file".to_string(), "/etc/gyre/ssh_key".to_string());
        options.insert("port".to_string(), "2222".to_string());
        let handle = ProcessHandle {
            id: "root@builder.internal:99".to_string(),
            target_type: "ssh".to_string(),
            pid: Some(99),
            options,
        };
        let target = SshTarget::from_handle(&handle).expect("valid handle must parse");
        assert_eq!(target.user, "root");
        assert_eq!(target.host, "builder.internal");
        assert_eq!(target.identity_file.as_deref(), Some("/etc/gyre/ssh_key"));
        assert_eq!(target.port, Some(2222));
    }

    /// Malformed handle ids must yield `None` — a reconstruction that
    /// "falls back" to some destination would kill the wrong host's process.
    #[test]
    fn from_handle_rejects_malformed_ids() {
        for bad in [
            "4242",                // bare pid: no destination
            "localhost:4242",      // no user@ separator
            "ubuntu@:4242",        // empty host
            "@localhost:4242",     // empty user
            "ubuntu@192.168.1.10", // no pid suffix
        ] {
            let handle = ProcessHandle {
                id: bad.to_string(),
                target_type: "ssh".to_string(),
                pid: None,
                options: HashMap::new(),
            };
            assert!(
                SshTarget::from_handle(&handle).is_none(),
                "malformed id {bad:?} must not reconstruct a target"
            );
        }
    }

    #[test]
    fn ssh_base_args_bound_connect_timeout() {
        // A blackholed remote must fail within ~10s, not the OS TCP
        // timeout — kill/is_alive probes hold no locks but still gate
        // the exit monitor loop.
        let target = SshTarget::new("user", "host");
        let args = target.base_ssh_args();
        let idx = args
            .iter()
            .position(|a| a.starts_with("ConnectTimeout="))
            .expect("ConnectTimeout option missing");
        let secs: u64 = args[idx]
            .split('=')
            .nth(1)
            .and_then(|v| v.parse().ok())
            .expect("ConnectTimeout value must be numeric");
        assert!(
            secs <= 15,
            "ConnectTimeout must bound probes tightly, got {secs}s"
        );
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

    /// `ssh_path` must drive the actual binary `spawn_process` invokes —
    /// a stub ssh that answers `docker run` with a container id (and
    /// records the invocation) proves the config override reaches the
    /// Command invocation. Mirrors `spawn_uses_configured_kubectl_path`.
    #[tokio::test]
    async fn ssh_docker_spawn_uses_configured_ssh_path() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("saw-docker-run");
        let stub = dir.path().join("ssh-stub");
        std::fs::write(
            &stub,
            format!(
                "#!/bin/sh\ncmd=\nfor arg in \"$@\"; do cmd=$arg; done\ncmd=$(printf '%s' \"$cmd\" | tr -d \"'\")\ncase $cmd in\n  'docker run '*) echo stubcontainer000000000; touch {marker} ;;\n  *) echo \"stub ssh: unhandled: $cmd\" >&2; exit 1 ;;\nesac\n",
                marker = marker.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let t = ssh_docker_target_from_config(&serde_json::json!({
            "user": "ci",
            "host": "fake-remote",
            "ssh_path": stub.to_str().unwrap()
        }));
        assert_eq!(
            t.ssh.ssh_path.as_deref(),
            Some(stub.to_str().unwrap()),
            "config ssh_path must be parsed onto the target"
        );
        let handle = gyre_ports::ComputeTarget::spawn_process(&t, &ssh_docker_config("path-agent"))
            .await
            .expect("spawn via stub ssh must succeed");
        assert_eq!(handle.id, "stubcontainer000000000");
        assert!(
            handle.pid.is_none(),
            "remote-container handles are pid-less"
        );
        assert!(
            marker.exists(),
            "the stub ssh must have received the remote docker run command"
        );
    }

    /// No `ssh_path` in the config must leave the binary at the PATH
    /// default (`ssh`), never a fabricated or empty client path.
    #[test]
    fn ssh_docker_config_without_ssh_path_defaults_to_path_ssh() {
        let t = ssh_docker_target_from_config(&serde_json::json!({
            "user": "ci",
            "host": "fake-remote"
        }));
        assert!(
            t.ssh.ssh_path.is_none(),
            "no config ssh_path must fall back to ssh from PATH"
        );
        assert_eq!(t.ssh.ssh_bin(), "ssh");
    }

    fn which_exists(bin: &str) -> bool {
        std::env::var_os("PATH")
            .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).exists()))
            .unwrap_or(false)
    }

    /// Write an executable stub `ssh` running `script`. The stub receives
    /// the same argv a real ssh would; the remote command is the LAST
    /// argument (mirrors the spawn stub above).
    fn write_stub_ssh(script: &str) -> String {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ssh-stub");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // Leak the tempdir: the stub must outlive this function (the
        // target invokes it asynchronously afterwards).
        std::mem::forget(dir);
        path.to_str().unwrap().to_string()
    }

    fn ssh_kill_handle(pid: u32) -> ProcessHandle {
        ProcessHandle {
            id: format!("user@host:{pid}"),
            target_type: "ssh".to_string(),
            pid: Some(pid),
            options: HashMap::new(),
        }
    }

    fn ssh_container_handle(id: &str) -> ProcessHandle {
        ProcessHandle {
            id: id.to_string(),
            target_type: "ssh".to_string(),
            pid: None,
            options: HashMap::new(),
        }
    }

    /// review task-273: a bare-ssh kill whose transport fails must Err —
    /// kill_and_mark (shared by remote_agent_cleanup and the admin kill
    /// path) marks the durable record terminated only after a verified
    /// kill, and the old implementation logged the non-zero exit and
    /// returned Ok, permanently hiding the live process behind a
    /// "reaped" record.
    #[tokio::test]
    async fn ssh_kill_transport_failure_is_err() {
        let stub = write_stub_ssh(
            "echo 'ssh: connect to host host port 22: Connection refused' >&2\nexit 255",
        );
        let target = SshTarget::new("user", "host").with_ssh_path(&stub);
        let err = target
            .kill_process(&ssh_kill_handle(4242))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("Connection refused"),
            "the transport failure must surface, got: {err:#}"
        );
    }

    /// review task-273: a non-zero remote kill that is NOT a transport
    /// failure is a REAL failure (permission denial, bad signal) and
    /// must Err too.
    #[tokio::test]
    async fn ssh_kill_remote_failure_is_err() {
        let stub = write_stub_ssh(
            "cmd=\"\"\nfor arg in \"$@\"; do cmd=$arg; done\ncase \"$cmd\" in\n  'kill -TERM 4242') echo 'kill: (4242) - Operation not permitted' >&2; exit 1 ;;\n  *) echo \"stub ssh: unhandled: $cmd\" >&2; exit 1 ;;\nesac",
        );
        let target = SshTarget::new("user", "host").with_ssh_path(&stub);
        let err = target
            .kill_process(&ssh_kill_handle(4242))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("Operation not permitted"),
            "the remote kill failure must surface, got: {err:#}"
        );
    }

    /// "No such process" on the first signal is the already-gone signal —
    /// an idempotent success, never an error.
    #[tokio::test]
    async fn ssh_kill_already_gone_is_ok() {
        let stub = write_stub_ssh(
            "cmd=\"\"\nfor arg in \"$@\"; do cmd=$arg; done\ncase \"$cmd\" in\n  'kill -TERM 4242') echo 'kill: (4242) - No such process' >&2; exit 1 ;;\n  *) echo \"stub ssh: unhandled: $cmd\" >&2; exit 1 ;;\nesac",
        );
        let target = SshTarget::new("user", "host").with_ssh_path(&stub);
        target.kill_process(&ssh_kill_handle(4242)).await.unwrap();
    }

    /// SIGTERM accepted, process exits before the escalation deadline —
    /// a normal successful kill.
    #[tokio::test]
    async fn ssh_kill_term_then_exit_is_ok() {
        let stub = write_stub_ssh(
            "cmd=\"\"\nfor arg in \"$@\"; do cmd=$arg; done\ncase \"$cmd\" in\n  'kill -TERM 4242') exit 0 ;;\n  'kill -0 4242') echo 'kill: (4242) - No such process' >&2; exit 1 ;;\n  *) echo \"stub ssh: unhandled: $cmd\" >&2; exit 1 ;;\nesac",
        );
        let target = SshTarget::new("user", "host").with_ssh_path(&stub);
        target.kill_process(&ssh_kill_handle(4242)).await.unwrap();
    }

    /// A remote process that survives SIGTERM and SIGKILL must NOT be
    /// reported as killed — mirroring LocalTarget, the kill's success is
    /// the process actually being gone.
    #[tokio::test]
    async fn ssh_kill_surviving_sigterm_and_sigkill_is_err() {
        let stub = write_stub_ssh(
            "cmd=\"\"\nfor arg in \"$@\"; do cmd=$arg; done\ncase \"$cmd\" in\n  'kill -TERM '*|'kill -KILL '*|'kill -0 '*) exit 0 ;;\n  *) echo \"stub ssh: unhandled: $cmd\" >&2; exit 1 ;;\nesac",
        );
        let target = SshTarget::new("user", "host").with_ssh_path(&stub);
        let err = target
            .kill_process(&ssh_kill_handle(4242))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("survived SIGTERM and SIGKILL"),
            "a surviving process must not be recorded as killed, got: {err:#}"
        );
    }

    /// review task-273: a hung remote (packet-dropped session past
    /// ConnectTimeout) must fail the kill BOUNDED, not stall the
    /// sequential remote_agent_cleanup / admin-kill sweep forever.
    #[tokio::test]
    async fn ssh_kill_hung_remote_times_out() {
        let stub = write_stub_ssh("sleep 30");
        let target = SshTarget::new("user", "host").with_ssh_path(&stub);
        let err = target
            .kill_process(&ssh_kill_handle(4242))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("timed out"),
            "a hung remote must fail bounded, got: {err:#}"
        );
    }

    /// review task-273: a remote-docker kill failure (daemon down, ssh
    /// transport failure) must Err — the old `let _ =` swallowed it, so
    /// kill_and_mark marked the still-running remote container
    /// terminated. "No such object" is the already-gone success.
    #[tokio::test]
    async fn ssh_docker_kill_failure_surfaces_already_gone_is_ok() {
        let stub = write_stub_ssh(
            "cmd=\"\"\nfor arg in \"$@\"; do cmd=$arg; done\ncmd=$(printf '%s' \"$cmd\" | tr -d \"'\")\ncase \"$cmd\" in\n  'docker rm --force gone123') echo 'Error: No such object: gone123' >&2; exit 1 ;;\n  'docker rm --force live123') echo 'Cannot connect to the Docker daemon at unix:///var/run/docker.sock' >&2; exit 1 ;;\n  *) echo \"stub ssh: unhandled: $cmd\" >&2; exit 1 ;;\nesac",
        );
        let t = SshDockerTarget::new(
            SshTarget::new("user", "host").with_ssh_path(&stub),
            "gyre-agent:latest",
        );

        let gone = t.kill_process(&ssh_container_handle("gone123")).await;
        gone.unwrap();

        let err = t
            .kill_process(&ssh_container_handle("live123"))
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("Cannot connect to the Docker daemon"),
            "the remote docker failure must surface, got: {err:#}"
        );
    }

    /// review task-273: a hung remote docker (or stalled ssh session)
    /// must fail the remote-container kill BOUNDED.
    #[tokio::test]
    async fn ssh_docker_kill_hung_remote_times_out() {
        let stub = write_stub_ssh("sleep 30");
        let t = SshDockerTarget::new(
            SshTarget::new("user", "host").with_ssh_path(&stub),
            "gyre-agent:latest",
        );
        let err = t
            .kill_process(&ssh_container_handle("hung123"))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("timed out"),
            "a hung remote must fail bounded, got: {err:#}"
        );
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

    /// Task-206 review F5: a docker-inspect failure must only read as a
    /// confirmed exit when docker itself says the container is gone —
    // "No such object" is the `--rm` auto-remove exit signal. SSH and
    /// daemon failures are probe failures and must stay errors.
    #[test]
    fn docker_inspect_failure_classifies_missing_container() {
        let missing =
            anyhow::anyhow!("remote docker on user@host failed: Error: No such object: abc123");
        assert!(remote_container_is_missing(&missing));

        let refused = anyhow::anyhow!(
            "remote docker on user@host failed: ssh: connect to host host port 22: Connection refused"
        );
        assert!(!remote_container_is_missing(&refused));

        let daemon_down = anyhow::anyhow!(
            "remote docker on user@host failed: Cannot connect to the Docker daemon at unix:///var/run/docker.sock"
        );
        assert!(!remote_container_is_missing(&daemon_down));
    }

    /// Task-206 review F5: bare-SSH stderr classification — an ssh client
    /// diagnostic ("ssh: ...") is a transport failure; a silent non-zero
    /// exit is the remote command's confirmed result.
    #[test]
    fn ssh_stderr_classifies_transport_failure() {
        assert!(ssh_transport_failed(
            "ssh: connect to host 10.0.0.9 port 22: Connection timed out\r\n"
        ));
        assert!(ssh_transport_failed(
            "ssh: Could not resolve hostname host: Name or service not known"
        ));
        // Remote kill -0 on a gone process: non-zero exit, empty stderr.
        assert!(!ssh_transport_failed(""));
        // Remote command noise is not an ssh client diagnostic.
        assert!(!ssh_transport_failed("kill: (4242) - No such process\n"));
    }
}
