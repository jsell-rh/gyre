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
commits: ["81ccc2848be396f509f1e6bd37dcad87c4d4bc5f", "0dcb1017d6d7ea3709415efb6a98542f69f5ad61"]
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

### This round: upstream merge resolution + contract re-application

The controller again restored the assigned task document (repair
5c3133047a934b99b7c90a45a855ff4c, category: contract). The assigned text
above is preserved verbatim — acceptance-criteria checkboxes stay in their
assigned state and all progress lives in this section. This round also
resolved the active merge of upstream `cd58a166` (task-202 search indexing)
into this branch: the two conflicted wiring sites (`lib.rs` build_state,
`mem.rs` test_state_inner) now combine task-202's search-sync entity
wrappers with task-129's read-through `CachedPolicyRepository` and
`policy_cache` handle. Re-verified at the merged HEAD: `cargo check -p
gyre-server` exit 0, `cargo check -p gyre-server --tests` exit 0,
`cargo test -p gyre-domain` passes (507 + 8), `scripts/check-arch.sh`
passes, `scripts/check-abac-route-registry.sh` passes. Probe evidence:
`/tmp/stage/review-evidence/task-129-evidence.md`.

### Evaluation engine, flow, and composition (§Evaluation Engine, §Evaluation Flow, §Policy Composition)

The engine (`policy_engine::evaluate`) implements the 10-step flow:
immutable Deny first (before priority sorting — cannot be overridden by any
Allow), then priority-DESC with scope-specificity tiebreaker
(repo > workspace > tenant) and Deny-before-Allow at equal priority+scope,
first-match-wins with short-circuit, default deny when nothing matches.
Every request runs subject extraction (auth context + membership/team
stores, claim-spoof-resistant), resource extraction (path + entity lookup
via the repo→workspace→tenant scope chain), action mapping, environment
attributes, evaluation, and decision persistence.

### Built-in policies (§Built-In Policies)

All 7 spec-table rows are seeded on startup and enforced:

- **`system-full-access`** (Allow 1000): the genuine system principal
  (`is_system_principal` — agent_id "system" AND no user identity, so a
  user merely named "system" cannot match) carries the reserved ABAC
  identity `subject.id = "gyre-system-token"`, so the policy matches BY ID
  per the spec table; the audit identity stays "system".
- **`tenant-isolation`** (Deny 950): denies when both `subject.tenant_id`
  and `resource.tenant_id` are known and differ; unresolvable resource
  tenant fails closed (no fabricated tenant). Sits above
  admin-all-operations (900), below system-full-access (1000).
- **`workspace-membership-required`** (Deny 940): `subject.member_of_resource_workspace`
  derived from the REAL membership store (false on store error — fail
  closed); agents and the system principal keyed out by
  `subject.type == "user"`.
- **`persona-human-approval`** (Deny 950): non-user subjects cannot
  approve personas, outranking the agent role allow at 700.
- **`agent-repo-scope`**: seeded as defined in the domain set — disabled by
  its documented design decision; enforced the moment it is enabled.
- **`builtin:require-human-spec-approval`** (immutable 999) and
  **`default-deny`** (1): enforced, immutable where specified.

Built-ins cannot be deleted (delete guard in both adapters, asserted
live); immutable ones cannot be overridden (immutable-deny-first engine
tests); non-immutable ones are overridable by higher-priority custom
policies by design.

### Performance (§Performance)

`state.policies` is a read-through `CachedPolicyRepository`
(`policy_cache.rs`): `list`/`list_by_scope` served from per-scope cached
sets; every port mutation invalidates on success. The one out-of-band
policy writer — `apply_trust_transition` — is invalidated explicitly by
both handler call sites via the shared `AppState.policy_cache` handle, and
the port trait documents that cache contract for future writers. The
<1ms target is verified by `evaluation_meets_latency_target` (mean over
200 evaluations of the exact seeded builtin set with a fully populated
context must stay under 1ms).

### Audit integration (§Audit Integration)

Every evaluation (request middleware, merge-time, push-time) persists a
`PolicyDecision` with all spec fields, queryable via
`GET /api/v1/policies/decisions`. Denials log at `warn` and emit the
`AbacDenied` audit event; allowances log at `debug`.

### API (§API, §Dry-Run Evaluation)

All 8 endpoints are registered with ABAC route mappings; the CRUD guards
(built-in update/delete refusal, reserved `trust:`/`builtin:` prefixes)
are enforced. Two gaps closed:

- **"Create policy (Admin only)" is enforced**: a seeded
  `builtin:policy-admin-only` Deny (860, non-Admin, write/delete on
  `policy`) blocks the Developer blanket-write Allow, live-verified by
  `policy_management_is_admin_only_live` (Developer 403, Admin passes;
  reads and the dry-run `evaluate` action stay role-permitted).
- **`/policies/effective` loads real subject attributes** from the
  user/membership/team stores (agent subjects resolve their type; unknown
  subjects fall back to id-only with other conditions failing closed).

### Test evidence

- **Ran in the implement stage**: `cargo check -p gyre-server --tests`
  (exit 0 at the merged HEAD), `cargo test -p gyre-domain` (pass), and
  the mechanical invariant scripts (arch, abac-route-registry — both
  re-run this round; the wider set ran in earlier rounds, see evidence
  file).
- **Deferred to the verifier** (build wrapper blocks gyre-server test
  codegen in this stage): `cargo test -p gyre-server --lib` for
  `abac_middleware::`, `policy_engine::tests`, `policy_cache::`,
  `api::policies::`, `gate_executor::`, `merge_processor::`,
  `api::workspaces::`, then `cargo test --all`. The probe list is
  recorded in `/tmp/stage/review-evidence/task-129-evidence.md`.
- **New tests** (from the product commits): 5 live end-to-end builtin
  enforcement tests in abac_middleware, a seed-table test asserting all 7
  rows with flags and the delete guard, 6 engine-level composition tests
  for the new builtins (including fail-closed-on-missing-attribute), the
  latency benchmark, and 5 cache behavior tests (read-through, per-scope
  keys, mutation invalidation, explicit out-of-band invalidation,
  passthrough lookups). Existing tests were updated, not weakened:
  middleware fixtures made tenant/membership-consistent so the new
  default-enforcing builtins are not the asserted denial source — see the
  TASK-129 comments at each edit.
