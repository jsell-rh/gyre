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
commits: ["1bb51ffe543ab2bcb71df3ce83b1dab0dfe24c97", "3471b92b89a81ae095f8076edc8f23eb2e08e7fb", "0dc466e798e554e648c46e608cf88ff91f86276d", "3571156517e360f1b7e9d34570dc15ea79827b11", "9ba4dd4064ae81fd259628dc8e71620453984dbc", "0c148e2827d70f449c22a30856cc610fbcb1e053", "81ccc2848be396f509f1e6bd37dcad87c4d4bc5f", "0dcb1017d6d7ea3709415efb6a98542f69f5ad61"]
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

### What is implemented (all in the listed commits)

The ABAC evaluation engine and its full surface are in production code:

- **Evaluation engine** (`gyre-server/src/policy_engine.rs`): the
  10-step flow — subject/resource/action/environment attribute
  extraction, repo → workspace → tenant policy collection, immutable
  Deny evaluated first (bypasses priority ordering and cannot be
  overridden), then priority-DESC ordering with scope specificity as
  tiebreaker, AND-condition evaluation, first-match-wins with
  short-circuit, and a `PolicyDecision` audit record per evaluation.
- **Policy composition** per §Policy Composition: scope precedence
  (repo > workspace > tenant) at equal priority, higher priority wins
  regardless of scope, Deny overrides Allow at the same priority+scope,
  tenant-level `default-deny` catches the rest.
- **Built-ins**: all seven policies from §Built-In Policies are seeded
  on startup (`system-full-access`, `tenant-isolation`,
  `agent-repo-scope`, `workspace-membership-required`,
  `persona-human-approval`, `builtin:require-human-spec-approval`
  (immutable, priority 999), `default-deny`). Built-ins cannot be
  deleted; immutable ones cannot be overridden; trust-preset names
  (`trust:`/`builtin:` prefix) are protected on update/delete by id.
- **Performance**: per-scope policy-set cache with invalidation on
  every policy write (`policy_cache.rs`); membership lookups
  precomputed in the auth context; benchmark test asserts the <1ms
  evaluation target.
- **Audit integration**: every decision is persisted to the
  `policy_decisions` table and logged (Deny at `warn`, Allow at
  `debug`); audit-write failures warn via
  `policy_engine::record_decision_audit` so a dropped decision is
  distinguishable from "no decisions".
- **API**: `GET/POST /api/v1/policies`, `GET/PUT/DELETE
  /api/v1/policies/{id}` (create admin-only, built-ins undeletable),
  `POST /api/v1/policies/evaluate` (dry-run), `GET
  /api/v1/policies/decisions` (audit query), `GET
  /api/v1/policies/effective` (effective permissions), all registered
  in `api/mod.rs` with ABAC route-registry entries.
- **Middleware migration**: `abac_middleware.rs` evaluates through the
  new engine; attestation-gate paths fail closed on store errors
  (hold-then-requeue on the enforced merge path, warn+skip on the
  audit-only push path).

Review rounds 1–6 (findings and fixes) are recorded in the earlier
listed commits; round 6 closed the last seven findings, including
re-spawn JWT worker-scope claims (`mint_worker_scoped`), reserved-
prefix protection on policy update/delete, concrete decision resource
ids, visible audit-write failures, and set-semantics
`subject.global_role` handling.

### Round 7 (this commit): verification-gate formatting repair

The round-6 checkpoint failed the deterministic formatting gates —
one trailing-whitespace line (`policy_engine.rs:524`) and rustfmt
violations on task-129-changed lines in nine files. Because the gate
truncates each file's violation list at twelve lines, each flagged
file was fully reformatted with the gate's exact invocation
(`rustfmt --edition 2021 --config skip_children=true`). Formatting
only — no behavior change.

### Test evidence

- `python3 scripts/check-rustfmt-diff.py f7f5c59` → "changed lines
  clean (45 Rust files checked)" (exit 0); `git diff --check` on the
  branch → no whitespace errors. Evidence saved under
  `/tmp/stage/review-evidence/`.
- `cargo check -p gyre-server` passes on the reformatted tree
  (exit 0, ~6 min).
- Behavior tests from rounds 1–6 (one per finding, each with a
  documented killing direction): `respawn_jwt_carries_worker_scope_claims`,
  `condition_equals_on_list_attribute_uses_set_semantics`,
  `roleless_caller_gets_no_fabricated_readonly_role`,
  `decision_records_concrete_resource_id`,
  `trust_preset_policy_cannot_be_updated_or_deleted_by_id`,
  `attestation_deny_records_abac_denied_audit_event`,
  `attestation_abac_fails_closed_on_workspace_store_error`, plus the
  engine/benchmark/cache/API suites in `policy_engine`,
  `abac_middleware`, `api::policies`, `merge_processor`,
  `ralph_loop`, and `git_http` tests.

This sandbox is cgroup-capped at 4 GiB, so gyre-server test-binary
compilation is deferred to the independent verifier: the verifier
must run the tests above (`cargo test -p gyre-server policy_engine
abac_middleware api::policies merge_processor ralph_loop git_http`
plus the seven named round-6 tests) — `cargo check -p gyre-server
--tests` type-checks all of them.
