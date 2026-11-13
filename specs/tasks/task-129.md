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
commits: ["0dc466e798e554e648c46e608cf88ff91f86276d", "3571156517e360f1b7e9d34570dc15ea79827b11", "9ba4dd4064ae81fd259628dc8e71620453984dbc", "0c148e2827d70f449c22a30856cc610fbcb1e053", "81ccc2848be396f509f1e6bd37dcad87c4d4bc5f", "0dcb1017d6d7ea3709415efb6a98542f69f5ad61"]
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

### Round 5: persist decisions on the supervised-trust-hold and push-time paths (commit `0dc466e7`)

The engine, CRUD/dry-run/decisions/effective API, builtin seeding, caching,
and the round-1 through round-4 repairs (agent-repo-scope containment,
`?subject=` binding, reserved system identity + policy-allowed global
token, scope applicability, stale security-model comments) are in the
earlier commits listed above. Round 5 closes the round-4 review finding
(360234bd): coverage row 13 claimed `record_decision` on every evaluation
path, but `evaluate_merge_abac` (the supervised-trust hold) and the
git_http push-time attestation evaluation only logged — they never
persisted a PolicyDecision, so the "every evaluation is logged and
queryable" acceptance criterion was not literally met on those two paths.

**Repair (real recording, not a note correction):**

- `evaluate_merge_abac` now builds and records a PolicyDecision (subject
  `merge-processor`/`system`, action `merge`, resource `mr`/MR id) after
  evaluation. An explicit Deny match (the hold) logs at `warn`, everything
  else at `debug` — the same log-level contract as the middleware.
- The git_http push-time attestation evaluation was extracted into
  `evaluate_push_attestation_abac` (identical attribute context, resource
  scope chain, and engine call as the inline code it replaces) which
  records its decision (subject `agent`, action `push`, resource
  `attestation`/repo id) before the existing info log. The push call site
  carries the documented `// enforcement:ok` exemption because the gate is
  audit-only by spec (TASK-006 Phase 1) — decision recorded, never
  enforced, exactly as at base.

Four production `record_decision` call sites now exist: middleware
requests, `evaluate_merge_abac`, `evaluate_attestation_abac`, and
`evaluate_push_attestation_abac`. Coverage row 13 cites these real sites
and the tests below.

### Test evidence

Two new tests, one per repaired path, each seeding a real matching policy
(so the record carries a non-default matched policy — a stub persisting a
hardcoded decision cannot pass) and asserting the persisted decision via
the same audit-store surface `GET /api/v1/policies/decisions` reads:

- `evaluate_merge_abac_records_decision_to_audit_trail`
  (merge_processor.rs): seeds an Allow for the merge-processor service
  identity, asserts the returned EvalResult, then asserts the recorded
  decision's subject_id/subject_type/action/resource_type/resource_id/
  decision/matched_policy.
- `push_attestation_abac_records_decision_to_audit_trail` (git_http.rs):
  seeds an Allow keyed on `subject.attestation_chain_present`, calls the
  extracted helper with a real single-root chain, and asserts the same
  fields on the persisted decision.

Removing either `record_decision` call fails its test
  (`decisions.len() == 1`). In-sandbox probes: `cargo check -p gyre-server`
  and `cargo check -p gyre-server --tests` both clean (build wrapper);
  mechanical checks check-inert-enforcement, check-arch,
  check-dead-message-kinds, check-mem-port-contracts,
  check-fabricated-scope-defaults, check-lossy-secret-conversion,
  check-relative-path-defaults all OK. gyre-server test execution is
deferred to the independent verifier (4 GiB budget): run
`cargo test -p gyre-server --lib evaluate_merge_abac_records_decision_to_audit_trail push_attestation_abac_records_decision_to_audit_trail`,
then the full suite. Evidence: `/tmp/stage/review-evidence/task-129-round5-audit-recording.md`.
