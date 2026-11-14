---
title: "Implement ABAC policy management UI"
spec_ref: "abac-policy-engine.md §UI"
depends_on:
  - task-128
  - task-129
progress: ready-for-review
coverage_sections:
  - "abac-policy-engine.md §UI"
commits: ["5ba1bdf35f564d19b0e8c1a12a72c46d23781d3d", "11b0a02f6cb47e3197c2455edf384a08568c3adf"]
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

## Shipped

The full `abac-policy-engine.md` §UI surface ships as real production frontend code (commit `11b0a02f`), plus a verification repair (commit `5ba1bdf3`).

### Components (`web/src/components/abac/`)

- **`PolicyManager.svelte`** — management page: scope selector (Tenant / Workspace / Repo) + scope_id input, policy table (name, scope, effect, priority, enabled) with create / edit / delete, and the four §UI views behind a tab bar (Policies / Dry-Run Simulator / Decision Log / Effective Permissions). Edit/delete disabled for server-managed policies (`built_in`, `immutable`, `trust:`/`builtin:` prefixes), mirroring the server's guards. Conflict detection: rows in a detected conflict pair get a warning class + badge; a panel explains each pair.
- **`PolicyEditor.svelte`** — modal form (name, description, scope, scope_id, priority, effect, actions, resource_types, enabled) with the visual condition builder: add/remove condition rows, attribute autocomplete from the §Attributes catalog (Subject / Resource / Environment datalists), all 8 operators, typed value serialization (`in`/`not_in` → string arrays, `greater_than`/`less_than` → numbers, `exists` → null). Create → `POST /api/v1/policies`; update → `PUT /api/v1/policies/{id}`.
- **`PolicySimulator.svelte`** — dry-run: subject (type + id + optional workspace role), action, resource (type + id + workspace/repo/tenant scope chain) → `POST /api/v1/policies/evaluate`; displays decision (Allow/Deny), matched policy, evaluated policy count, evaluation time.
- **`PolicyDecisionLog.svelte`** — decision audit table (subject, action, resource, decision, timestamp, matched policy) with server-side `subject_id`/`resource_type` filters and client-side decision (allow/deny) + date-range filters → `GET /api/v1/policies/decisions`.
- **`EffectivePermissions.svelte`** — subject input → `GET /api/v1/policies/effective?subject={id}` per known resource type; renders resource types × sampled actions with decision and matched policy.
- **`web/src/lib/abac.js`** — §Attributes autocomplete catalog, operator/action/resource-type lists, and `detectPolicyConflicts()`: flags enabled policy pairs with overlapping scope + resource_type + action coverage and conflicting effects, mirroring the engine's `scope_applies` and §Policy Composition winner rules for the explanation text.

### URL pattern

All policy calls use the **global** `/api/v1/policies` endpoints with `?scope=` query parameters (capitalized spelling per the spec's `?scope=Workspace&scope_id=` example); no workspace-prefixed policy URLs. One server fix honors that spec text: `parse_scope` in `crates/gyre-server/src/api/policies.rs` is now case-insensitive (the documented capitalized query form previously 400'd), with `list_policies_scope_query_is_case_insensitive` as the killing test.

### Integration

- **TenantSettings** — Policies tab embeds `PolicyManager` at tenant scope (admin's tenant id prefilled from `me()`).
- **WorkspaceSettings** — Policies tab embeds `PolicyManager` locked to workspace scope (existing trust-grouped summary view and its HSI §2a behavior/testids preserved).
- **RepoSettings** — Policies tab embeds `PolicyManager` locked to repo scope.

### Verification repair (round 2)

The previous verification run (dev-check.sh, exit 1) failed the `check-rustfmt-diff.py` gate: the two trailing `assert!` blocks of `list_policies_scope_query_is_case_insensitive` were not rustfmt-clean (changed lines 1154–1159, 1174–1179). The web build and all other gates passed — the failure was formatting only. Commit `5ba1bdf3` reformats those two asserts to rustfmt's output with no logic change; `check-rustfmt-diff.py` now passes on the full base..HEAD diff.

### Test evidence

- `web/src/__tests__/abac.test.js` — 21 unit tests (catalog + conflict detector, killing cases for each non-overlap dimension and winner rule).
- `web/src/__tests__/PolicyManager.test.js` — 19 component tests (scope query params, CRUD payloads on global endpoints, condition-builder serialization, conflict highlighting, simulator, decision-log filters, effective permissions).
- Integration assertions in the three settings suites (tenant / workspace / repo embedding + scope params).
- Full `web` `npm test` in this round: **115 files, 2351 passed, 41 skipped** (evidence in `/tmp/stage/review-evidence/`).
- `cargo check -p gyre-server --tests` clean; `cargo clippy -p gyre-server --all-targets --all-features` clean for the changed file; rustfmt gate passes. The verifier should run `cargo test -p gyre-server policies::` (server test compilation exceeds the in-sandbox 4 GiB budget), which covers the case-insensitivity test plus all task-128/129 policy API tests.
