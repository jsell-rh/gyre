---
title: "Implement ABAC policy management UI"
spec_ref: "abac-policy-engine.md §UI"
depends_on:
  - task-128
  - task-129
progress: not-started
coverage_sections:
  - "abac-policy-engine.md §UI"
commits: []
---

## Spec Excerpt

From `abac-policy-engine.md` §UI:

- **Policy management page** (tenant/workspace/repo scoped): list, create, edit, delete policies
- **Policy editor:** visual condition builder with attribute autocomplete
- **Dry-run simulator:** "what would happen if user X tried to do Y on resource Z?"
- **Decision audit log:** filterable table of all policy evaluations with Allow/Deny outcome
- **Effective permissions view:** select a user/agent, see what they can do across all resource types
- **Policy conflict detector:** highlight policies that could produce unexpected interactions

**URL pattern:** The UI uses the **global** policy endpoints (`/api/v1/policies`) with `?scope=Workspace&scope_id=<workspace_id>` query parameters for workspace-scoped views. There are no separate `/workspaces/:id/abac-policies` endpoints — workspace scoping is a filter on the global CRUD, not a separate resource hierarchy. The dry-run simulator uses `POST /api/v1/policies/evaluate` (not a workspace-scoped simulate endpoint). The frontend must use these global URLs — workspace-prefixed policy URLs will return 405.

## Implementation Plan

1. **Policy management page component (`PolicyManager.svelte`):**
   - Scope selector (Tenant / Workspace / Repo) with scope_id input
   - Table of policies with columns: name, scope, effect, priority, enabled, actions
   - Create / Edit / Delete actions
   - Calls `GET /api/v1/policies?scope={scope}&scope_id={id}`

2. **Policy editor component (`PolicyEditor.svelte`):**
   - Form for policy fields: name, description, scope, priority, effect, actions, resource_types
   - Visual condition builder: add/remove conditions, select attribute (with autocomplete from known attribute list), operator, value
   - Subject/Resource/Action/Environment attribute autocomplete
   - Create: `POST /api/v1/policies`
   - Update: `PUT /api/v1/policies/{id}`

3. **Dry-run simulator component (`PolicySimulator.svelte`):**
   - Subject picker: type (user/agent) + id
   - Action picker: dropdown of known actions
   - Resource picker: type + id
   - Submit: `POST /api/v1/policies/evaluate`
   - Display: decision (Allow/Deny), matched policy name, evaluation time, evaluated policy count

4. **Decision audit log component (`PolicyDecisionLog.svelte`):**
   - Filterable table: subject, action, resource, decision, timestamp, matched policy
   - Filters: subject type/id, decision (allow/deny), date range
   - Calls `GET /api/v1/policies/decisions`

5. **Effective permissions view component (`EffectivePermissions.svelte`):**
   - Subject selector (user/agent dropdown)
   - Table showing resource types × allowed actions
   - Calls `GET /api/v1/policies/effective?subject_type={type}&subject_id={id}`

6. **Policy conflict detector:**
   - Analyze loaded policies client-side for overlapping scope+resource_type+action combinations with conflicting effects
   - Highlight conflicting pairs with explanation

7. **Integration into settings UI:**
   - Tenant-level: accessible from TenantSettings
   - Workspace-level: accessible from WorkspaceSettings
   - Repo-level: accessible from RepoSettings

## Acceptance Criteria

- [ ] Policy management page with CRUD operations
- [ ] Scope filtering (tenant/workspace/repo)
- [ ] Visual condition builder with attribute autocomplete
- [ ] Dry-run simulator with decision display
- [ ] Decision audit log with filters
- [ ] Effective permissions view per subject
- [ ] Policy conflict detection highlights
- [ ] Uses global `/api/v1/policies` endpoints with scope query params (NOT workspace-prefixed URLs)
- [ ] Integrates into settings pages at all three scope levels
- [ ] `npm test` passes (web/ component tests)
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/abac-policy-engine.md` §UI for the full spec. Frontend: `web/src/`. Existing settings components: `web/src/lib/components/` — look at TenantSettings.svelte, WorkspaceSettings.svelte for integration patterns. API client: check how other components call the REST API. API endpoints: `GET /api/v1/policies`, `POST /api/v1/policies`, `PUT /api/v1/policies/{id}`, `DELETE /api/v1/policies/{id}`, `POST /api/v1/policies/evaluate`, `GET /api/v1/policies/decisions`, `GET /api/v1/policies/effective` — all at global scope with `?scope=` query params. Svelte 5 patterns: check existing components for $state, $derived usage. Do NOT use workspace-prefixed policy URLs.
