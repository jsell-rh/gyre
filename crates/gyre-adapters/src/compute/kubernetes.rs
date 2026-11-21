use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use gyre_ports::{ComputeTarget, ProcessHandle, SpawnConfig};
use tokio::process::Command;

/// Total wall-clock bound on one `kubectl delete pod` invocation. Must
/// exceed `--grace-period=30` so a slow-but-successful graceful shutdown
/// still completes; a hung kubectl / API server must not stall the
/// sequential remote_agent_cleanup / admin-kill paths that run these
/// kills one after another (review task-273: only LocalTarget bounded
/// its kill wait). Shortened under test so the timeout path is
/// assertable without sleeping.
#[cfg(not(test))]
const KUBECTL_DELETE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
#[cfg(test)]
const KUBECTL_DELETE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(750);

/// Spawns agents as Pods on a Kubernetes cluster via the `kubectl` CLI
/// (agent-runtime.md §3 Supported Backends — Kubernetes).
///
/// The Pod runs the same agent image as the container backend; only the
/// orchestration layer differs. Pod name is derived from the spawn name
/// (which the server validates to `[a-zA-Z0-9._-]`), namespace / service
/// account / resource limits come from the compute target config.
///
/// `kill_process` deletes the Pod; `is_alive` checks the Pod phase.
pub struct KubernetesTarget {
    /// Agent container image (e.g. `ghcr.io/my-org/gyre-agent:latest`).
    pub image: String,
    /// Namespace to create Pods in. `None` = cluster default ("default").
    pub namespace: Option<String>,
    /// Service account the Pod runs as. `None` = namespace default.
    pub service_account: Option<String>,
    /// Memory limit, e.g. "2Gi" (spec default when `None`).
    pub memory_limit: Option<String>,
    /// CPU limit, e.g. "2".
    pub cpu_limit: Option<String>,
    /// Container image pull policy. `None` = cluster default.
    pub image_pull_policy: Option<String>,
    /// Kubernetes context override (kubectl --context). `None` = current.
    pub context: Option<String>,
    /// Explicit kubectl binary path. `None` = `kubectl` from PATH.
    /// Lets a deployment pin a cluster-specific CLI (e.g. OpenShift `oc`,
    /// a version-pinned kubectl) instead of relying on PATH resolution.
    pub kubectl_path: Option<String>,
}

impl KubernetesTarget {
    pub fn new(image: impl Into<String>) -> Self {
        Self {
            image: image.into(),
            namespace: None,
            service_account: None,
            memory_limit: None,
            cpu_limit: None,
            image_pull_policy: None,
            context: None,
            kubectl_path: None,
        }
    }

    pub fn with_namespace(mut self, ns: impl Into<String>) -> Self {
        self.namespace = Some(ns.into());
        self
    }

    pub fn with_service_account(mut self, sa: impl Into<String>) -> Self {
        self.service_account = Some(sa.into());
        self
    }

    pub fn with_memory_limit(mut self, limit: impl Into<String>) -> Self {
        self.memory_limit = Some(limit.into());
        self
    }

    pub fn with_cpu_limit(mut self, limit: impl Into<String>) -> Self {
        self.cpu_limit = Some(limit.into());
        self
    }

    pub fn with_image_pull_policy(mut self, policy: impl Into<String>) -> Self {
        self.image_pull_policy = Some(policy.into());
        self
    }

    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        self.context = Some(ctx.into());
        self
    }

    /// Use an explicit kubectl binary instead of `kubectl` from PATH.
    pub fn with_kubectl_path(mut self, path: impl Into<String>) -> Self {
        self.kubectl_path = Some(path.into());
        self
    }

    /// Prefix every kubectl invocation with context/namespace flags.
    fn kubectl(&self) -> Command {
        let mut cmd = Command::new(self.kubectl_path.as_deref().unwrap_or("kubectl"));
        if let Some(ctx) = &self.context {
            cmd.arg("--context").arg(ctx);
        }
        if let Some(ns) = &self.namespace {
            cmd.arg("--namespace").arg(ns);
        }
        cmd
    }

    /// Resolve the digest the cluster actually pulled for the agent image
    /// (`status.containerStatuses[0].imageID`), e.g.
    /// `docker-pullable://registry/gyre-agent@sha256:...`.
    ///
    /// Best-effort: `Err` when the Pod is not yet scheduled or the cluster
    /// is unreachable. Callers use it for the `wl_image_hash` attestation
    /// claim and must tolerate absence.
    pub async fn pod_image_digest(&self, pod: &str) -> Result<String> {
        let output = self
            .kubectl()
            .arg("get")
            .arg("pod")
            .arg(pod)
            .arg("-o")
            .arg("jsonpath={.status.containerStatuses[0].imageID}")
            .output()
            .await
            .with_context(|| format!("kubectl get pod {pod} (imageID) failed"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("kubectl get pod {pod} imageID failed: {}", stderr));
        }
        let digest = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if digest.is_empty() {
            return Err(anyhow!("pod {pod} has no imageID yet (not scheduled)"));
        }
        Ok(digest)
    }
}

/// Build the Pod manifest passed to `kubectl apply`. Kept as a separate
/// function so tests can assert the generated security defaults.
///
/// The Pod name is the RFC 1123-sanitized form of the agent name — the same
/// `pod_name()` that `spawn_process` returns in the `ProcessHandle`, so the
/// manifest, wait, kill, and is_alive paths all address the same Pod.
fn build_pod_manifest(target: &KubernetesTarget, config: &SpawnConfig) -> serde_json::Value {
    let name = pod_name(&config.name);
    // Spec security defaults (agent-runtime.md §3): --memory=2g,
    // --pids-limit=512 equivalents. Kubernetes has no direct pids-limit
    // field; it is enforced via the pod-level `pids` cgroup limit which
    // kubelet derives from --pod-max-pids / container runtime defaults —
    // the memory limit is what we can set per-Pod here.
    let memory = target.memory_limit.as_deref().unwrap_or("2Gi");
    let cpu = target.cpu_limit.as_deref().unwrap_or("2");

    let mut container = serde_json::json!({
        "name": "agent",
        "image": target.image,
        "command": [config.command],
        "args": config.args,
        "workingDir": config.work_dir,
        "env": config
            .env
            .iter()
            .map(|(k, v)| serde_json::json!({"name": k, "value": v}))
            .collect::<Vec<_>>(),
        "resources": {
            "limits": {
                "memory": memory,
                "cpu": cpu,
            },
        },
        "securityContext": {
            // Spec: --user=65534:65534 (nobody:nogroup, non-root).
            "runAsUser": 65534,
            "runAsGroup": 65534,
            "runAsNonRoot": true,
            "allowPrivilegeEscalation": false,
            "readOnlyRootFilesystem": false,
        },
    });

    if let Some(policy) = &target.image_pull_policy {
        container["imagePullPolicy"] = serde_json::Value::String(policy.clone());
    }

    let mut pod = serde_json::json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": name,
            "labels": {
                "app.kubernetes.io/managed-by": "gyre",
                "gyre-agent": name,
            },
        },
        "spec": {
            "containers": [container],
            // Spec: --network=none equivalent — no egress except what the
            // cluster admin explicitly grants. DNS is the minimum needed
            // for the agent to phone home; network policies are the
            // cluster-side control.
            "restartPolicy": "Never",
        },
    });

    // serviceAccountName is a Pod-spec field, NOT a container field —
    // server-side validation (strict by default since k8s 1.27) rejects
    // unknown container fields, which would fail the whole apply.
    if let Some(sa) = &target.service_account {
        pod["spec"]["serviceAccountName"] = serde_json::Value::String(sa.clone());
    }

    pod
}

/// Extract a sanitized Pod name: RFC 1123 subdomain (lowercase alphanumeric,
/// '-' or '.', max 253). The server validates agent names as
/// `[a-zA-Z0-9._-]` before reaching here; this lowercases and truncates
/// defensively.
fn pod_name(name: &str) -> String {
    let lowered = name.to_lowercase();
    let sanitized: String = lowered
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    // Char-boundary-safe truncation to the RFC 1123 max length (253).
    // The sanitized string is ASCII-only by construction, but take()
    // over chars keeps that invariant local instead of relying on it.
    let mut n: String = sanitized.chars().take(253).collect();
    // Must start and end with an alphanumeric.
    while n.starts_with('-') || n.starts_with('.') {
        n.remove(0);
    }
    while n.ends_with('-') || n.ends_with('.') {
        n.pop();
    }
    n
}

#[async_trait]
impl ComputeTarget for KubernetesTarget {
    fn name(&self) -> &str {
        "kubernetes"
    }

    fn target_type(&self) -> &'static str {
        "kubernetes"
    }

    async fn spawn_process(&self, config: &SpawnConfig) -> Result<ProcessHandle> {
        let name = pod_name(&config.name);
        if name.is_empty() {
            return Err(anyhow!(
                "agent name '{}' yields an empty Kubernetes Pod name",
                config.name
            ));
        }
        let manifest = build_pod_manifest(self, config);

        // Apply via stdin: `kubectl apply -f -`. Args are separate argv
        // entries — no shell interpolation of user-controlled strings.
        let manifest_str = serde_json::to_string(&manifest).context("serializing Pod manifest")?;
        let mut cmd = self.kubectl();
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.arg("apply").arg("-f").arg("-");

        let mut child = cmd
            .spawn()
            .context("kubectl apply failed — is kubectl installed?")?;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin
                .write_all(manifest_str.as_bytes())
                .await
                .context("writing Pod manifest to kubectl stdin")?;
            stdin.shutdown().await.context("closing kubectl stdin")?;
        }
        let output = child
            .wait_with_output()
            .await
            .context("waiting for kubectl apply")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("kubectl apply failed: {}", stderr));
        }

        // Wait for the Pod to reach a scheduled state (best-effort, bounded).
        // `kubectl wait --for=condition=PodScheduled` returns immediately
        // when already scheduled; we tolerate NotFound races with a short
        // retry loop.
        let mut scheduled = false;
        for attempt in 0..3 {
            let status = self
                .kubectl()
                .arg("wait")
                .arg("--for=condition=PodScheduled")
                .arg("--timeout=10s")
                .arg(format!("pod/{name}"))
                .status()
                .await;
            match status {
                Ok(s) if s.success() => {
                    scheduled = true;
                    break;
                }
                _ => {
                    if attempt == 2 {
                        tracing::warn!(pod = %name, "pod not observed scheduled within timeout; continuing");
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }
        let _ = scheduled;

        Ok(ProcessHandle {
            id: name,
            target_type: "kubernetes".to_string(),
            pid: None,
            // Kubernetes kills are reconstructed from the pod name (id) via
            // kubectl on the server host — no per-target connection details.
            options: std::collections::HashMap::new(),
        })
    }

    async fn kill_process(&self, handle: &ProcessHandle) -> Result<()> {
        let output = tokio::time::timeout(
            KUBECTL_DELETE_TIMEOUT,
            self.kubectl()
                .kill_on_drop(true)
                .arg("delete")
                .arg("pod")
                .arg(&handle.id)
                .arg("--grace-period=30")
                .output(),
        )
        .await
        .map_err(|_| {
            anyhow!(
                "kubectl delete pod {} timed out after {:?} — cluster unreachable or stalled",
                handle.id,
                KUBECTL_DELETE_TIMEOUT
            )
        })?
        .with_context(|| format!("kubectl delete pod {} failed", handle.id))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // A pod that does not exist is already gone — a successful
            // kill. EVERYTHING else (wrong namespace, missing context,
            // unreachable cluster, RBAC denial) is a REAL failure that
            // must surface: callers mark the instance terminated only
            // after a verified kill, and a warn-and-return-Ok here would
            // permanently hide a live orphan behind a "reaped" record
            // (review task-273).
            if pod_already_gone(&stderr) {
                tracing::info!(pod = %handle.id, "pod already gone (NotFound)");
                return Ok(());
            }
            return Err(anyhow!(
                "kubectl delete pod {} failed: {}",
                handle.id,
                stderr.trim()
            ));
        }
        Ok(())
    }

    async fn is_alive(&self, handle: &ProcessHandle) -> Result<bool> {
        let output = self
            .kubectl()
            .arg("get")
            .arg("pod")
            .arg(&handle.id)
            .arg("-o")
            .arg("jsonpath={.status.phase}")
            .output()
            .await
            .with_context(|| format!("kubectl get pod {} failed", handle.id))?;
        if !output.status.success() {
            return Ok(false);
        }
        let phase = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // Pending/Running are alive; Succeeded/Failed/Unknown are not.
        Ok(phase == "Pending" || phase == "Running")
    }
}

/// Build a [`KubernetesTarget`] from a compute-target config blob.
///
/// Recognized keys: `image`, `namespace`, `service_account`,
/// `memory_limit`, `cpu_limit`, `image_pull_policy`, `context`.
/// Absent keys fall back to the spec's security defaults (memory 2Gi).
pub fn kubernetes_target_from_config(config: &serde_json::Value) -> KubernetesTarget {
    let image = config
        .get("image")
        .and_then(|v| v.as_str())
        .unwrap_or("gyre-agent:latest");
    let mut t = KubernetesTarget::new(image);
    if let Some(ns) = config.get("namespace").and_then(|v| v.as_str()) {
        t = t.with_namespace(ns);
    }
    if let Some(sa) = config.get("service_account").and_then(|v| v.as_str()) {
        t = t.with_service_account(sa);
    }
    t.memory_limit = config
        .get("memory_limit")
        .and_then(|v| v.as_str())
        .map(String::from);
    t.cpu_limit = config
        .get("cpu_limit")
        .and_then(|v| v.as_str())
        .map(String::from);
    if let Some(p) = config.get("image_pull_policy").and_then(|v| v.as_str()) {
        t = t.with_image_pull_policy(p);
    }
    if let Some(c) = config.get("context").and_then(|v| v.as_str()) {
        t = t.with_context(c);
    }
    if let Some(p) = config.get("kubectl_path").and_then(|v| v.as_str()) {
        t = t.with_kubectl_path(p);
    }
    t
}

/// Whether a failed `kubectl delete` stderr means the pod does not exist
/// (already gone — a successful kill) as opposed to a real failure
/// (unreachable cluster, wrong namespace, RBAC denial, …).
/// `Error from server (NotFound): pods "x" not found` is kubectl's
/// already-gone shape (review task-273: only this class may be treated as
/// success; every other non-zero exit must fail the kill so the caller
/// does not mark a live orphan terminated).
fn pod_already_gone(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    lower.contains("notfound") || lower.contains("not found")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn spawn_config(name: &str) -> SpawnConfig {
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

    #[test]
    fn pod_name_sanitizes_and_lowercases() {
        assert_eq!(pod_name("My-Agent_1"), "my-agent_1".replace('_', "-"));
        assert_eq!(pod_name("--lead--"), "lead");
        assert_eq!(pod_name("---"), "");
    }

    #[test]
    fn pod_name_truncates_to_253() {
        let long = "a".repeat(300);
        let n = pod_name(&long);
        assert!(n.len() <= 253);
    }

    #[test]
    fn manifest_has_security_defaults() {
        let t = KubernetesTarget::new("gyre-agent:test");
        let m = build_pod_manifest(&t, &spawn_config("agent-1"));
        let c = &m["spec"]["containers"][0];
        // Spec security defaults: non-root 65534:65534, memory 2g,
        // no privilege escalation.
        assert_eq!(c["securityContext"]["runAsUser"], 65534);
        assert_eq!(c["securityContext"]["runAsGroup"], 65534);
        assert_eq!(c["securityContext"]["runAsNonRoot"], true);
        assert_eq!(c["securityContext"]["allowPrivilegeEscalation"], false);
        assert_eq!(c["resources"]["limits"]["memory"], "2Gi");
        // Env vars injected into the container.
        assert_eq!(c["env"][0]["name"], "GYRE_AGENT_ID");
        assert_eq!(c["env"][0]["value"], "a-1");
        assert_eq!(c["command"][0], "/gyre/entrypoint.sh");
        assert_eq!(m["metadata"]["name"], "agent-1");
    }

    #[test]
    fn manifest_service_account_and_overrides() {
        let t = KubernetesTarget::new("gyre-agent:test")
            .with_namespace("agents")
            .with_service_account("gyre-agent")
            .with_memory_limit("4Gi")
            .with_cpu_limit("4");
        let m = build_pod_manifest(&t, &spawn_config("agent-2"));
        assert_eq!(m["spec"]["serviceAccountName"], "gyre-agent");
        assert_eq!(
            m["spec"]["containers"][0]["resources"]["limits"]["memory"],
            "4Gi"
        );
        assert_eq!(
            m["spec"]["containers"][0]["resources"]["limits"]["cpu"],
            "4"
        );
        // serviceAccountName must NOT appear at the container level —
        // strict server-side validation rejects unknown container fields,
        // which would fail the whole apply.
        assert!(
            m["spec"]["containers"][0]
                .get("serviceAccountName")
                .is_none(),
            "serviceAccountName leaked into container spec"
        );
    }

    /// The manifest name must be the same sanitized Pod name that
    /// spawn_process puts in the ProcessHandle — otherwise kill/is_alive
    /// address a Pod that never exists (or the apply is rejected for an
    /// invalid name).
    #[test]
    fn manifest_name_matches_handle_name_for_names_needing_sanitization() {
        let t = KubernetesTarget::new("gyre-agent:test");
        let cfg = spawn_config("My_Agent.1");
        let m = build_pod_manifest(&t, &cfg);
        let expected = pod_name(&cfg.name);
        assert_eq!(expected, "my-agent.1");
        assert_eq!(m["metadata"]["name"], expected);
        assert_eq!(m["metadata"]["labels"]["gyre-agent"], expected);
    }

    #[test]
    fn from_config_maps_fields() {
        let cfg = serde_json::json!({
            "image": "reg.example/gyre-agent@sha256:abc",
            "namespace": "gyre",
            "service_account": "agent-sa",
            "memory_limit": "1Gi",
            "cpu_limit": "1",
            "context": "prod",
            "kubectl_path": "/opt/bin/kubectl"
        });
        let t = kubernetes_target_from_config(&cfg);
        assert_eq!(t.image, "reg.example/gyre-agent@sha256:abc");
        assert_eq!(t.namespace.as_deref(), Some("gyre"));
        assert_eq!(t.service_account.as_deref(), Some("agent-sa"));
        assert_eq!(t.memory_limit.as_deref(), Some("1Gi"));
        assert_eq!(t.cpu_limit.as_deref(), Some("1"));
        assert_eq!(t.context.as_deref(), Some("prod"));
        assert_eq!(t.kubectl_path.as_deref(), Some("/opt/bin/kubectl"));
    }

    #[test]
    fn from_config_defaults() {
        let t = kubernetes_target_from_config(&serde_json::json!({}));
        assert_eq!(t.image, "gyre-agent:latest");
        assert!(t.namespace.is_none());
        assert!(t.memory_limit.is_none());
        assert!(
            t.kubectl_path.is_none(),
            "no config kubectl_path must fall back to kubectl from PATH"
        );
    }

    #[test]
    fn pod_already_gone_matches_only_not_found_stderr() {
        // Already-gone shapes (successful kill).
        assert!(pod_already_gone(
            "Error from server (NotFound): pods \"agent-1\" not found"
        ));
        assert!(pod_already_gone("error: pods \"x\" not found"));
        // Real failures must NOT be treated as already-gone.
        assert!(!pod_already_gone("error: context \"prod\" does not exist"));
        assert!(!pod_already_gone(
            "Error from server (Forbidden): pods \"agent-1\" is forbidden"
        ));
        assert!(!pod_already_gone(
            "The connection to the server 10.0.0.1:6443 was refused"
        ));
        assert!(!pod_already_gone(""));
    }

    /// review task-273: kill_process must distinguish an already-gone pod
    /// (NotFound → Ok) from a real failure (anything else → Err) — a
    /// warn-and-return-Ok on every non-zero exit let a wrong-namespace or
    /// unreachable-cluster delete be recorded as a successful kill while
    /// the orphan pod kept running. Drives the real kubectl invocation
    /// through a stub binary via `kubectl_path`.
    #[tokio::test]
    async fn kill_process_not_found_is_ok_real_failure_is_err() {
        let stub = write_stub_kubectl(
            "if echo \"$*\" | grep -q 'already-gone'; then
                 echo 'Error from server (NotFound): pods already-gone not found' >&2
                 exit 1
             fi
             echo 'The connection to the server was refused' >&2
             exit 1",
        );
        let target = KubernetesTarget::new("gyre-agent:test").with_kubectl_path(&stub);

        let gone = ProcessHandle {
            id: "already-gone".to_string(),
            target_type: "kubernetes".to_string(),
            pid: None,
            options: Default::default(),
        };
        target.kill_process(&gone).await.unwrap();

        let live = ProcessHandle {
            id: "still-running".to_string(),
            target_type: "kubernetes".to_string(),
            pid: None,
            options: Default::default(),
        };
        let err = target.kill_process(&live).await.unwrap_err();
        assert!(
            err.to_string().contains("still-running"),
            "a real delete failure must name the pod, got: {err}"
        );
    }

    /// review task-273: the kill must address the pod in the namespace
    /// (and context) the target was constructed with — the
    /// remote_agent_cleanup sweep reconstructs the target from the
    /// durable record's backend_config, and a default-constructed target
    /// without the persisted namespace deletes from the wrong place.
    #[tokio::test]
    async fn kill_process_addresses_persisted_namespace_and_context() {
        let (stub, recorded) = write_recording_kubectl();
        let target = KubernetesTarget::new("gyre-agent:test")
            .with_kubectl_path(&stub)
            .with_namespace("gyre-agents")
            .with_context("prod");
        let handle = ProcessHandle {
            id: "agent-pod-7".to_string(),
            target_type: "kubernetes".to_string(),
            pid: None,
            options: Default::default(),
        };
        target.kill_process(&handle).await.unwrap();
        let invoked = std::fs::read_to_string(recorded).unwrap();
        assert!(
            invoked.contains("--namespace gyre-agents") && invoked.contains("--context prod"),
            "the kill must carry the persisted namespace/context, invoked: {invoked}"
        );
        assert!(
            invoked.contains("delete pod agent-pod-7"),
            "the kill must delete the recorded pod, invoked: {invoked}"
        );
    }

    /// review task-273: a hung kubectl / API server must fail the kill
    /// BOUNDED, not stall the sequential remote_agent_cleanup /
    /// admin-kill sweep forever.
    #[tokio::test]
    async fn kill_hung_kubectl_times_out() {
        let stub = write_stub_kubectl("sleep 30");
        let target = KubernetesTarget::new("gyre-agent:test").with_kubectl_path(&stub);
        let handle = ProcessHandle {
            id: "hung-pod".to_string(),
            target_type: "kubernetes".to_string(),
            pid: None,
            options: Default::default(),
        };
        let err = target.kill_process(&handle).await.unwrap_err();
        assert!(
            err.to_string().contains("timed out"),
            "a hung kubectl must fail the kill bounded, got: {err:#}"
        );
    }

    /// Write an executable stub `kubectl` that exits per `script`.
    fn write_stub_kubectl(script: &str) -> String {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kubectl");
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        make_executable(&path);
        // Leak the tempdir: the stub must outlive this function (the
        // target invokes it asynchronously afterwards).
        std::mem::forget(dir);
        path.to_str().unwrap().to_string()
    }

    /// Write an executable stub `kubectl` that records its arguments to
    /// `recorded` (a file next to the stub) and exits 0. Returns
    /// (kubectl_path, recorded_path).
    fn write_recording_kubectl() -> (String, String) {
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("kubectl");
        let recorded = dir.path().join("invoked.txt");
        std::fs::write(
            &stub,
            format!("#!/bin/sh\necho \"$@\" > {}\n", recorded.to_str().unwrap()),
        )
        .unwrap();
        make_executable(&stub);
        std::mem::forget(dir);
        (
            stub.to_str().unwrap().to_string(),
            recorded.to_str().unwrap().to_string(),
        )
    }

    fn make_executable(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }

    /// `kubectl_path` must drive the actual binary spawn_process invokes —
    /// a stub kubectl that records the applied manifest proves the config
    /// override reaches the Command invocation (and that the manifest is
    /// really applied via stdin).
    #[tokio::test]
    async fn spawn_uses_configured_kubectl_path() {
        let dir = tempfile::tempdir().unwrap();
        let dump = dir
            .path()
            .join("manifest.json")
            .to_string_lossy()
            .into_owned();
        let stub = dir
            .path()
            .join("kubectl-stub")
            .to_string_lossy()
            .into_owned();
        std::fs::write(
            &stub,
            format!("#!/bin/sh\nif [ \"$1\" = \"apply\" ]; then {{ cat; echo; }} > {dump}; exit 0; fi\nexit 1\n"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let t = kubernetes_target_from_config(&serde_json::json!({
            "kubectl_path": stub
        }));
        let handle = gyre_ports::ComputeTarget::spawn_process(&t, &spawn_config("path-agent"))
            .await
            .expect("spawn via stub kubectl must succeed");
        assert_eq!(handle.id, pod_name("path-agent"));

        let manifest = std::fs::read_to_string(&dump).expect("stub must capture the manifest");
        let m: serde_json::Value =
            serde_json::from_str(manifest.trim_end()).expect("captured stdin is the Pod manifest");
        assert_eq!(m["kind"], "Pod");
        assert_eq!(m["metadata"]["name"], pod_name("path-agent"));
    }

    /// spawn_process must fail fast when kubectl is absent — proves the
    /// backend invokes a real binary and propagates failure (agent-runtime
    /// §1 spawn-failure path).
    #[tokio::test]
    async fn spawn_without_kubectl_errors() {
        // Only meaningful where kubectl is genuinely absent; skip when the
        // environment has kubectl installed (CI runners may).
        if which_exists("kubectl") {
            return;
        }
        let t = KubernetesTarget::new("gyre-agent:test");
        let res = t.spawn_process(&spawn_config("agent-x")).await;
        assert!(res.is_err(), "spawn must fail when kubectl is missing");
    }

    fn which_exists(bin: &str) -> bool {
        std::env::var_os("PATH")
            .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).exists()))
            .unwrap_or(false)
    }
}
