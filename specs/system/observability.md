# Observability & Governance

## Tracing & Observability

- **OpenTelemetry (OTel)** tracing throughout - traces, metrics, logs.
- **Domain-oriented observability** - instrumentation follows domain boundaries, not just infrastructure. Traces should tell you what the system *did*, not just what the code *ran*.

## Audit System

- **Total auditing** - everything that happens is captured. No exceptions.
- Every agent runtime includes an **eBPF program** capturing all system-level activity (syscalls, network, file access, process execution).
- All audit data streams back to the **central server** in real time.
- Server supports **forwarding to SIEM server(s)** (Splunk, Elastic, Sentinel, etc.).

## Agent Auditability

- Every single action an agent takes has an audit trail, **traceable from start to finish**.
- The **entire model context window** is captured and stored for auditability - full replay of what the agent saw and decided.
