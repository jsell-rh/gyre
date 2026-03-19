# Milestone 4: Identity & Observability

Secure Gyre with proper identity management and make it observable. After M4, Gyre has SSO authentication, role-based agent permissions, full OpenTelemetry tracing, and an admin panel for operational management.

## Deliverables

### 1. Keycloak SSO Integration

Replace static token auth with proper SSO:

- **Keycloak as identity provider** — OIDC integration
- **User login flow** — dashboard redirects to Keycloak for login, receives JWT
- **JWT validation** — server validates JWTs on every API request
- **User model** — User entity in domain (id, name, email, roles)
- **Agent identity** — agents get service account tokens from Keycloak
- **API key alternative** — for CI/scripts, support API keys mapped to users

### 2. Role-Based Access Control (RBAC)

Simple role-based permissions:

- **Roles**: Admin, Developer, Agent, ReadOnly
- **Admin** — full access to all endpoints + admin panel
- **Developer** — CRUD on projects/repos/tasks/MRs, can spawn agents
- **Agent** — can push/pull, update own status/heartbeat, complete tasks
- **ReadOnly** — GET endpoints only
- Enforce via middleware, checked after auth

### 3. OpenTelemetry Tracing

Full OTel instrumentation:

- **OTLP exporter** — send traces to any OTel collector (Jaeger, Tempo, etc.)
- **Request tracing** — every HTTP request gets a trace with span
- **Domain operation spans** — task transitions, MR reviews, merge queue processing
- **Agent session spans** — trace from spawn to completion
- **Trace context propagation** — pass trace IDs through WebSocket messages
- **Metrics** — request count, latency histograms, active agents gauge, queue depth

### 4. Admin Panel

Server-side admin capabilities:

- **Background job viewer** — see running jobs (merge processor, stale agent checker), status, last run
- **Audit log** — searchable activity event history with filters
- **System health** — database stats, connection pool, uptime
- **Agent management** — force-kill agents, reassign tasks, clear worktrees
- **Dashboard page** — admin-only section in Svelte SPA

### 5. Structured Logging Enhancement

Upgrade from basic tracing to structured, queryable logs:

- **JSON log output** — structured logs for machine parsing
- **Request ID** — every request gets a unique ID, propagated through spans
- **Agent context** — logs include agent_id when available
- **Log levels** — configurable per-module via RUST_LOG

## Success Criteria

- Users authenticate via Keycloak SSO
- Agents authenticate via service account tokens
- RBAC enforced on all endpoints
- Traces visible in OTel collector (Jaeger)
- Admin panel shows system health and job status
- 300+ tests

## Non-Goals for M4

- No eBPF audit (M5+)
- No SIEM forwarding (M5+)
- No SPIFFE workload attestation (M5+)
- No SCIM user provisioning (M5+)

## Dependencies

- M3 Agent Orchestration (complete)
