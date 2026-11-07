---
title: "Expand AuditEvent schema to spec-compliant envelope"
spec_ref: "observability.md §Audit Event Schema"
depends_on: []
progress: complete
coverage_sections:
  - "observability.md §Audit Event Schema"
commits: ["4fe453dfb18c84a8a00ae02faae99043785bd699", "99de714b08938493b1a60d1e2217baabbf665275", "9e70fa7f78e58979f79aef505e9120c10460e280"]
review: specs/reviews/task-102.md
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

- [x] `AuditEvent` struct matches spec envelope (all 14 fields)
- [x] `AuditOutcome` enum with Success/Failure/Blocked variants
- [x] `agent_id` is `Option<Id>` (null for server-initiated events)
- [x] Database migration adds new columns, removes path/pid
- [x] Query API supports filtering by workspace_id, user_id, resource_type, outcome
- [x] SIEM formatters include new fields
- [x] All existing audit tests updated and passing
- [x] `cargo test --all` passes

## Agent Instructions

Read `specs/system/observability.md` §Audit Event Schema for the full spec. The current domain model is in `gyre-domain/src/audit.rs`. The port is in `gyre-ports/src/audit.rs`. The SQLite adapter is in `gyre-adapters/src/sqlite/audit.rs`. The API handlers are in `gyre-server/src/api/audit.rs`. The SIEM formatter is in `gyre-server/src/siem.rs`. Grep for `AuditEvent::new(` to find all call sites that need updating. Check migration numbering in `gyre-adapters/src/sqlite/migrations.rs` — currently at 000038.

## Shipped

The audit event envelope is implemented end-to-end and verified on the current branch head after the base rebase.

### Envelope

- `gyre-domain/src/audit.rs`: `AuditEvent` carries all 14 spec fields; `agent_id` is `Option<Id>` (null for server-initiated events); `AuditOutcome` enum (Success/Failure/Blocked) with snake_case serde both ways. Legacy `path`/`pid` are subsumed by the `detail` JSON payload.
- `gyre-ports/src/audit.rs`: `AuditRepository::query` takes an `AuditQueryFilter` with 9 dimensions (agent_id, event_type, workspace_id, user_id, resource_type, outcome, since, until, limit).
- SQLite, Postgres, and mem adapters all implement the 14-column row shape and every filter dimension with identical semantics.
- Migration `2026-09-30-000054_audit_event_envelope` (rebuild-table pattern): adds the 9 new columns, makes `agent_id` nullable, folds legacy `path`/`pid` into `detail` with portable SQL (RFC 7396 merge-patch fold via CASE WHEN, no dialect-only functions), backfills `resource_type='agent'`/`outcome='success'`. Down migration reverses the shape and restores legacy columns from detail.
- `gyre-server/src/api/audit.rs`: POST `/api/v1/audit/events` binds `agent_id`/`user_id` to the verified caller (NEW-31) and validates caller-supplied `workspace_id`/`repo_id` against the caller's tenant (Forbidden/NotFound on mismatch); GET supports all new filter dimensions; SSE/broadcasts emit the full envelope.
- `gyre-server/src/trusted_proxy.rs`: source_ip comes from the socket peer address; `X-Forwarded-For`/`X-Real-Ip` honored only when the peer is in `GYRE_TRUSTED_PROXIES` (empty by default), parsed rightmost-untrusted, failing closed to `None` without socket info.
- `gyre-server/src/siem.rs`: syslog JSON and CEF extensions include workspaceId, resourceType, outcome, agentId/userId, src, and detail-derived filePath/pid.
- `container_audit.rs`, `procfs_monitor.rs`, `audit_simulator.rs`, `retention.rs` use the new constructor; container events set `resource_type: "container"` with per-class outcomes (started/stopped=Success, crashed/oom=Failure, network_blocked=Blocked).

### Base-merge repair (commit 7f1e7362)

Checkpoint merge `bca461d8` had committed unresolved conflict markers in `web/dist/index.html` and both `elk.bundled-*.js` bundles (rename/rename + rename/delete conflicts between task-side and base-side dist artifacts). Web sources are byte-identical between base `1ae18d1c` and HEAD, so `web/dist` was restored to the base build wholesale — stale task-side artifacts (`elk.bundled-BEFJVH3L.js`, `index-9ZNGTuNm.js`) deleted, `index.html`/`elk.bundled-BeoNa-iK.js` taken from base. `git diff 1ae18d1c -- web/dist` is empty, both bundles parse under `node --check`, zero conflict markers in `web/dist`.

### Test evidence

Focused suites run on the branch head (logs in `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-server --lib api::audit` → 12 passed, 0 failed (envelope round-trip, agent-id binding, scope validation, trusted-proxy source_ip both directions, ApiError shape on bad outcome filter, legacy path/pid fold, outcome filter)
- `cargo test -p gyre-server --lib trusted_proxy` → 9 passed, 0 failed
- `cargo test -p gyre-domain --lib audit` → 8 passed, 0 failed
- `cargo test -p gyre-adapters --lib migration_000054` → 1 passed, 0 failed (legacy path/pid fold regression)
- Mechanical checks green: `check-arch`, `check-migration-sql-portability`, `check-migration-versions`, `check-forwarded-header-trust`, `check-forged-scope-fields`, `check-mem-port-contracts`
- `node --check` on both `web/dist` bundles → parse OK; `grep -c '<<<<<<<\|>>>>>>>'` across `web/dist` → 0

Implementation commits: `9e70fa7f` (F2/F3/F4 handler + trusted-proxy layer), `99de714b` (portable migration rewrite + fold regression test), `7f1e7362` (web/dist base-merge conflict repair).
