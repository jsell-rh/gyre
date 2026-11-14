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
commits: ["23454bedb4cea415385c953c7afd8687de2d15f0", "c459a69bb7af15825092f414e7de3603333f9282", "6e712b451a152eab71b9e3e9d53e69643424c25a", "a9f254b21b69ff07384f379502f5fc1cdff789b8", "1bb51ffe543ab2bcb71df3ce83b1dab0dfe24c97", "3471b92b89a81ae095f8076edc8f23eb2e08e7fb", "0dc466e798e554e648c46e608cf88ff91f86276d", "3571156517e360f1b7e9d34570dc15ea79827b11", "9ba4dd4064ae81fd259628dc8e71620453984dbc", "0c148e2827d70f449c22a30856cc610fbcb1e053", "81ccc2848be396f509f1e6bd37dcad87c4d4bc5f", "0dcb1017d6d7ea3709415efb6a98542f69f5ad61"]
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

### ABAC evaluation engine, API, and audit integration

Production code, in the listed commits:

- **Evaluation engine** (`gyre-server/src/policy_engine.rs`): the 10-step
  flow — subject/resource/action/environment attribute extraction,
  repo → workspace → tenant policy collection, immutable Deny evaluated
  first (bypasses priority ordering, cannot be overridden), then
  priority-DESC with scope specificity (repo > workspace > tenant) and
  Deny-before-Allow as tiebreakers, AND-condition evaluation,
  first-match-wins with short-circuit, default deny. `scope_applies` is
  the single canonical scope filter every evaluation path shares
  (middleware, dry-run, effective-permissions, merge-time, push-time).
- **Built-in policies**: the full §Built-In Policies table seeded at
  startup (`seed_builtin_policies`, fail-closed) — system-full-access
  (1000, matched by the reserved identity `gyre-system-token`),
  tenant-isolation (950, fires only when both tenant ids resolve),
  workspace-membership-required (940, keyed on the membership-store
  boolean), persona-human-approval (950), agent-repo-scope (750, seeded
  enabled with the dynamic `$resource.repo_id` reference), the immutable
  `builtin:require-human-spec-approval`, and default-deny. Built-ins
  cannot be deleted; immutable ones cannot be overridden; drifted
  built-ins are refreshed to the code definition.
- **Performance**: read-through `CachedPolicyRepository` (per-scope set
  caching, invalidated on every port mutation and on out-of-band trust
  transitions); membership lookups precomputed into the auth context;
  mean-latency benchmark over the full seeded set under the 1 ms target.
- **Audit integration**: every evaluation path (middleware, dry-run,
  effective, merge-time, push-time) persists a `PolicyDecision` via
  `record_decision_audit` (denials warn, allowances debug) to the
  queryable `policy_decisions` store.
- **API** (`gyre-server/src/api/policies.rs`): list (scope-filtered),
  create (Admin-only), get, update, delete (not built-ins), dry-run
  `evaluate`, `decisions` audit query, `effective` permissions with
  real-store subject attribute resolution — all registered in
  `api/mod.rs` and the ABAC route registry.
- **Middleware migration**: `abac_middleware.rs` runs every registered
  route through the engine (no system-token bypass — the global token
  is allowed BY `system-full-access`), with reserved-identity and
  scoped-gate-token containment ahead of evaluation.
- **Merge-time attestation gate** (`merge_processor.rs`):
  `evaluate_attestation_abac` returns `Result` and FAILS CLOSED — a
  workspace/policy-store read error requeues the entry (never degrades
  to a tenant-scope-less evaluation); a Deny fails the queue entry via
  the shared `fail_queue_entry` helper (emitting the spec-required
  `merge_queue.processed` failed analytics event) before the merge
  commit is created, plus an `AbacDenied` audit event and a warn-level
  decision log (allowances at debug).

### Verification repair (commit 23454bed)

The round-12 `cargo test --all` failure
  (`merge_processor::tests::attestation_abac_deny_blocks_merge`: entry
  `Merged` instead of `Failed`) was a test-fixture defect, not a product
  defect: the fixture seeded its Deny policy at priority 100, below the
  `builtin:attested-authorization` Allow (150) that the seeded startup
  set contributes, so under §Policy Composition rule 2 ("higher
  priority always wins") the builtin Allow legitimately matched first
  for the attested chain and the merge proceeded. The fixture now uses
  priority 250 — the documented user band (200–299) above the builtin
  trust band, identical to the sibling `evaluate_attestation_abac` unit
  tests — so the Deny decides and the `fail_queue_entry` enforcement
  path the regression test exists to prove is actually exercised.
  No production code changed in this round.
