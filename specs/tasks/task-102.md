---
title: "Expand AuditEvent schema to spec-compliant envelope"
spec_ref: "observability.md §Audit Event Schema"
depends_on: []
progress: ready-for-review
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

The audit event envelope is implemented end-to-end (domain → port → all three adapters → migration → API → SIEM → web) and every round-1 review finding (F1–F4) is fixed with regression tests.

### Envelope

- `gyre-domain/src/audit.rs`: `AuditEvent` carries all 14 spec fields; `agent_id` is `Option<Id>` (null for server-initiated events); `AuditOutcome` enum (Success/Failure/Blocked) with snake_case serde both ways. Legacy `path`/`pid` are subsumed by the `detail` JSON payload.
- `gyre-ports/src/audit.rs`: `AuditRepository::query` takes an `AuditQueryFilter` with 9 dimensions (agent_id, event_type, workspace_id, user_id, resource_type, outcome, since, until, limit).
- SQLite, Postgres, and mem adapters all implement the 14-column row shape and every filter dimension with identical semantics.
- Migration `2026-09-30-000054_audit_event_envelope` (rebuild-table pattern): adds the 9 new columns, makes `agent_id` nullable, folds legacy `path`/`pid` into `detail`, backfills `resource_type='agent'`/`outcome='success'`. Down migration reverses the shape and restores legacy columns from detail.
- `gyre-server/src/api/audit.rs`: POST `/api/v1/audit/events` binds `agent_id`/`user_id` to the verified caller (NEW-31), GET supports all new filter dimensions; SSE/broadcasts emit the full envelope.
- `gyre-server/src/siem.rs`: syslog JSON and CEF extensions include workspaceId, resourceType, outcome, agentId/userId, src, and detail-derived filePath/pid.
- `container_audit.rs`, `procfs_monitor.rs`, `audit_simulator.rs`, `retention.rs` updated to the new constructor; container events set `resource_type: "container"` with per-class outcomes (started/stopped=Success, crashed/oom=Failure, network_blocked=Blocked).

### Review-revision fixes (F1–F4)

- **F1 (migration portability):** migration 000054's up/down rewritten from SQLite-only JSON1 functions to portable SQL (CASE WHEN + string concatenation) with RFC 7396 merge-patch fold semantics; the four temporary portability exemptions were deleted and the frozen count lowered 5→1. Regression test `migration_000054_folds_legacy_path_pid_into_detail` drives the real embedded up.sql against seeded legacy rows (JSON-hostile paths, non-object details, pre-existing keys) and asserts exact merged payloads.
- **F2 (forged scope attribution):** `validate_workspace_scope`/`validate_repo_scope` look up every caller-supplied `workspace_id`/`repo_id` and reject (Forbidden/NotFound) anything outside the caller's tenant; tests cover cross-tenant workspace, cross-tenant repo, and mismatched workspace+repo pairs.
- **F3 (forgeable source_ip):** new `gyre-server/src/trusted_proxy.rs` — source of truth is the socket peer address (`ConnectInfo`); `X-Forwarded-For`/`X-Real-Ip` are honored only when the peer is in `GYRE_TRUSTED_PROXIES` (empty by default), parsed rightmost-untrusted; resolution fails closed to `None` without socket info. `check-forwarded-header-trust.sh` passes.
- **F4 (outcome filter 400 shape):** `?outcome=` parses as a plain string and a typo returns the repo's `ApiError` JSON shape naming the valid values, consistent with the other handlers.

### Test evidence

Focused suites run on this branch head (logs in `/tmp/stage/review-evidence/`):

- `cargo test -p gyre-domain --lib audit` → 8 passed, 0 failed
- `cargo test -p gyre-adapters --lib sqlite::` → 263 passed, 0 failed (includes the F1 migration-fold test and 11 audit adapter tests)
- `cargo test -p gyre-server --lib api::audit` → 12 passed, 0 failed (envelope round-trip, agent-id binding, F2 scope validation, F3 trusted-proxy source_ip both directions, F4 ApiError shape, legacy path/pid fold, outcome filter)
- `cargo test -p gyre-server --lib trusted_proxy` → 9 passed, 0 failed
- `cargo test -p gyre-server --lib siem::` → 7 passed, 0 failed
- Mechanical checks all green: `check-migration-sql-portability`, `check-migration-versions`, `check-arch`, `check-forwarded-header-trust`, `check-forged-scope-fields`, `check-mem-port-contracts`, `check-abac-route-registry`, `check-task-commit-attribution`, `check-inert-enforcement`, `check-scope-literal-defaults`.

Implementation commits: `9e70fa7f` (F2/F3/F4 handler + trusted-proxy layer), `99de714b` (F1 portable migration rewrite + portability-exemption shrink + fold regression test).

### Round-3 repair (review 1e921d5f)

- **Committed merge-conflict artifact (R1):** merge d93eaf20 had committed `web/dist/assets/elk.bundled-C7RdDx88.js` with unresolved conflict markers and both sides' content (HEAD side importing `index-DauSjDxg.js`, absent from the tree). Fixed by rebuilding `web/dist` from the committed web sources (`npm ci --no-audit && npm run build`; sources identical to base `2f215b1e` outside `web/dist`). The committed set is now internally consistent — `index.html` → `index-9ZNGTuNm.js` + `index-GeLbLKhg.css`, `index-9ZNGTuNm.js` lazy-imports `./elk.bundled-BEFJVH3L.js`, which imports back `./index-9ZNGTuNm.js` — with zero conflict markers and both bundles parsing cleanly under `node --check` (the exact SyntaxError-at-parse-time failure mode the review flagged for the RustEmbed/SKIP_WEB_BUILD=1 shipping path). Commit `7cc1a7a5`.
- **Stale exemption debt (R2):** the F2/F3 product fixes had landed but `scripts/forwarded-header-trust-exemptions.txt` and `scripts/forged-scope-fields-exemptions.txt` still carried their 2 grandfather entries each. Deleted the stale lines and lowered `FROZEN_EXEMPTION_COUNT` 2 → 0 in both `check-forwarded-header-trust.sh` and `check-forged-scope-fields.sh`, per the exemption files' own shrink-only protocol. Both checks pass against the tree with the now-empty exemption files. Commit `67bbe683`.

### Round-3 evidence

- `bash scripts/check-forwarded-header-trust.sh` → OK (0 exemptions)
- `bash scripts/check-forged-scope-fields.sh` → OK (0 exemptions)
- `bash scripts/check-arch.sh`, `check-migration-sql-portability`, `check-migration-versions`, `check-mem-port-contracts`, `check-abac-route-registry`, `check-inert-enforcement`, `check-scope-literal-defaults` → all OK
- `node --check` on both rebuilt bundles → parse OK; `grep -c '<<<<<<<\|>>>>>>>'` across `web/dist` → 0
- Note: `npm ci` requires `--no-audit` in this sandbox (the registry's bulk security-advisories endpoint returns 403 policy_denied, which aborts the install); package installation itself is unaffected.
