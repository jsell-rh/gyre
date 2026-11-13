---
title: "Implement ABAC evaluation engine, API, and audit integration"
spec_ref: "abac-policy-engine.md §Evaluation Engine"
depends_on:
  - task-128
progress: ready-for-review
coverage_sections:
  - "abac-policy-engine.md §Evaluation Engine"
  - "abac-policy-engine.md §Evaluation Flow"
  - "abac-policy-engine.md §Policy Composition"
  - "abac-policy-engine.md §Built-In Policies"
  - "abac-policy-engine.md §Performance"
  - "abac-policy-engine.md §Audit Integration"
  - "abac-policy-engine.md §API"
  - "abac-policy-engine.md §Dry-Run Evaluation"
commits: ["9ba4dd4064ae81fd259628dc8e71620453984dbc", "0c148e2827d70f449c22a30856cc610fbcb1e053", "81ccc2848be396f509f1e6bd37dcad87c4d4bc5f", "0dcb1017d6d7ea3709415efb6a98542f69f5ad61", "3571156517e360f1b7e9d34570dc15ea79827b11"]
---

## Spec Excerpt

From `abac-policy-engine.md` §Evaluation Engine:

**Evaluation Flow:**
1. Extract subject attributes (from auth context, OIDC token, memberships)
2. Extract resource attributes (from entity lookup)
3. Extract action (from request method + route)
4. Extract environment attributes (time, IP, system state)
5. Collect applicable policies: repo → workspace → tenant scope
6. Check immutable Deny policies first (cannot be overridden by any Allow)
7. Sort remaining policies by priority (highest first)
8. Evaluate each policy's conditions (all conditions must match = AND)
9. First matching policy wins
10. Log decision to audit trail

**Policy Composition:**
1. More specific scope takes precedence when priorities are equal (repo > workspace > tenant)
2. Higher priority always wins regardless of scope
3. Deny overrides Allow at the same priority and scope
4. Default deny at tenant level catches everything not explicitly allowed

**Built-In Policies:**

| Policy | Effect | Purpose |
|---|---|---|
| `system-full-access` | Allow | Global GYRE_AUTH_TOKEN gets full access |
| `tenant-isolation` | Deny | Users/agents can't access other tenants |
| `agent-repo-scope` | Deny | Agents can't access resources outside their scoped repo |
| `workspace-membership-required` | Deny | Users must be workspace members |
| `persona-human-approval` | Deny | Agents can't approve personas |
| `builtin:require-human-spec-approval` | Deny (immutable) | Non-user subjects cannot approve specs |
| `default-deny` | Deny | Everything not explicitly allowed is denied |

**API:**

| Endpoint | Method | Purpose |
|---|---|---|
| `GET /api/v1/policies` | GET | List policies (filtered by scope/scope_id) |
| `POST /api/v1/policies` | POST | Create policy (Admin only) |
| `GET /api/v1/policies/{id}` | GET | Get policy details |
| `PUT /api/v1/policies/{id}` | PUT | Update policy |
| `DELETE /api/v1/policies/{id}` | DELETE | Delete policy (not built-ins) |
| `POST /api/v1/policies/evaluate` | POST | Dry-run evaluation |
| `GET /api/v1/policies/decisions` | GET | Query audit log |
| `GET /api/v1/policies/effective` | GET | Effective permissions for a subject |

**Audit Integration:**
```rust
pub struct PolicyDecision {
    pub request_id: String,
    pub subject_id: String,
    pub subject_type: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub decision: PolicyEffect,
    pub matched_policy: Option<Id>,
    pub evaluated_at: u64,
}
```

## Implementation Plan

1. **Evaluation engine in `gyre-server`:**
   - Implement the 10-step evaluation flow
   - Immutable Deny evaluated first (bypass priority ordering)
   - Priority-based evaluation: sort by priority DESC, scope specificity as tiebreaker
   - First-match-wins semantics
   - Short-circuit on match

2. **Policy composition:**
   - Scope precedence: repo > workspace > tenant (at equal priority)
   - Higher priority always wins
   - Deny overrides Allow at same priority+scope

3. **Built-in policies:**
   - Seed 7 built-in policies on startup (or via migration)
   - Mark as `immutable` where specified (require-human-spec-approval)
   - Built-ins cannot be deleted
   - Non-immutable built-ins can be overridden by higher-priority custom policies

4. **Performance:**
   - Cache policy sets per scope (invalidate on policy change)
   - Precompute membership lookups in auth context
   - Target: <1ms per evaluation

5. **Audit integration:**
   - Log every policy decision (PolicyDecision struct)
   - Denied requests at `warn` level, allowed at `debug`
   - Store decisions in queryable table
   - Migration for `policy_decisions` table

6. **API endpoints:**
   - CRUD for policies: `GET/POST /api/v1/policies`, `GET/PUT/DELETE /api/v1/policies/{id}`
   - Dry-run: `POST /api/v1/policies/evaluate`
   - Audit log: `GET /api/v1/policies/decisions`
   - Effective permissions: `GET /api/v1/policies/effective`
   - Register all routes in `api/mod.rs`

7. **Replace existing ABAC middleware:**
   - Migrate `abac_middleware.rs` to use the new evaluation engine
   - Ensure all existing route protections continue to work

## Acceptance Criteria

- [ ] 10-step evaluation flow implemented
- [ ] Immutable Deny policies evaluated before all others (cannot be overridden)
- [ ] Priority-based evaluation with scope tiebreaker
- [ ] First-match-wins with short-circuit
- [ ] 7 built-in policies seeded on startup
- [ ] Built-in policies cannot be deleted; immutable ones cannot be overridden
- [ ] Policy caching with invalidation on change
- [ ] <1ms evaluation target (verify with benchmark test)
- [ ] PolicyDecision audit logging on every evaluation
- [ ] CRUD API endpoints: list, create, get, update, delete
- [ ] Dry-run evaluation endpoint
- [ ] Decision audit query endpoint
- [ ] Effective permissions query endpoint
- [ ] Existing ABAC middleware migrated to new engine
- [ ] All routes registered in `api/mod.rs`
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/abac-policy-engine.md` §Evaluation Engine through §Dry-Run Evaluation. Existing ABAC: `gyre-server/src/abac.rs` (current policy evaluation), `abac_middleware.rs` (route protection), `policy_engine.rs` (if exists). Route registration: `gyre-server/src/api/mod.rs`. Policy domain: `gyre-domain/src/policy.rs`. Policy adapter: `gyre-adapters/src/sqlite/policy.rs`. Built-in policy seeding: grep for `builtin_policies\|seed_policies` in the codebase. The evaluation engine must integrate with the existing auth middleware — check `gyre-server/src/auth.rs` for how auth context is available in handlers. Verify API endpoint paths against `api/mod.rs` route registration before implementing.

## Shipped

### Round 4: scope applicability on the evaluation path (commit `35711565`)

The engine, CRUD/dry-run/decisions/effective API, audit integration,
builtin seeding, caching, the round-1/2 repairs (agent-repo-scope
containment, `?subject=` binding) and the round-3 repairs (reserved
system identity, policy-allowed global token) are in the earlier commits
listed above. Round 4 closes the two remaining review findings:

**1. Scope applicability (the escalation).** `evaluate` retained
policies only on `enabled && applies_to(action, resource_type)` —
`Policy::applies_to` never checks `scope`/`scope_id`, and the middleware
fed it the entire store, so a ws-A-scoped Allow granted Developers in
ws-B (and other tenants) and a scoped Deny restricted globally. The fix
is ONE canonical filter, `policy_engine::scope_applies`, applied inside
`evaluate` so every caller shares it:

- `scope_id: Some(id)` requires the addressed resource's scope-chain
  attribute (`resource.repo_id` / `resource.workspace_id` /
  `resource.tenant_id`, extracted before evaluation per §Evaluation Flow
  steps 1-4) to be present AND equal — a ws-A policy never applies to a
  ws-B request, a t-1 policy never governs t-2 resources, and no scope
  is fabricated when the level is unaddressed;
- `scope_id: None` means all entities at that level — every seeded
  builtin (all Tenant/None) stays global.

Call-site alignment: `evaluate_merge_abac` drops its private
workspace-only filter and delegates to the canonical one (its ctx now
sets `resource.tenant_id`, so tenant-scoped policies are tenant-scoped
at merge time too); `evaluate_attestation_abac` and the git-http
push-attestation ctx carry the full resource scope chain (the push path
resolves the tenant from the repo's workspace, never the caller's
claim); the dry-run endpoint accepts `resource.tenant_id`; the effective
endpoint gained optional `workspace_id`/`repo_id`/`tenant_id` params so
scoped policies can be assessed against a concrete scope.

**2. Stale security-model comments.** `auth.rs` (`jwt_claims` doc) and
`git_http.rs` (`resolve_repo_by_slug`) claimed the global token/API keys
"bypass ABAC" — false since round 3 (API keys always evaluate; the
global token is allowed BY `system-full-access`). Both now state the
actual behavior, so the comments no longer misdocument the escalation
surface.

### Test evidence

New tests: five engine unit tests (workspace/repo/tenant
non-applicability, None-means-all/keeps-builtins-global, immutable-Deny
still overrides an in-scope Allow), two live middleware tests
(`scoped_workspace_deny_does_not_apply_out_of_scope_live`,
`scoped_workspace_allow_does_not_escalate_across_workspaces_live` — the
reviewer's exact repro shape, each asserting both the out-of-scope
non-effect and the in-scope effect via the decision audit's
`matched_policy`), and a dry-run API test
(`evaluate_respects_policy_scope`).

In this sandbox `cargo check -p gyre-server` and `cargo check -p
gyre-server --tests` pass (exit 0, no warnings); gyre-server test
compilation exceeds the 4 GiB cgroup budget and is delegated to the
independent verifier — the required probes (focused new tests plus
abac/policy/merge/git_http regression scopes) are listed in
`/tmp/stage/review-evidence/round4-scope-applicability.md`.
