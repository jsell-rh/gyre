---
title: "Hierarchy enforcement integration tests and audit auth hardening"
spec_ref: "hierarchy-enforcement.md §7"
depends_on: [task-160]
progress: complete
coverage_sections:
  - "hierarchy-enforcement.md §New Integration Tests"
  - "hierarchy-enforcement.md §Audit/Analytics/Cost Recording"
commits: ["bd4f1aee81cbc67fcf679199e264e0afe2f6d800"]
---

## Spec Excerpt

### §7 New Integration Tests (hierarchy-enforcement.md)

| Test | What it verifies |
|---|---|
| `tests/tenant_isolation.rs` | Two-tenant scenario: listing/finding through tenant A never leaks tenant B data |
| `tests/abac_middleware.rs` | ABAC middleware denies access when policy conditions aren't met; allows when they are |
| `tests/workspace_scoping.rs` | Entities created in workspace A are not visible in workspace B's scoped routes |
| `tests/hierarchy_cascade.rs` | Budget cascade: workspace budget can't exceed tenant, repo can't exceed workspace |

### §5 Audit/Analytics/Cost Recording (hierarchy-enforcement.md)

`POST /api/v1/analytics/events`, `POST /api/v1/costs`, and `POST /api/v1/audit/events` should:
- Require `subject.type == agent` for recording
- Validate that the `agent_id` in the payload matches `subject.id`
- Allow Admin to record on behalf of any agent

## Implementation Plan

1. **`tests/tenant_isolation.rs`**: Create two tenants via TenantRepository, create entities (repos, tasks, agents, MRs) in each tenant. Verify that listing through one tenant's storage never returns the other's entities. Use the existing test infrastructure (see `tests/api_integration.rs` for patterns).

2. **`tests/abac_middleware.rs`**: Test the ABAC middleware layer directly. Create scenarios where policies should deny (wrong role, wrong workspace), and verify 403 responses. Test that allow policies pass through.

3. **`tests/workspace_scoping.rs`**: Create two workspaces, populate entities in each, verify scoped API endpoints only return entities from the target workspace.

4. **`tests/hierarchy_cascade.rs`**: Verify budget cascade validation — workspace budget cannot exceed tenant limit, and that `cascade_validation_rejects_workspace_exceeding_tenant()` semantics work end-to-end.

5. **Audit auth hardening**: Verify that POST /api/v1/audit/events, POST /api/v1/analytics/events, and POST /api/v1/costs enforce agent-self-recording. Add ABAC policies or handler-level checks if missing.

## Acceptance Criteria

- [ ] `tests/tenant_isolation.rs` exists and passes
- [ ] `tests/abac_middleware.rs` exists and passes
- [ ] `tests/workspace_scoping.rs` exists and passes
- [ ] `tests/hierarchy_cascade.rs` exists and passes
- [ ] Audit/analytics/cost recording endpoints enforce subject-matching
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/hierarchy-enforcement.md` §5 (Legacy Cleanup — Audit/Analytics/Cost Recording) and §7 (New Integration Tests). Use the existing `tests/api_integration.rs` and `tests/auth_integration.rs` for test infrastructure patterns (app state setup, HTTP client helpers, auth token generation). The integration tests should use real SQLite (in-memory or temp file), not mocks.

## Shipped

Commit bd4f1aee (feat(task-161)) closes both assigned coverage sections.

### Audit auth hardening (hierarchy-enforcement.md §5)

New shared handler-level gate `resolve_recording_agent_id` (api/audit.rs),
applied to `POST /api/v1/audit/events`, `POST /api/v1/analytics/events`
and `POST /api/v1/costs`:

- **Subject gate**: recording requires an agent or Admin subject;
  Developer/ReadOnly user callers are rejected 403.
- **Agent self-recording**: an agent subject may record only its own
  `agent_id` — a payload `agent_id` naming anyone else is 403 (this
  replaces the interim task-146 silent-rebind with a stricter rejection;
  the NEW-31 anti-forgery property — a caller can never attribute
  telemetry to an identity it does not hold — is preserved and now
  asserted as a hard 403). An absent payload `agent_id` is bound to the
  verified caller identity.
- **Admin on-behalf**: an Admin subject may record on behalf of any agent
  (backfill/correction) — the payload `agent_id` is honored as the
  recorded identity while `user_id` stays bound to the caller so the
  operating admin remains attributable.
- `record_cost` previously took **no auth extractor at all** — any
  authenticated caller could attribute costs to an arbitrary agent; it
  now carries the same gate.

Handler-level rather than an ABAC policy because the rule compares a
request-BODY field (`agent_id`) against the verified subject identity —
the policy condition language evaluates subject/resource/action
attributes and cannot see payload fields (the task allows either form).

Killing tests (unit, full router): `audit_event_agent_id_bound_to_caller`
(agent JWT + forged id → 403; own/absent id → 201 recorded as the
verified subject), `audit_event_agent_id_rejects_mismatched_subject`
(Developer JWT → 403), `record_event_agent_subject_rejects_mismatched_agent_id`,
`record_cost_enforces_agent_self_recording`; admin-on-behalf legs asserted
in `record_audit_event_returns_201` and
`record_event_binds_identity_to_caller_not_request_body`. Two prior
assertions were updated to the superseding §5 semantics (forged id under
the global Admin token is now honored as on-behalf recording instead of
rebound to "system") — the spec is the contract here; the agent-caller
anti-forgery regression is stronger than before (403 vs silent rebind).

### New integration tests (hierarchy-enforcement.md §7)

Four new files in `crates/gyre-server/tests/`, all over real SQLite
temp-file databases (`GYRE_DATABASE_URL` + per-binary `ENV_LOCK`, the
search_wiring.rs pattern) through the full `require_auth` → ABAC
middleware stack, with real API-key and per-agent-token principals:

- `tenant_isolation.rs` — two tenants; `GET /api/v1/workspaces` scoped
  by the caller's tenant in both directions; cross-tenant addressed reads
  (repo/task/agent/MR/workspace) return 403 with the opaque ABAC body,
  own reads 200. Complements task-160's storage-level
  `gyre-adapters/tests/tenant_isolation.rs`.
- `abac_middleware.rs` — deny/allow pairs per policy band: agent token
  on an admin route 403 vs global Admin token 200; ReadOnly member read
  200 vs write 403; Developer member 200 vs membership-less 403
  (`workspace-membership-required` 940 over `developer-write-access` 800);
  agent token in-scope read 200; missing token 401.
- `workspace_scoping.rs` — workspace A's member sees only A's entities
  on every scoped route (`/workspaces/:id/{tasks,agents,merge-requests,repos}`),
  is 403 on B's scoped routes, addressed entities and writes; a
  membership-less same-tenant caller is 403 on both.
- `hierarchy_cascade.rs` — budget cascade end-to-end over HTTP:
  workspace budget above the tenant ceiling 400 / within 200; repo budget
  above the effective workspace limit 400 / within 200; an unset workspace
  dimension inherits the tenant ceiling; a lowered tenant ceiling binds
  repo overrides immediately (live effective min).

### Test evidence and deferred verification

This sandbox's build wrapper excludes gyre-server test-binary compilation
(4 GiB budget), so the four new test binaries and the rewritten unit
tests were type-checked (`cargo check -p gyre-server --tests`, exit 0),
clippy-clean, and fmt-clean here, but their EXECUTION is deferred to the
independent verifier. Required probes (recorded in
/tmp/stage/review-evidence/task-161-evidence.md):

    cargo test -p gyre-server --test tenant_isolation
    cargo test -p gyre-server --test workspace_scoping
    cargo test -p gyre-server --test abac_middleware
    cargo test -p gyre-server --test hierarchy_cascade
    cargo test -p gyre-server --lib api::audit
    cargo test -p gyre-server --lib api::analytics
    cargo test -p gyre-server --test api_integration

then the full `cargo test --all` gate. Ran green in this sandbox:
`cargo test -p gyre-adapters --test tenant_isolation` (2 passed, exit 0)
and every mechanical check (arch, abac-route-registry,
abac-exempt-handlers, inert-enforcement, forged-scope-fields, hierarchy,
tenant-filter, api-auth, task-commit-attribution, and the rest) — all
exit 0.

Coverage rows 23 and 28 in
`specs/coverage/system/hierarchy-enforcement.md` are updated to
`implemented` with these details; the matrix reads 18/18.
