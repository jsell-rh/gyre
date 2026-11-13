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
commits: ["9ba4dd4064ae81fd259628dc8e71620453984dbc", "0c148e2827d70f449c22a30856cc610fbcb1e053", "81ccc2848be396f509f1e6bd37dcad87c4d4bc5f", "0dcb1017d6d7ea3709415efb6a98542f69f5ad61"]
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

### Round 3: system-full-access privilege escalation + dead-code bypass repair (commit `9ba4dd40`)

The engine, CRUD/dry-run/decisions/effective API, audit integration,
builtin seeding, caching, and the round-1/round-2 repairs
(agent-repo-scope containment, `?subject=` param binding) are in the
earlier commits listed above. Round 3 repairs the two remaining review
findings on the newly-seeded `system-full-access` builtin:

**1. Privilege escalation via a spoofed reserved identity.** The policy
matches `subject.id == "gyre-system-token"`, but the live middleware
resolves `subject.id` from `auth.agent_id` — the user's display_name for
JWT/API-key principals, caller-settable through PUT /api/v1/users/me,
admin user creation, SCIM provisioning, and the SSO `preferred_username`
claim. The identity is now **reserved** (`gyre_domain::SYSTEM_ABAC_IDENTITY`,
next to the seed that uses it):

- PUT /api/v1/users/me rejects the string as display_name (400, nothing
  applied — `put_me_rejects_reserved_system_display_name`);
- admin POST /api/v1/users rejects it as a username (display_name derives
  from it — `create_user_rejects_reserved_system_username`);
- SCIM create/update reject it as displayName, including the
  userName-fallback case (`scim_rejects_reserved_system_identity`);
- SSO provisioning fails closed on `preferred_username` = the reserved
  string, and re-login of a pre-existing user whose stored display_name is
  the reserved string is rejected (401, no user row created —
  `jwt_rejects_reserved_system_identity_provisioning`);
- **evaluation-time containment**: the middleware denies (403, warn log,
audited PolicyDecision) any NON-system principal whose agent_id is the
reserved string — covers pre-existing rows and direct DB edits that no
write-path validation can reach (`spoofed_system_identity_denied_before_evaluation`:
an API-key user whose display_name is the reserved string gets 403 before
any policy, including system-full-access, is evaluated).

**2. Dead-code mitigation / unreachable policy.** The
`ctx.set("subject.id", "gyre-system-token")` mapping was unreachable — the
identical `is_system_principal()` predicate returned the request earlier
(system token bypassed ABAC entirely), so the genuine global
GYRE_AUTH_TOKEN never reached the policy that exists to allow it, and the
seeded policy could only ever be matched by the spoofed identity above.
The early bypass is **removed**: the system principal now flows through
the normal evaluation flow — attribute extraction, immutable-Deny check,
priority sort — and is allowed BY `system-full-access` at priority 1000
(above tenant-isolation 950 and every role band; the only immutable Deny
`builtin:require-human-spec-approval` keys on `subject.type != "user"` and
does not match the system principal, whose Admin-role type derivation is
"user", so spec-approval behavior is unchanged). The decision audit trail
now proves the mechanism instead of a misleading rationale:
`system_token_allowed_by_system_full_access_policy` (renamed from
`system_token_bypasses_abac`) asserts the recorded decision has
`matched_policy == builtin-system-access`, and
`builtin_tenant_isolation_denies_cross_tenant_live` asserts the same for
the global token's cross-tenant 200. Blast-radius analysis: api-module
tests never layer the ABAC middleware (only `build_router` does); the
only two global-token usages in middleware tests use builtin-seeded
states; no seeded or test-created policy outranks (1000, tenant, allow).

The spec (abac-policy-engine.md §Built-In Policies) and the coverage
matrix row 11 were amended to document the reservation and the
allowed-by-policy (no-bypass) mechanism.

### Test evidence

Run in this sandbox (gyre-server codegen is blocked by the build
wrapper — server tests are deferred to the independent verifier):

- `cargo check -p gyre-server --tests` — exit 0 (all new code, including
  test code, type-checks).
- `cargo test -p gyre-domain` — 507 + 8 passed, 0 failed.
- `scripts/check-arch.sh`, `check-abac-route-registry.sh`,
  `check-scope-literal-defaults.sh`, `check-fabricated-scope-defaults.sh`,
  `check-inert-enforcement.sh`, `check-mem-port-contracts.sh`,
  `check-fail-open-ref-resolution.sh`, `check-lossy-secret-conversion.sh`
  — all pass.

Verifier must run (focused, then full):

- `cargo test -p gyre-server --lib system_token_allowed_by_system_full_access_policy`
- `cargo test -p gyre-server --lib spoofed_system_identity_denied_before_evaluation`
- `cargo test -p gyre-server --lib builtin_tenant_isolation_denies_cross_tenant_live`
- `cargo test -p gyre-server --lib put_me_rejects_reserved_system_display_name`
- `cargo test -p gyre-server --lib create_user_rejects_reserved_system_username`
- `cargo test -p gyre-server --lib jwt_rejects_reserved_system_identity_provisioning`
- `cargo test -p gyre-server --lib scim_rejects_reserved_system_identity`
- full `cargo test --all` (prior rounds' suites unchanged:
  agent_repo_scope, policies API, decisions audit, dry-run, effective)
