# Coverage: Observability & Governance

**Spec:** [`system/observability.md`](../../system/observability.md)
**Last audited:** 2026-04-13 (full audit — reclassification from not-started)
**Coverage:** 5/9

| # | Section | Depth | Status | Task | Notes |
|---|---------|-------|--------|------|-------|
| 1 | Tracing & Observability | 2 | implemented | - | OTel tracing (telemetry.rs), OTLP gRPC export via OTEL_EXPORTER_OTLP_ENDPOINT, Prometheus /metrics: gyre_http_requests_total, gyre_http_request_duration_seconds, gyre_active_agents, gyre_merge_queue_depth. Structured JSON logging via GYRE_LOG_FORMAT. |
| 2 | Audit System | 2 | implemented | - | Procfs-based monitoring (procfs_monitor.rs, 5s polling), central audit storage (SQLite adapter), SIEM forwarding (siem.rs). eBPF-level syscall interception is a stretch goal — current procfs monitoring covers file descriptors and TCP connections. |
| 3 | Agent Auditability | 2 | implemented | - | model_context field captured in AgentCommit (agent_tracking.rs). AIBOM attestation heuristics in aibom.rs. Full replay audit via stored context. |
| 4 | Audit Event Schema | 2 | task-assigned | task-102 | Current AuditEvent struct is agent-centric with minimal fields (id, agent_id, event_type, path, details, pid, timestamp). Spec requires rich envelope: user_id, session_id, workspace_id, repo_id, resource_type, resource_id, outcome (Success/Failure/Blocked), source_ip, user_agent. |
| 5 | Audit Event Types | 3 | task-assigned | task-103, task-104 | 9/34 spec event types implemented (FileAccess, NetworkConnect, ProcessExec, Syscall, Container{Started,Stopped,Crashed,Oom,NetworkBlocked}). Missing: 7 agent lifecycle events, 6 additional container events, 2 file/network events, 4 source control events, 6 access control events. |
| 6 | Audit API | 2 | implemented | - | POST/GET /api/v1/audit/events, GET /api/v1/audit/stream, GET /api/v1/audit/stats. All registered in api/mod.rs:447-451. Auth-bound agent_id (NEW-31 fix). |
| 7 | SSE Stream Format | 3 | implemented | - | SSE stream via broadcast channel (audit.rs:127-153). 30s heartbeat timeout, 15s keep-alive ping. JSON payload with id, event_type, agent_id, details, timestamp. |
| 8 | SIEM Integration | 2 | implemented | - | Full CRUD: POST/GET/PUT/DELETE /api/v1/admin/siem. Syslog (RFC 5424) + webhook targets. JSON + CEF output formats. Background forwarder every 10s (siem.rs:286-330). |
| 9 | Log Retention & Compression | 2 | task-assigned | task-105 | Retention policy store (retention.rs) with defaults: activity 90d, analytics 365d, cost 365d. Ring-buffer auto-eviction via TelemetryBuffer. No log-compression background job. No gzip/bzip2 compression after 7 days per spec. |
