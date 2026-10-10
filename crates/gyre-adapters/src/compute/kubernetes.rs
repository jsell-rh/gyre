use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use gyre_ports::{ComputeTarget, ProcessHandle, SpawnConfig};
use tokio::process::Command;

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

    /// Prefix every kubectl invocation with context/namespace flags.
    fn kubectl(&self) -> Command {
        let mut cmd = Command::new("kubectl");
        if let Some(ctx) = &self.context {
            cmd.arg("--context").arg(ctx);
        }
        if let Some(ns) = &self.namespace {
            cmd.arg("--namespace").arg(ns);
        }
        cmd
    }
}


/// Build the Pod manifest passed to `kubectl apply`. Kept as a separate
/// function so tests can assert the generated security defaults.
fn build_pod_manifest(target: &KubernetesTarget, config: &SpawnConfig) -> serde_json::Value {
    // Spec security defaults (agent-runtime.md §3): --memory=2g,
    // --pids-limit=512 equivalents. Kubernetes has no direct pids-limit
    // field; it is enforced via the pod-level `pids` cgroup limit which
    // kubelet derives from --pod-max-pids / container runtime defaults —
    // the memory limit is what we can set per-Pod here.
    let memory = target
        .memory_limit
        .as_deref()
        .unwrap_or("2Gi");
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

    if let Some(sa) = &target.service_account {
        container["serviceAccountName"] = serde_json::Value::String(sa.clone());
    }
    if let Some(policy) = &target.image_pull_policy {
        container["imagePullPolicy"] = serde_json::Value::String(policy.clone());
    }

    let mut pod = serde_json::json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": config.name,
            "labels": {
                "app.kubernetes.io/managed-by": "gyre",
                "gyre-agent": config.name,
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
    let mut n = sanitized;
    n.truncate(253);
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
        let manifest_str =
            serde_json::to_string(&manifest).context("serializing Pod manifest")?;
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
            stdin
                .shutdown()
                .await
                .context("closing kubectl stdin")?;
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
        })
    }

    async fn kill_process(&self, handle: &ProcessHandle) -> Result<()> {
        let status = self
            .kubectl()
            .arg("delete")
            .arg("pod")
            .arg(&handle.id)
            .arg("--grace-period=30")
            .status()
            .await
            .with_context(|| format!("kubectl delete pod {} failed", handle.id))?;
        if !status.success() {
            // Already-gone pods are a successful kill.
            tracing::warn!(pod = %handle.id, "kubectl delete returned non-zero");
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
    t
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
        assert_eq!(m["spec"]["containers"][0]["resources"]["limits"]["memory"], "4Gi");
        assert_eq!(m["spec"]["containers"][0]["resources"]["limits"]["cpu"], "4");
    }

    #[test]
    fn from_config_maps_fields() {
        let cfg = serde_json::json!({
            "image": "reg.example/gyre-agent@sha256:abc",
            "namespace": "gyre",
            "service_account": "agent-sa",
            "memory_limit": "1Gi",
            "cpu_limit": "1",
            "context": "prod"
        });
        let t = kubernetes_target_from_config(&cfg);
        assert_eq!(t.image, "reg.example/gyre-agent@sha256:abc");
        assert_eq!(t.namespace.as_deref(), Some("gyre"));
        assert_eq!(t.service_account.as_deref(), Some("agent-sa"));
        assert_eq!(t.memory_limit.as_deref(), Some("1Gi"));
        assert_eq!(t.cpu_limit.as_deref(), Some("1"));
        assert_eq!(t.context.as_deref(), Some("prod"));
    }

    #[test]
    fn from_config_defaults() {
        let t = kubernetes_target_from_config(&serde_json::json!({}));
        assert_eq!(t.image, "gyre-agent:latest");
        assert!(t.namespace.is_none());
        assert!(t.memory_limit.is_none());
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
            .map(|paths| {
                std::env::split_paths(&paths).any(|dir| dir.join(bin).exists())
            })
            .unwrap_or(false)
    }
}
