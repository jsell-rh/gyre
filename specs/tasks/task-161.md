---
title: "Hierarchy enforcement integration tests and audit auth hardening"
spec_ref: "hierarchy-enforcement.md §7"
depends_on: [task-160]
progress: not-started
coverage_sections:
  - "hierarchy-enforcement.md §New Integration Tests"
  - "hierarchy-enforcement.md §Audit/Analytics/Cost Recording"
commits: []
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
