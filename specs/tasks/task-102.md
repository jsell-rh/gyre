---
title: "Expand AuditEvent schema to spec-compliant envelope"
spec_ref: "observability.md §Audit Event Schema"
depends_on: []
progress: not-started
coverage_sections:
  - "observability.md §Audit Event Schema"
commits: []
---

## Spec Excerpt

The spec requires a rich audit event envelope for compliance auditing:

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

The current implementation (`gyre-domain/src/audit.rs`) has a simpler struct: `id, agent_id (required), event_type, path, details, pid, timestamp`. Missing: `user_id`, `session_id`, `workspace_id`, `repo_id`, `resource_type`, `resource_id`, `outcome`, `source_ip`, `user_agent`. Also `agent_id` must become `Option<Id>`.

## Implementation Plan

1. **Domain layer (`gyre-domain/src/audit.rs`):**
   - Add `AuditOutcome` enum (Success, Failure, Blocked) with serde serialization
   - Expand `AuditEvent` struct to add all missing fields per spec
   - Make `agent_id` optional (`Option<Id>`)
   - Remove `path` field (subsumed by `detail` JSON payload)
   - Remove `pid` field (subsumed by `detail` JSON payload)
   - Update `AuditEvent::new()` constructor
   - Update tests

2. **Port layer (`gyre-ports/src/audit.rs`):**
   - Update `AuditRepository` trait methods to accept new query filters (workspace_id, user_id, resource_type, outcome)
   - Add query method for new filter dimensions

3. **Adapter layer (`gyre-adapters/src/sqlite/audit.rs`):**
   - Create migration adding columns: user_id, session_id, workspace_id, repo_id, resource_type, resource_id, outcome, source_ip, user_agent
   - Make agent_id nullable
   - Remove path and pid columns (data moves to detail JSON)
   - Update INSERT/SELECT queries

4. **Server layer (`gyre-server/src/api/audit.rs`):**
   - Update `RecordAuditEventRequest` to accept new fields
   - Update `AuditEventResponse` to include new fields
   - Update `QueryAuditParams` with new filter options
   - Update `record_audit_event` handler to extract source_ip and user_agent from request headers

5. **SIEM formatting (`gyre-server/src/siem.rs`):**
   - Update `format_syslog` and `format_cef` to include new fields
   - CEF extensions should include workspace_id, resource_type, outcome

6. **Ripple updates:**
   - Update `container_audit.rs` to use new schema
   - Update `audit_simulator.rs` to use new schema
   - Update `procfs_monitor.rs` to use new schema
   - Update all callers of `AuditEvent::new()`

## Acceptance Criteria

- [ ] `AuditEvent` struct matches spec envelope (all 14 fields)
- [ ] `AuditOutcome` enum with Success/Failure/Blocked variants
- [ ] `agent_id` is `Option<Id>` (null for server-initiated events)
- [ ] Database migration adds new columns, removes path/pid
- [ ] Query API supports filtering by workspace_id, user_id, resource_type, outcome
- [ ] SIEM formatters include new fields
- [ ] All existing audit tests updated and passing
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/observability.md` §Audit Event Schema for the full spec. The current domain model is in `gyre-domain/src/audit.rs`. The port is in `gyre-ports/src/audit.rs`. The SQLite adapter is in `gyre-adapters/src/sqlite/audit.rs`. The API handlers are in `gyre-server/src/api/audit.rs`. The SIEM formatter is in `gyre-server/src/siem.rs`. Grep for `AuditEvent::new(` to find all call sites that need updating. Check migration numbering in `gyre-adapters/src/sqlite/migrations.rs` — currently at 000038.
