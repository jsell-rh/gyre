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
commits: ["3471b92b89a81ae095f8076edc8f23eb2e08e7fb", "0dc466e798e554e648c46e608cf88ff91f86276d", "3571156517e360f1b7e9d34570dc15ea79827b11", "9ba4dd4064ae81fd259628dc8e71620453984dbc", "0c148e2827d70f449c22a30856cc610fbcb1e053", "81ccc2848be396f509f1e6bd37dcad87c4d4bc5f", "0dcb1017d6d7ea3709415efb6a98542f69f5ad61"]
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

### Round 6: repair the seven review findings from checkpoint a989d27 (commit `3471b92b`)

The engine, CRUD/dry-run/decisions/effective API, builtin seeding,
caching, and the round-1 through round-5 repairs are in the earlier
merged commits. Round 6 closes all seven findings of the round-5
review:

1. **Re-spawn containment (agent-repo-scope).**
   `relaunch_agent_session` now mints the fresh session JWT via
   `mint_worker_scoped` — the same worker-scope shape as the first
   spawn (`workspace_id` + `repo_id` claims, attestation level mirrored
   from the resolved compute target) — instead of `mint_with_workload`,
   which hardcoded `repo_id: None`. A re-spawned agent's requests now
   carry a real `subject.repo_scope`, so the `builtin-agent-repo-scope`
   containment Deny (750) fires against out-of-scope repos instead of
   `builtin-agent-scoped-access` (700) granting tenant-wide access.
2. **Fail-closed attestation gates.** `evaluate_attestation_abac`
   (enforced merge path) propagates workspace-store and policy-store
   read errors as `Err`; both merge-path callers requeue the queue entry
   (hold-then-retry, same semantics as `supervised_trust_hold`) instead
   of evaluating with a silently missing tenant scope.
   `evaluate_push_attestation_abac` propagates the same reads; its
   caller logs a warn and skips the audit-only evaluation on error.
3. **Reserved-prefix invariant on update/delete.** `update_policy`
   and `delete_policy` now reject when the STORED policy name carries
   the `trust:`/`builtin:` prefix, so trust-preset policies (built_in:
   false, immutable: false) can no longer be disabled, rewritten, or
   deleted by id — only the atomic, transition-validated
   `apply_trust_transition` path mutates them.
4. **§Audit Integration log-level contract + AbacDenied events** on
   both attestation paths: denials at `warn`, allowances at `debug`, and
   a Deny emits an `AbacDenied` audit event via the new
   `record_abac_denied_service` (marked `enforced: true` on the merge
   gate, `enforced: false` on the audit-only push path).
5. **Concrete decision resource ids.** `PolicyDecision.resource_id`
   now records the addressed entity id (`resource.id` from the
   evaluation context / path extraction; `addressed_resource_id` /
   `path_resource_id` helpers), falling back to the route pattern only
   on collection routes.
6. **Visible audit-write failures.** All six `record_decision` sites go
   through `policy_engine::record_decision_audit`, which warn-logs on
   store failure — a dropped decision is distinguishable from "no
   decisions".
7. **`subject.global_role` set semantics.** Empty roles leave the
   attribute unset (fail closed — the fabricated `ReadOnly` that
   matched `builtin-readonly-get-only` is gone); multi-role callers
   get a list-valued attribute, and `eval_condition` evaluates
   Equals/NotEquals on list values with membership semantics. The
   effective-permissions endpoint's extraction was aligned.

### Test evidence

Seven new focused tests (one per finding, each with a documented
killing direction): `respawn_jwt_carries_worker_scope_claims`,
`condition_equals_on_list_attribute_uses_set_semantics`,
`roleless_caller_gets_no_fabricated_readonly_role`,
`decision_records_concrete_resource_id`,
`trust_preset_policy_cannot_be_updated_or_deleted_by_id`,
`attestation_deny_records_abac_denied_audit_event`, and
`attestation_abac_fails_closed_on_workspace_store_error` (injects a
failing workspace store and asserts the `Err`).

This sandbox is cgroup-capped at 4 GiB, so gyre-server test-binary
compilation is deferred to the independent verifier per the assignment
instructions: `cargo check -p gyre-server --tests` (which type-checks
every new test) passes — evidence in
`/tmp/stage/review-evidence/`. The verifier must run the seven tests
above plus the touched-surface suites (`abac_middleware`,
`merge_processor`, `ralph_loop`, `policy_engine`, `api::policies`,
`git_http`) — see `review-evidence/README.md` for the exact commands.
All mechanical repo checks (arch, inert-enforcement, abac-route-registry
and the other frozen-count scripts) pass.
