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
/// Spec §3 container security defaults are enforced in the created API
/// objects: the Pod manifest carries the memory / PID / non-root defaults
/// (`build_pod_manifest`), and — unless the compute target config opts
/// into bridged networking (`"network": "bridge"`) — the backend applies
/// a default-deny NetworkPolicy alongside the Pod in the same
/// `kubectl apply` stream (`build_network_policy_manifest`), the
/// `--network=none` equivalent. `kill_process` deletes both objects.
///
/// `kill_process` deletes the Pod (and its NetworkPolicy); `is_alive`
/// checks the Pod phase.
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
    /// PID limit (spec §3 `--pids-limit=512` equivalent). `None` = 512
    /// (spec default). Expressed in the created Pod object as the
    /// container resource limit `pids`.
    pub pids_limit: Option<u32>,
    /// Network mode override. `None` or `"none"` (spec default) = the
    /// backend creates a default-deny NetworkPolicy alongside the Pod
    /// (the `--network=none` equivalent). `"bridge"` opts in to cluster
    /// networking — no NetworkPolicy is created, matching the container
    /// backend's `--network=bridge` opt-in.
    pub network: Option<String>,
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
            pids_limit: None,
            network: None,
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

    /// Override the PID limit (spec default: 512).
    pub fn with_pids_limit(mut self, limit: u32) -> Self {
        self.pids_limit = Some(limit);
        self
    }

    /// Override the network mode (spec default: isolated — default-deny
    /// NetworkPolicy). Example: `.with_network("bridge")` to opt in to
    /// cluster networking.
    pub fn with_network(mut self, network: impl Into<String>) -> Self {
        self.network = Some(network.into());
        self
    }

    /// Whether the backend applies the spec's network-isolation default:
    /// every network mode except the explicit bridge opt-in isolates the
    /// Pod behind a platform-created default-deny NetworkPolicy.
    fn applies_network_isolation(&self) -> bool {
        self.network.as_deref().unwrap_or("none") != "bridge"
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
    // Spec security defaults (agent-runtime.md §3): --memory=2g and
    // --pids-limit=512 equivalents, expressed in the created Pod object
    // as the container resource limits.
    let memory = target.memory_limit.as_deref().unwrap_or("2Gi");
    let cpu = target.cpu_limit.as_deref().unwrap_or("2");
    let pids = target.pids_limit.unwrap_or(512);

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
                // Spec: --pids-limit=512 equivalent.
                "pids": pids.to_string(),
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

/// Deterministic NetworkPolicy name for an agent Pod: the sanitized pod
/// name plus a suffix, truncated to the RFC 1123 subdomain max (253).
/// `pod_name` output is ASCII by construction; `chars().take()` keeps the
/// truncation char-boundary-safe regardless.
fn network_policy_name(pod_name: &str) -> String {
    const SUFFIX: &str = "-net-isolate";
    let base: String = pod_name.chars().take(253 - SUFFIX.len()).collect();
    format!("{base}{SUFFIX}")
}

/// Build the default-deny NetworkPolicy applied alongside the Pod — the
/// spec §3 `--network=none` equivalent (agent-runtime.md §3).
///
/// The policy selects exactly this agent's Pod (via the manifest's
/// `gyre-agent` label) and carries empty `ingress`/`egress` rule lists
/// with both policy types: a NetworkPolicy with an empty rule list for a
/// policy type denies ALL traffic of that type, so the created objects
/// deny all ingress and all egress for the Pod. Network access beyond
/// that exists only when the compute target config opts in
/// (`"network": "bridge"` — `spawn_process` then applies no policy and
/// the Pod uses cluster networking).
///
/// The platform creates and deletes this object with the Pod
/// (`kill_process` removes both) — isolation is not deferred to the
/// cluster admin.
fn build_network_policy_manifest(pod_name: &str) -> serde_json::Value {
    serde_json::json!({
        "apiVersion": "networking.k8s.io/v1",
        "kind": "NetworkPolicy",
        "metadata": {
            "name": network_policy_name(pod_name),
            "labels": {
                "app.kubernetes.io/managed-by": "gyre",
                "gyre-agent": pod_name,
            },
        },
        "spec": {
            // Select exactly this agent's Pod — never other pods in the
            // namespace.
            "podSelector": {
                "matchLabels": {"gyre-agent": pod_name},
            },
            "policyTypes": ["Ingress", "Egress"],
            // Empty rule lists = deny all traffic of each policy type.
            "ingress": [],
            "egress": [],
        },
    })
}

/// The `kubectl apply -f -` stdin stream for a spawn: the Pod manifest,
/// plus the default-deny NetworkPolicy unless the compute target config
/// opted into bridged networking. Documents are separated as a YAML
/// stream (`---`); each serialized JSON object is a valid YAML document.
fn build_apply_stream(target: &KubernetesTarget, config: &SpawnConfig) -> Result<String> {
    let pod = build_pod_manifest(target, config);
    let mut stream = serde_json::to_string(&pod).context("serializing Pod manifest")?;
    if target.applies_network_isolation() {
        let policy = build_network_policy_manifest(&pod_name(&config.name));
        let policy_str =
            serde_json::to_string(&policy).context("serializing NetworkPolicy manifest")?;
        stream.push_str("\n---\n");
        stream.push_str(&policy_str);
    }
    Ok(stream)
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
        let manifest_str = build_apply_stream(self, config)?;
        let mut cmd = self.kubectl();
        // Apply via stdin: `kubectl apply -f -`. Args are separate argv
        // entries — no shell interpolation of user-controlled strings.
        // The stream carries the Pod and (unless the target opted into
        // bridged networking) the default-deny NetworkPolicy.
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
        // Delete the Pod first (stops the workload), then the per-Pod
        // default-deny NetworkPolicy the platform created alongside it —
        // both best-effort: already-gone objects are a successful kill.
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
        let policy = network_policy_name(&handle.id);
        let status = self
            .kubectl()
            .arg("delete")
            .arg("networkpolicy")
            .arg(&policy)
            .status()
            .await
            .with_context(|| format!("kubectl delete networkpolicy {policy} failed"))?;
        if !status.success() {
            // A leaked default-deny policy selects a deleted Pod and is
            // inert, but litter must not accumulate silently.
            tracing::warn!(network_policy = %policy, "kubectl delete networkpolicy returned non-zero");
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
/// `memory_limit`, `cpu_limit`, `pids_limit`, `network` (`"bridge"`
/// opts in to cluster networking; absent/`"none"` keeps the
/// default-deny NetworkPolicy isolation), `image_pull_policy`,
/// `context`.
/// Absent keys fall back to the spec's security defaults (memory 2Gi,
/// pids 512, network isolation).
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
    t.pids_limit = config
        .get("pids_limit")
        .and_then(|v| v.as_u64())
        .map(|p| p as u32);
    if let Some(n) = config.get("network").and_then(|v| v.as_str()) {
        t = t.with_network(n);
    }
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
        // pids 512, no privilege escalation.
        assert_eq!(c["securityContext"]["runAsUser"], 65534);
        assert_eq!(c["securityContext"]["runAsGroup"], 65534);
        assert_eq!(c["securityContext"]["runAsNonRoot"], true);
        assert_eq!(c["securityContext"]["allowPrivilegeEscalation"], false);
        assert_eq!(c["resources"]["limits"]["memory"], "2Gi");
        // The pids-limit=512 equivalent rides the created Pod object as
        // the container resource limit.
        assert_eq!(
            c["resources"]["limits"]["pids"], "512",
            "default manifest must carry the 512 pids limit: {m}"
        );
        // Env vars injected into the container.
        assert_eq!(c["env"][0]["name"], "GYRE_AGENT_ID");
        assert_eq!(c["env"][0]["value"], "a-1");
        assert_eq!(c["command"][0], "/gyre/entrypoint.sh");
        assert_eq!(m["metadata"]["name"], "agent-1");
    }

    /// The pids limit must be overridable per compute target (the same
    /// override surface as memory/cpu), and the override must REPLACE the
    /// 512 default, not ride alongside it.
    #[test]
    fn manifest_pids_limit_override_replaces_default() {
        let t = KubernetesTarget::new("gyre-agent:test").with_pids_limit(256);
        let m = build_pod_manifest(&t, &spawn_config("agent-1"));
        assert_eq!(
            m["spec"]["containers"][0]["resources"]["limits"]["pids"],
            "256"
        );
    }

    /// The default-deny NetworkPolicy is the --network=none equivalent:
    /// both policy types with EMPTY rule lists deny all traffic, and the
    /// podSelector must scope the policy to exactly this agent's Pod —
    /// a policy that selected the whole namespace would deny traffic for
    /// unrelated pods.
    #[test]
    fn network_policy_is_default_deny_and_scoped_to_the_pod() {
        let p = build_network_policy_manifest("agent-1");
        assert_eq!(p["apiVersion"], "networking.k8s.io/v1");
        assert_eq!(p["kind"], "NetworkPolicy");
        assert_eq!(p["metadata"]["name"], "agent-1-net-isolate");
        assert_eq!(
            p["spec"]["podSelector"]["matchLabels"]["gyre-agent"], "agent-1",
            "policy must select exactly the agent Pod"
        );
        assert_eq!(
            p["spec"]["policyTypes"],
            serde_json::json!(["Ingress", "Egress"])
        );
        assert_eq!(p["spec"]["ingress"], serde_json::json!([]));
        assert_eq!(p["spec"]["egress"], serde_json::json!([]));
    }

    /// The NetworkPolicy name must stay a valid RFC 1123 subdomain for any
    /// sanitized pod name: bounded length, suffix intact — kill_process
    /// derives the same name from the handle id, so a mismatch would leak
    /// the policy.
    #[test]
    fn network_policy_name_stays_dns1123_for_long_names() {
        let long_pod = "a".repeat(300);
        let name = network_policy_name(&long_pod);
        assert!(name.len() <= 253, "name must be ≤253 chars: {name}");
        assert!(name.ends_with("-net-isolate"));
        assert_eq!(network_policy_name("agent-1"), "agent-1-net-isolate");
    }

    /// The default apply stream is a two-document YAML stream: Pod +
    /// default-deny NetworkPolicy. The bridge opt-in must apply the Pod
    /// ALONE — an isolation policy applied alongside an opted-in Pod
    /// would silently re-isolate it.
    #[test]
    fn apply_stream_carries_policy_by_default_and_pod_only_on_bridge() {
        let isolated = KubernetesTarget::new("gyre-agent:test");
        let stream = build_apply_stream(&isolated, &spawn_config("agent-1")).unwrap();
        let docs: Vec<&str> = stream.split("\n---\n").collect();
        assert_eq!(
            docs.len(),
            2,
            "default stream: Pod + NetworkPolicy\n{stream}"
        );
        let pod: serde_json::Value = serde_json::from_str(docs[0]).unwrap();
        let policy: serde_json::Value = serde_json::from_str(docs[1]).unwrap();
        assert_eq!(pod["kind"], "Pod");
        assert_eq!(policy["kind"], "NetworkPolicy");

        let bridged = KubernetesTarget::new("gyre-agent:test").with_network("bridge");
        let stream = build_apply_stream(&bridged, &spawn_config("agent-1")).unwrap();
        let docs: Vec<&str> = stream.split("\n---\n").collect();
        assert_eq!(
            docs.len(),
            1,
            "bridge opt-in must apply the Pod only\n{stream}"
        );
        let pod: serde_json::Value = serde_json::from_str(docs[0]).unwrap();
        assert_eq!(pod["kind"], "Pod");
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
            "pids_limit": 256,
            "network": "bridge",
            "context": "prod",
            "kubectl_path": "/opt/bin/kubectl"
        });
        let t = kubernetes_target_from_config(&cfg);
        assert_eq!(t.image, "reg.example/gyre-agent@sha256:abc");
        assert_eq!(t.namespace.as_deref(), Some("gyre"));
        assert_eq!(t.service_account.as_deref(), Some("agent-sa"));
        assert_eq!(t.memory_limit.as_deref(), Some("1Gi"));
        assert_eq!(t.cpu_limit.as_deref(), Some("1"));
        assert_eq!(t.pids_limit, Some(256));
        assert_eq!(t.network.as_deref(), Some("bridge"));
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
            t.pids_limit.is_none(),
            "no config pids_limit must fall back to the spec 512 default"
        );
        assert!(
            t.network.is_none(),
            "no config network must keep default-deny isolation"
        );
        assert!(
            t.kubectl_path.is_none(),
            "no config kubectl_path must fall back to kubectl from PATH"
        );
    }

    /// `kubectl_path` must drive the actual binary spawn_process invokes —
    /// a stub kubectl that records the applied stream proves the config
    /// override reaches the Command invocation (and that the Pod AND its
    /// default-deny NetworkPolicy are really applied via stdin as one
    /// YAML stream).
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

        let captured = std::fs::read_to_string(&dump).expect("stub must capture the stream");
        let docs: Vec<serde_json::Value> = captured
            .trim_end()
            .split("\n---\n")
            .map(|d| serde_json::from_str(d).expect("each stream document must be JSON"))
            .collect();
        assert_eq!(
            docs.len(),
            2,
            "applied stream must carry the Pod and its NetworkPolicy:\n{captured}"
        );
        assert_eq!(docs[0]["kind"], "Pod");
        assert_eq!(docs[0]["metadata"]["name"], pod_name("path-agent"));
        assert_eq!(docs[1]["kind"], "NetworkPolicy");
        assert_eq!(
            docs[1]["metadata"]["name"],
            network_policy_name(&pod_name("path-agent"))
        );
        // The applied Pod carries the spec security defaults end-to-end
        // (manifest builder -> serialized -> applied stream).
        assert_eq!(
            docs[0]["spec"]["containers"][0]["resources"]["limits"]["pids"],
            "512"
        );
    }

    /// The bridge opt-in must reach the executed kubectl invocation: the
    /// captured apply stream carries the Pod ALONE — no NetworkPolicy.
    #[tokio::test]
    async fn spawn_bridge_opt_in_applies_pod_only() {
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
            "kubectl_path": stub,
            "network": "bridge"
        }));
        gyre_ports::ComputeTarget::spawn_process(&t, &spawn_config("bridge-agent"))
            .await
            .expect("spawn via stub kubectl must succeed");

        let captured = std::fs::read_to_string(&dump).expect("stub must capture the stream");
        let docs: Vec<serde_json::Value> = captured
            .trim_end()
            .split("\n---\n")
            .map(|d| serde_json::from_str(d).expect("each stream document must be JSON"))
            .collect();
        assert_eq!(docs.len(), 1, "bridge opt-in stream:\n{captured}");
        assert_eq!(docs[0]["kind"], "Pod");
    }

    /// kill_process must delete BOTH platform-created objects — the Pod and
    /// its NetworkPolicy. A kill that left the policy behind would litter
    /// the namespace with deny-all policies selecting deleted Pods. The
    /// stub records every kubectl invocation's argv for the assertions.
    #[tokio::test]
    async fn kill_deletes_pod_and_network_policy() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir
            .path()
            .join("kubectl.log")
            .to_string_lossy()
            .into_owned();
        let stub = dir
            .path()
            .join("kubectl-stub")
            .to_string_lossy()
            .into_owned();
        // Log "$*" per invocation, one line each; apply also cats stdin.
        std::fs::write(
            &stub,
            format!(
                "#!/bin/sh\nprintf '%s\n' \"$*\" >> {log}\nif [ \"$1\" = \"apply\" ]; then cat >/dev/null; fi\nexit 0\n"
            ),
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
        let handle = gyre_ports::ComputeTarget::spawn_process(&t, &spawn_config("kill-agent"))
            .await
            .expect("spawn via stub kubectl must succeed");
        gyre_ports::ComputeTarget::kill_process(&t, &handle)
            .await
            .expect("kill via stub kubectl must succeed");

        let log = std::fs::read_to_string(&log).expect("stub must record invocations");
        let pod_delete = format!("delete pod {} --grace-period=30", handle.id);
        let policy_delete = format!("delete networkpolicy {}", network_policy_name(&handle.id));
        assert!(
            log.lines().any(|l| l == pod_delete),
            "kill must delete the Pod; log:\n{log}"
        );
        assert!(
            log.lines().any(|l| l == policy_delete),
            "kill must delete the Pod's NetworkPolicy; log:\n{log}"
        );
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
