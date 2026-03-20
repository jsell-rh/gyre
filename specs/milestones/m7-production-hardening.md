# Milestone 7: Production Hardening

Deploy-ready infrastructure. After M7, Gyre has eBPF audit, SIEM forwarding, NixOS packaging, and the foundation for remote compute and WireGuard networking.

## Deliverables

### 1. eBPF Audit Framework

Capture agent system-level activity:

- **eBPF program stub** — define the audit event types (file access, network, process exec, syscalls)
- **Audit event domain** — AuditEvent entity with structured fields
- **Audit stream** — WebSocket endpoint for live audit streaming
- **Audit query API** — `GET /api/v1/audit/events` with filters (agent, event type, time range)
- **SQLite storage** — audit_events table with retention policy
- **Dashboard** — audit log viewer with filters and live stream

Note: Full eBPF implementation requires privileged access. For M7, implement the domain model, API, storage, and a simulated audit event generator for testing.

### 2. SIEM Forwarding

Forward audit data to external SIEM servers:

- **Syslog forwarder** — forward events via RFC 5424 syslog (TCP/UDP)
- **Webhook forwarder** — POST events to configurable webhook URLs
- **Forwarder config** — `POST /api/v1/admin/siem` to configure forwarding targets
- **Background job** — batch-forward events on interval
- **Format adapters** — CEF (Common Event Format) and JSON output formats

### 3. NixOS Packaging

Validate and complete the NixOS flake:

- **Validate `nix build`** — ensure server + CLI binaries build from flake
- **Docker image** — `nix build .#dockerImage` produces a minimal Docker image
- **NixOS module** — `services.gyre` NixOS module for system-level deployment
- **Dev shell** — `nix develop` provides complete dev environment
- **CI integration** — add Nix build step to GitHub Actions (optional, cache with cachix)

### 4. Remote Compute Foundation

Pluggable compute targets for agent provisioning:

- **ComputeTarget trait** — abstraction for where agents run
- **Local target** — spawn agent as local process (default, already working)
- **SSH target** — spawn agent on remote machine via SSH
- **Docker target** — spawn agent in a Docker container
- **Compute config** — `POST /api/v1/admin/compute-targets` CRUD
- **Agent spawn enhancement** — spawn endpoint accepts compute_target parameter

### 5. WireGuard Networking Stub

Foundation for agent networking mesh:

- **Network domain** — NetworkPeer entity (agent_id, wireguard_pubkey, endpoint, allowed_ips)
- **Peer registration** — agents register WireGuard keys on spawn
- **Peer discovery** — `GET /api/v1/network/peers` returns mesh topology
- **DERP map** — server maintains relay map for NAT traversal (stub)

Note: Full WireGuard/Tailscale integration requires network configuration. M7 implements the domain model and API. Actual WireGuard key exchange is a follow-up.

### 6. Production Hardening

- **Graceful shutdown** — drain connections, flush traces, complete in-flight work
- **Connection pooling** — r2d2 or deadpool for SQLite
- **Rate limiting** — tower-based rate limiter on API endpoints
- **Request size limits** — prevent oversized payloads
- **CORS configuration** — configurable allowed origins
- **Error recovery** — panic handler, error boundaries

## Success Criteria

- Audit events captured, stored, queryable, forwardable
- NixOS flake builds server, CLI, Docker image
- Remote compute targets configurable (at least local + Docker)
- 450+ tests

## Dependencies

- M6 Infrastructure (complete)
