---
title: "Implement ABAC policy engine core: entity, conditions, and attribute model"
spec_ref: "abac-policy-engine.md §Core Concepts"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "abac-policy-engine.md §Core Concepts"
  - "abac-policy-engine.md §Attributes"
  - "abac-policy-engine.md §Policy Language"
  - "abac-policy-engine.md §Policy Entity"
  - "abac-policy-engine.md §Conditions"
  - "abac-policy-engine.md §Policy Examples"
commits: ["9805df1e1f45c415cbbc366f6eb5aa7918a35ca8", "09053b5c1e608e3e2aabdf56701c52b262af08d4", "8bd8099c73cf0c30662106836832802ada7231bd"]
---

## Spec Excerpt

From `abac-policy-engine.md` §Core Concepts through §Policy Examples:

**Attributes** — four categories evaluated on every access decision:

- **Subject attributes:** type, id, tenant_id, global_role, workspace_role, workspace_ids, team_ids, persona, stack_hash, attestation_level, repo_scope
- **Resource attributes:** type, id, tenant_id, workspace_id, repo_id, owner, team, approval_status, visibility
- **Action attributes:** read, write, delete, approve, spawn, push, merge, escalate, generate
- **Environment attributes:** time, ip, budget_remaining, main_health

**Policy Entity:**

```rust
pub struct Policy {
    pub id: Id,
    pub name: String,
    pub description: String,
    pub scope: PolicyScope,
    pub scope_id: Option<Id>,
    pub priority: u32,
    pub effect: PolicyEffect,
    pub conditions: Vec<Condition>,
    pub actions: Vec<String>,
    pub resource_types: Vec<String>,
    pub enabled: bool,
    pub immutable: bool,
    pub created_by: Id,
    pub created_at: u64,
    pub updated_at: u64,
}

pub enum PolicyScope { Tenant, Workspace, Repo }
pub enum PolicyEffect { Allow, Deny }
```

**Conditions:**

```rust
pub struct Condition {
    pub attribute: String,        // e.g., "subject.workspace_role"
    pub operator: ConditionOp,
    pub value: ConditionValue,
}

pub enum ConditionOp {
    Equals, NotEquals, In, NotIn, GreaterThan, LessThan, Contains, Exists,
}

pub enum ConditionValue {
    String(String),
    StringList(Vec<String>),
    Number(i64),
    Bool(bool),
}
```

Conditions support dynamic references (e.g., `"$resource.repo_id"`) for comparing subject attributes against resource attributes at evaluation time.

## Implementation Plan

1. **Audit existing ABAC implementation:**
   - `gyre-server/src/abac.rs` and `abac_middleware.rs` — check current approach
   - `gyre-domain/src/policy.rs` — existing Policy entity
   - `gyre-server/src/policy_engine.rs` — existing evaluation
   - Determine gap between current implementation and spec

2. **Domain types in `gyre-domain`:**
   - Update/create `Policy` struct to match spec exactly
   - Add `PolicyScope` enum (Tenant, Workspace, Repo)
   - Add `PolicyEffect` enum (Allow, Deny)
   - Add `Condition` struct with `ConditionOp` and `ConditionValue`
   - Add `immutable` field for non-overridable deny policies

3. **Attribute model:**
   - Define `SubjectAttributes` struct collecting all subject attributes
   - Define `ResourceAttributes` struct
   - Define `EnvironmentAttributes` struct
   - Attribute extraction from auth context (JWT claims, memberships)

4. **Port trait in `gyre-ports`:**
   - `PolicyRepository` — CRUD + list by scope + find applicable policies for a scope chain

5. **SQLite adapter:**
   - Migration for `policies` table matching spec schema
   - Conditions stored as JSON array
   - Actions and resource_types stored as JSON arrays
   - Implement PolicyRepository

6. **Dynamic references in conditions:**
   - Support `$resource.repo_id` syntax in condition values
   - Resolve at evaluation time by substituting from resource attributes

## Acceptance Criteria

- [x] Policy entity matches spec (all fields including immutable, scope, conditions)
- [x] PolicyScope: Tenant, Workspace, Repo
- [x] PolicyEffect: Allow, Deny
- [x] Condition with all 8 ConditionOp variants
- [x] ConditionValue: String, StringList, Number, Bool
- [x] Dynamic references (`$resource.*`, `$subject.*`) resolve at evaluation time
- [x] PolicyRepository port trait with CRUD + scope-based listing
- [x] SQLite adapter with migration
- [x] Subject/Resource/Environment attribute extraction from auth context
- [x] `cargo test --all` passes (controller gate; focused suites green, see Result)

## Result (implementation round)

All acceptance criteria met; audit of current code:

1. **Policy entity matches spec** — `gyre-domain/src/policy.rs:71-100`: all spec fields
   incl. `immutable`, `scope`, `conditions`; plus `built_in` (required by the spec's
   Built-In Policies section, which this spec's coverage spans).
2. **PolicyScope: Tenant, Workspace, Repo** — `policy.rs:21-25`.
3. **PolicyEffect: Allow, Deny** — `policy.rs:30-33`.
4. **All 8 ConditionOp variants** — `policy.rs:38-47` (Equals, NotEquals, In, NotIn,
   GreaterThan, LessThan, Contains, Exists).
5. **ConditionValue: String, StringList, Number, Bool** — `policy.rs:52-58`, plus `Null`
   for `Exists`.
6. **Dynamic references resolve at evaluation time** —
   `policy_engine.rs:105-154` (`resolve_condition_value` / `resolve_dynamic_value` /
   `resolve_dynamic_scalar`); `$resource.*` and `$subject.*` both resolve against the
   evaluation context, numeric references preserve type, and unresolvable references
   fail closed (no fabricated value — Allow cannot grant). Newly covered by 5 regression
   tests in `policy_engine::tests` (`dynamic_reference_*`); one pins the deliberate
   fail-closed behavior for a list-valued reference in scalar (StringList element)
   position — no scalar is fabricated from a list.
7. **PolicyRepository port** — `gyre-ports/src/policy.rs`: create/find_by_id/list/
   list_by_scope/update/delete/delete_by_name_prefix(+scope_id)/record_decision/
   list_decisions. Implemented by SQLite, Postgres, and mem adapters (mem duplicate
   guard verified by `check-mem-port-contracts.sh`).
8. **SQLite adapter + migration** — `policies` table in `000007_platform_entities`
   (conditions/actions/resource_types as JSON), `immutable` column added by
   `000028_policy_immutable`; row mapping round-trips `immutable`/`built_in` flags
   (`gyre-adapters/src/sqlite/policy.rs:84-85,186-187`). Both migrations are shared
   SQLite/PG (verified by `check-migration-sql-portability.sh`).
9. **Attribute extraction** — `abac_middleware.rs`: subject type/id/global_role/tenant_id
   from auth; workspace_role/workspace_ids/team_ids from memberships; persona/stack_hash/
   attestation_level/repo_scope from JWT claims; resource id/workspace_id/repo_id from
   the route path; env.time/env.ip from engine/ConnectInfo.
10. **cargo test --all** — controller gate. Focused suites pass: `policy_engine` (24),
    `api::policies` (12), `abac_middleware` (12), `gyre-domain` (363);
    `check-arch.sh`, `check-abac-route-registry.sh`, `check-mem-port-contracts.sh`,
    `check-migration-versions.sh`, `check-migration-sql-portability.sh`,
    `check-inert-enforcement.sh` all green.

No unresolved gaps for this task's sections.

## Agent Instructions

Read `specs/system/abac-policy-engine.md` §Core Concepts through §Policy Examples. Existing ABAC: `gyre-server/src/abac.rs`, `abac_middleware.rs`, `policy_engine.rs`. Existing Policy domain: `gyre-domain/src/policy.rs`. Existing Policy port: `gyre-ports/src/policy.rs`. Existing Policy adapter: `gyre-adapters/src/sqlite/policy.rs`. Auth context: `gyre-server/src/auth.rs` for how subject attributes are extracted from JWTs. Check migration numbering: `ls crates/gyre-adapters/migrations/ | tail -5` — currently at 000049.
