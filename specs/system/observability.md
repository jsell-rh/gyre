# Observability & Governance

## Tracing & Observability

- **OpenTelemetry (OTel)** tracing throughout - traces, metrics, logs.
- **Domain-oriented observability** - instrumentation follows domain boundaries, not just infrastructure. Traces should tell you what the system *did*, not just what the code *ran*.
- OTLP export via `OTEL_EXPORTER_OTLP_ENDPOINT` (e.g., `http://otel-collector:4317`).
- Prometheus metrics at `GET /metrics` (request count, duration, active agents, merge queue depth).

---

## Audit System

- **Total auditing** — everything that happens is captured. No exceptions.
- Every agent runtime includes an **eBPF program** capturing all system-level activity (syscalls, network, file access, process execution).
- All audit data streams back to the **central server** in real time.
- Server supports **forwarding to SIEM server(s)** (Splunk, Elastic, Sentinel, etc.) via `POST /api/v1/admin/siem`.

---

## Agent Auditability

- Every single action an agent takes has an audit trail, **traceable from start to finish**.
- The **entire model context window** is captured and stored for auditability - full replay of what the agent saw and decided.

---

## Audit Event Schema

All audit events share a common envelope:

```rust
pub struct AuditEvent {
    pub id: Id,
    pub event_type: AuditEventType,
    pub agent_id: Option<Id>,       // Null for server-initiated events
    pub user_id: Option<Id>,        // Null for agent-only events
    pub session_id: Option<String>, // WebSocket or HTTP session
    pub workspace_id: Option<Id>,
    pub repo_id: Option<Id>,
    pub resource_type: String,      // "agent", "task", "mr", "repo", "container", etc.
    pub resource_id: Option<String>,
    pub outcome: AuditOutcome,      // Success, Failure, Blocked
    pub detail: serde_json::Value,  // Event-specific payload
    pub source_ip: Option<String>,
    pub user_agent: Option<String>,
    pub timestamp: u64,
}

pub enum AuditOutcome {
    Success,
    Failure,
    Blocked,
}
```

### Audit Event Types

#### Agent Lifecycle Events

| Event Type | Trigger | `detail` fields |
|---|---|---|
| `AgentSpawned` | `POST /api/v1/agents/spawn` succeeds | `name`, `task_id`, `compute_target`, `branch`, `spawned_by` |
| `AgentCompleted` | `POST /api/v1/agents/{id}/complete` succeeds | `task_id`, `mr_id`, `branch` |
| `AgentKilled` | Admin kills agent | `reason`, `killed_by`, `pid` |
| `AgentDead` | Stale agent detector marks agent Dead | `last_heartbeat_at`, `pid_alive` |
| `AgentHeartbeat` | `PUT /api/v1/agents/{id}/heartbeat` | `pid`, `pid_alive` |
| `AgentTokenRevoked` | Token revoked on complete or admin action | `jti`, `reason` |
| `AgentReassigned` | Admin reassigns agent's task | `old_task_id`, `new_task_id`, `reassigned_by` |

#### Container Lifecycle Events

Container agents produce additional lifecycle events beyond agent events. These are critical for supply-chain auditing — who ran what image, when, and what happened to it.

| Event Type | Trigger | `detail` fields |
|---|---|---|
| `ContainerStarted` | Container runtime successfully starts the container | `container_id`, `image`, `image_hash` (SHA-256 digest), `runtime` (docker/podman), `security_opts` (network, memory, pids_limit, user) |
| `ContainerStopped` | Container exits with code 0 (normal completion) | `container_id`, `image`, `image_hash`, `exit_code`, `duration_secs` |
| `ContainerKilled` | Container killed by admin or timeout | `container_id`, `image`, `image_hash`, `signal`, `killed_by` |
| `ContainerFailed` | Container exits with non-zero exit code | `container_id`, `image`, `image_hash`, `exit_code`, `stderr_tail` (last 2 KB) |
| `ContainerOOM` | Container killed by OOM killer | `container_id`, `image`, `image_hash`, `memory_limit_bytes`, `memory_used_bytes` |
| `ContainerNetworkBlocked` | Agent attempted outbound network when `--network=none` | `container_id`, `image`, `destination_ip`, `destination_port`, `syscall` |
| `ContainerImagePulled` | Docker/Podman pulls a new image layer | `image`, `image_hash`, `registry`, `size_bytes` |
| `ContainerImageRejected` | Image rejected (hash mismatch, policy denial) | `image`, `expected_hash`, `actual_hash`, `rejection_reason` |
| `ContainerSeccompViolation` | Agent syscall blocked by seccomp profile | `container_id`, `syscall_name`, `syscall_nr`, `pid_in_container` |
| `ContainerCapabilityUsed` | Agent uses a Linux capability | `container_id`, `capability` (e.g., `CAP_NET_RAW`), `allowed` |

**Why container lifecycle events matter:**

- `ContainerStarted` + `image_hash` is the AI Bill of Materials entry — which exact image bit-for-bit ran for this task.
- `ContainerOOM` + `memory_used_bytes` informs capacity planning and budget estimation.
- `ContainerNetworkBlocked` + `destination_ip` reveals exfiltration attempts (agents should have `--network=none`).
- `ContainerKilled` traces admin interventions for the audit trail.
- `ContainerImageRejected` catches supply-chain attacks (tampered image).

#### File & Network Events (eBPF)

Collected by the procfs/eBPF monitor (`GYRE_PROCFS_MONITOR`):

| Event Type | Trigger | `detail` fields |
|---|---|---|
| `FileAccess` | Agent reads/writes a file path | `pid`, `path`, `operation` (read/write/create/delete), `bytes` |
| `NetworkConnect` | Agent opens a TCP connection | `pid`, `destination_ip`, `destination_port`, `protocol` |
| `ProcessSpawned` | Agent spawns a child process | `pid`, `child_pid`, `command`, `args` |
| `ProcessExited` | Child process exits | `pid`, `child_pid`, `exit_code` |
| `SyscallBlocked` | Syscall blocked (seccomp/ABAC) | `pid`, `syscall_name`, `reason` |

#### Source Control Events

| Event Type | Trigger | `detail` fields |
|---|---|---|
| `GitPushAccepted` | Smart HTTP push accepted | `repo_id`, `branch`, `commit_sha`, `agent_id`, `push_gate_results` |
| `GitPushRejected` | Smart HTTP push rejected by gate | `repo_id`, `branch`, `gate_name`, `reason` |
| `GitClone` | Agent clones repo | `repo_id`, `agent_id` |
| `SpecChanged` | Spec file modified in default branch push | `repo_id`, `spec_path`, `change_kind`, `task_id` |

#### Access Control Events

| Event Type | Trigger | `detail` fields |
|---|---|---|
| `AuthSuccess` | Valid token accepted | `token_kind` (`global`, `agent_jwt`, `uuid_token`, `api_key`), `agent_id` |
| `AuthFailure` | Invalid or revoked token | `token_kind`, `rejection_reason` |
| `AbacDenied` | ABAC policy rejects request | `agent_id`, `policy_id`, `resource`, `action`, `claims` |
| `RbacDenied` | Role check rejects request | `user_id`, `required_role`, `actual_role`, `endpoint` |
| `ImpersonationStarted` | Admin begins impersonating user | `impersonator_id`, `target_user_id`, `approval_token` |
| `ImpersonationEnded` | Impersonation session ends | `impersonator_id`, `target_user_id`, `duration_secs`, `actions_taken` |

---

## Audit API

```
POST /api/v1/audit/events          → record eBPF audit event (agent-side push)
GET  /api/v1/audit/events          → query events (?agent_id=&event_type=&since=)
GET  /api/v1/audit/stream          → SSE stream of live audit events
GET  /api/v1/audit/stats           → event counts by type (last 24 h)
```

### SSE Stream Format

```
data: {"id":"<uuid>","event_type":"ContainerStarted","agent_id":"<uuid>","detail":{"container_id":"abc123","image":"ghcr.io/org/agent:v1","image_hash":"sha256:...","runtime":"docker","security_opts":{"network":"none","memory":"2g","pids_limit":512,"user":"65534:65534"}},"timestamp":1711036800}
```

---

## SIEM Integration

Audit events are forwarded in real time to configured SIEM targets:

```
POST   /api/v1/admin/siem          → add forwarding target
GET    /api/v1/admin/siem          → list targets
PUT    /api/v1/admin/siem/{id}     → update target (URL, format, filter, enabled)
DELETE /api/v1/admin/siem/{id}     → remove target
```

Supported formats: `json`, `cef` (Common Event Format), `leef` (Log Event Extended Format).

Events are filtered by minimum severity before forwarding. Container lifecycle events (`ContainerStarted`, `ContainerStopped`, `ContainerKilled`, `ContainerFailed`, `ContainerOOM`, `ContainerNetworkBlocked`) are forwarded at `High` severity by default — they're the events SOC teams care most about.

---

## Log Retention & Compression

See `business-continuity.md` for retention policy details. Summary:

- Audit events: 365 days local, indefinite in SIEM
- Agent logs: 30 days (compressed after 7 days via the `log-compression` background job)
- eBPF event streams: 90 days (high-volume; compressed aggressively)
