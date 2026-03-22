# M23: Platform Operations

**Status:** Done

## Summary

M23 adds platform-level operational capabilities: container lifecycle observability, analytics decision support, agent resilience, and enterprise provisioning.

## Deliverables

### M23.1 — Container Lifecycle Audit Events

Five new `AuditEventType` variants emitted automatically by the container runtime:

| Event | Meaning |
|---|---|
| `container_started` | Container successfully started for an agent |
| `container_stopped` | Container exited cleanly |
| `container_crashed` | Container exited with non-zero code or was force-killed |
| `container_oom` | Container OOM-killed by the kernel |
| `container_network_blocked` | Outbound network attempt blocked by `--network=none` (G8) |

Events flow through the existing audit infrastructure: `GET /api/v1/audit/events`, `GET /api/v1/audit/stream` (SSE), and SIEM forwarding.

### M23.2 — Analytics Decision API

Three new analytics endpoints for decision-support queries:

| Endpoint | Description |
|---|---|
| `GET /api/v1/analytics/usage` | Event count, unique agent count, trend (up/down/flat) vs prior period |
| `GET /api/v1/analytics/compare` | Before/after pivot comparison: count, change_pct, improved flag |
| `GET /api/v1/analytics/top` | Top N event names by count |

MCP tool `gyre_analytics_query` wraps all three endpoints (query_type: `usage`|`compare`|`top`).

### M23.3 — BCP Graceful Degradation

Two new constructs for agent resilience when connectivity is lost:

- **`DisconnectedBehavior`** enum on spawn: `Pause` (default), `ContinueOffline`, `Abort` — controls stale-agent-detector behavior
- **`AgentStatus::Paused`** — new status for agents that lost connectivity and were gracefully paused rather than killed

New admin endpoints:
- `GET /api/v1/admin/bcp/targets` — returns `{rto_seconds, rpo_seconds}` from `GYRE_RTO`/`GYRE_RPO` env vars
- `POST /api/v1/admin/bcp/drill` — triggers live snapshot+verify cycle; returns `{snapshot_id, verified, duration_ms}`

Admin Panel **BCP tab** added to the Health/Jobs/Audit/... tab set.

### M23.4 — SCIM 2.0 Provisioning

Eleven SCIM 2.0 endpoints under `/scim/v2/` for enterprise IdP integration:

| Endpoint | Description |
|---|---|
| `GET /scim/v2/ServiceProviderConfig` | SCIM discovery (no auth required) |
| `GET /scim/v2/Schemas` | Schema definitions |
| `GET /scim/v2/ResourceTypes` | Resource type registry |
| `GET /scim/v2/Users` | List users (`?startIndex=&count=&filter=`) |
| `POST /scim/v2/Users` | Provision a new user |
| `GET /scim/v2/Users/{id}` | Get user |
| `PUT /scim/v2/Users/{id}` | Replace user attributes |
| `DELETE /scim/v2/Users/{id}` | Deprovision user |

Auth via `GYRE_SCIM_TOKEN` Bearer (separate from `GYRE_AUTH_TOKEN`). When unset, SCIM endpoints return 401.

## Acceptance Criteria

- [x] Container lifecycle events appear in audit stream when Docker/Podman agent exits cleanly, crashes, or is OOM-killed
- [x] `GET /api/v1/analytics/usage` returns trend field (up/down/flat)
- [x] `gyre_analytics_query` MCP tool callable with query_type=usage/compare/top
- [x] `POST /api/v1/agents/spawn` accepts `disconnected_behavior` field; defaults to Pause
- [x] `GET /api/v1/admin/bcp/targets` returns rto_seconds/rpo_seconds from env vars
- [x] `POST /api/v1/admin/bcp/drill` returns snapshot_id, verified, duration_ms
- [x] `POST /scim/v2/Users` provisions a user accessible via `GET /api/v1/users/me`
- [x] `DELETE /scim/v2/Users/{id}` deprovisions user; subsequent API calls return 401
