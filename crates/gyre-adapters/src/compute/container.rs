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

    /// Build the `{runtime} run` argument vector for a spawn.
    ///
    /// Factored out of [`spawn_process`](ComputeTarget::spawn_process) so
    /// the spec §3 security defaults are pinned by tests against the real
    /// argv (same pattern as `SshDockerTarget::docker_run_args`).
    fn docker_run_args(&self, config: &SpawnConfig) -> Vec<String> {
        let mut args = vec![
            "run".to_string(),
            "--detach".to_string(),
            "--rm".to_string(),
            format!("--name={}", config.name),
            format!("--workdir={}", config.work_dir),
        ];

        // --- Security defaults (CISO G8-A/B/C) ---

        // G8-A: network isolation — deny all outbound by default.
        let network = self.network.as_deref().unwrap_or("none");
        args.push(format!("--network={}", network));

        // G8-B: resource limits — prevent runaway memory and fork bombs.
        let memory = self.memory_limit.as_deref().unwrap_or("2g");
        args.push(format!("--memory={}", memory));
        let pids = self.pids_limit.unwrap_or(512);
        args.push(format!("--pids-limit={}", pids));

        // G8-C: non-root user — run as nobody:nogroup (uid/gid 65534).
        let user = self.user.as_deref().unwrap_or("65534:65534");
        args.push(format!("--user={}", user));

        // --- End security defaults ---

        // Mount worktree path so the agent can access its checkout.
        args.push(format!("--volume={}:{}", config.work_dir, config.work_dir));

        // Additional user-configured volume mounts.
        for v in &self.volumes {
            args.push(format!("--volume={}", v));
        }

        // Pre-configured env vars (e.g. GYRE_SERVER_URL, GYRE_AGENT_TOKEN).
        for (k, v) in &self.env_vars {
            args.push(format!("--env={}={}", k, v));
        }

        // SpawnConfig env vars (caller-supplied, may override pre-configured).
        for (k, v) in &config.env {
            args.push(format!("--env={}={}", k, v));
        }

        args.push(self.image.clone());
        args.push(config.command.clone());
        args.extend(config.args.iter().cloned());
        args
    }

    async fn resolve_runtime(&self) -> Result<Runtime> {
        match &self.runtime {
            Some(r) => Ok(r.clone()),
            None => Runtime::detect().await,
        }
    }

    /// Resolve the local image digest via `docker image inspect
    /// --format {{.Id}}` (best-effort; `Err` when the runtime cannot
    /// resolve the image). Callers use it for the `wl_image_hash`
    /// attestation claim and must tolerate absence.
    pub async fn image_digest(&self) -> Result<String> {
        let runtime = self.resolve_runtime().await?;
        let bin = runtime.binary();
        let output = Command::new(bin)
            .args(["image", "inspect", "--format={{.Id}}", &self.image])
            .output()
            .await
            .with_context(|| format!("{} image inspect failed", bin))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "{} image inspect for {} failed: {}",
                bin,
                self.image,
                stderr
            ));
        }
        let digest = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if digest.is_empty() {
            return Err(anyhow::anyhow!(
                "{} returned empty digest for {}",
                bin,
                self.image
            ));
        }
        Ok(digest)
    }
}

#[async_trait]
impl ComputeTarget for ContainerTarget {
    fn name(&self) -> &str {
        "container"
    }

    fn target_type(&self) -> &'static str {
        "container"
    }

    async fn spawn_process(&self, config: &SpawnConfig) -> Result<ProcessHandle> {
        let runtime = self.resolve_runtime().await?;
        let bin = runtime.binary();

        let mut cmd = Command::new(bin);
        cmd.args(self.docker_run_args(config));

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

    fn container_config(name: &str) -> SpawnConfig {
        let mut env = HashMap::new();
        env.insert("GYRE_AGENT_ID".to_string(), "a-1".to_string());
        SpawnConfig {
            name: name.to_string(),
            command: "/gyre/entrypoint.sh".to_string(),
            args: vec![],
            env,
            work_dir: "/workspace".to_string(),
        }
    }

    /// G8-A/B/C + spec §3: a default-config target builds a `docker run`
    /// argv that carries every security default. Asserted against the real
    /// argv vector — not a restatement of the `unwrap_or` fallbacks — so
    /// dropping any flag from `docker_run_args` fails here.
    #[test]
    fn docker_run_args_enforce_security_defaults() {
        let t = ContainerTarget::new("gyre-agent:latest");
        let args = t.docker_run_args(&container_config("agent-1"));
        // The four spec §3 security defaults.
        assert!(
            args.contains(&"--network=none".to_string()),
            "default argv must pin --network=none: {args:?}"
        );
        assert!(
            args.contains(&"--memory=2g".to_string()),
            "default argv must pin --memory=2g: {args:?}"
        );
        assert!(
            args.contains(&"--pids-limit=512".to_string()),
            "default argv must pin --pids-limit=512: {args:?}"
        );
        assert!(
            args.contains(&"--user=65534:65534".to_string()),
            "default argv must pin --user=65534:65534: {args:?}"
        );
        // Structural flags and worktree volume.
        assert!(args.contains(&"--detach".to_string()));
        assert!(args.contains(&"--rm".to_string()));
        assert!(args.contains(&"--name=agent-1".to_string()));
        assert!(args.contains(&"--workdir=/workspace".to_string()));
        assert!(args.contains(&"--volume=/workspace:/workspace".to_string()));
        // Env vars forwarded so the container can authenticate.
        assert!(args.contains(&"--env=GYRE_AGENT_ID=a-1".to_string()));
        assert!(args.contains(&"gyre-agent:latest".to_string()));
        assert!(args.contains(&"/gyre/entrypoint.sh".to_string()));
    }

    /// G8-A/B/C: overrides replace the defaults on the real argv — the
    /// default flags must not linger alongside the configured ones.
    #[test]
    fn docker_run_args_apply_overrides() {
        let t = ContainerTarget::new("gyre-agent:latest")
            .with_network("bridge")
            .with_memory_limit("4g")
            .with_pids_limit(256)
            .with_user("1000:1000")
            .with_volume("/host/data:/data")
            .with_env("GYRE_SERVER_URL", "http://localhost:3000");
        let args = t.docker_run_args(&container_config("agent-2"));
        assert!(args.contains(&"--network=bridge".to_string()));
        assert!(args.contains(&"--memory=4g".to_string()));
        assert!(args.contains(&"--pids-limit=256".to_string()));
        assert!(args.contains(&"--user=1000:1000".to_string()));
        assert!(args.contains(&"--volume=/host/data:/data".to_string()));
        assert!(args.contains(&"--env=GYRE_SERVER_URL=http://localhost:3000".to_string()));
        // Defaults must not linger alongside overrides.
        assert!(!args.contains(&"--network=none".to_string()));
        assert!(!args.contains(&"--memory=2g".to_string()));
        assert!(!args.contains(&"--pids-limit=512".to_string()));
        assert!(!args.contains(&"--user=65534:65534".to_string()));
    }

    /// Serializes tests that mutate `PATH`: `set_var` changes
    /// process-global state, so PATH lookup must not race across tests
    /// that spawn through it.
    static PATH_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
        std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

    /// Drive the REAL `spawn_process` through a stub runtime binary and
    /// return the argv the stub actually executed plus the resulting
    /// handle. Pins the wiring from `docker_run_args` to the spawned
    /// process: dropping a security flag from the executed command line
    /// fails the caller's assertions.
    #[cfg(unix)]
    async fn spawn_via_stub(runtime: Runtime, stub_name: &str) -> (ProcessHandle, Vec<String>) {
        let _guard = PATH_LOCK.lock().await;
        let dir = tempfile::TempDir::new().unwrap();
        let argv_dump = dir.path().join("argv");
        let stub = dir.path().join(stub_name);
        std::fs::write(
            &stub,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\necho stub-container-id\n",
                argv_dump.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();

        let old_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", dir.path().display(), old_path));
        let target = ContainerTarget::new("gyre-agent:latest").with_runtime(runtime);
        let spawned = target.spawn_process(&container_config("agent-stub")).await;
        std::env::set_var("PATH", &old_path);
        let handle = spawned.expect("stub runtime run must succeed");

        let argv: Vec<String> = std::fs::read_to_string(&argv_dump)
            .expect("stub must have dumped its argv")
            .lines()
            .map(str::to_string)
            .collect();
        (handle, argv)
    }

    /// The docker spawn path executes the spec §3 security defaults on the
    /// argv the runtime binary actually receives (mutation check: turning
    /// any of the four flags conditional fails here).
    #[cfg(unix)]
    #[tokio::test]
    async fn spawn_executes_security_defaults_via_docker_stub() {
        let (handle, argv) = spawn_via_stub(Runtime::Docker, "docker").await;
        assert_eq!(handle.id, "stub-container-id");
        assert_eq!(handle.target_type, "container");
        assert_eq!(argv.first().map(String::as_str), Some("run"));
        assert!(
            argv.contains(&"--network=none".to_string()),
            "executed argv: {argv:?}"
        );
        assert!(
            argv.contains(&"--memory=2g".to_string()),
            "executed argv: {argv:?}"
        );
        assert!(
            argv.contains(&"--pids-limit=512".to_string()),
            "executed argv: {argv:?}"
        );
        assert!(
            argv.contains(&"--user=65534:65534".to_string()),
            "executed argv: {argv:?}"
        );
    }

    /// Same pinning for the podman runtime: the explicitly-configured
    /// runtime selects the podman binary and the security defaults ride
    /// the executed argv unchanged.
    #[cfg(unix)]
    #[tokio::test]
    async fn spawn_executes_security_defaults_via_podman_stub() {
        let (handle, argv) = spawn_via_stub(Runtime::Podman, "podman").await;
        assert_eq!(handle.id, "stub-container-id");
        assert_eq!(handle.target_type, "container");
        assert!(
            argv.contains(&"--network=none".to_string()),
            "executed argv: {argv:?}"
        );
        assert!(
            argv.contains(&"--memory=2g".to_string()),
            "executed argv: {argv:?}"
        );
        assert!(
            argv.contains(&"--pids-limit=512".to_string()),
            "executed argv: {argv:?}"
        );
        assert!(
            argv.contains(&"--user=65534:65534".to_string()),
            "executed argv: {argv:?}"
        );
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
