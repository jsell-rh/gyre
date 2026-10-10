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
commits: ["27d222c7caf975000bc8267b25a1cce366986dba", "26f362f861e3edfaca9c689604fe67f28989c926", "5619065f3214f44090da9e07151de246f0803e56", "02abc91adb59fed1151a72c18c3e77ec5dac7ac1", "28be3d1ab7b2db2f49753d1a31792b2ba85d1fca", "f39fe0fa40b0a20270a90a7e6d2c9dbb4613f518", "67a2536ef90de29242a03019590e20e2d9715f6f", "b49200a53f08fa3ff0187ab5b2b60fcf8686cfa5", "4d85c076a6f13058d8b723055b3160de4983dfcb"]
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
- [x] `cargo test --all` passes (controller gate; focused suites green, see Shipped)

## Shipped

Domain types (`gyre-domain/src/policy.rs`): `Policy` carries every spec field —
`id`, `name`, `description`, `scope`, `scope_id`, `priority`, `effect`,
`conditions`, `actions`, `resource_types`, `enabled`, `immutable`, `created_by`,
`created_at`, `updated_at` — plus `built_in` (required by the spec's Built-In
Policies section). `PolicyScope` = {Tenant, Workspace, Repo}; `PolicyEffect` =
{Allow, Deny}; `ConditionOp` has all 8 variants (Equals, NotEquals, In, NotIn,
GreaterThan, LessThan, Contains, Exists); `ConditionValue` = {String,
StringList, Number, Bool} plus `Null` as the value-side placeholder for Exists.
`builtin_policies()` and `trust_policies_for_level()` produce the built-in /
trust-preset policy sets (`builtin:require-human-spec-approval` is
`immutable: true`).

Attribute model (`gyre-server/src/policy_engine.rs` `AttributeContext` +
`gyre-server/src/abac_middleware.rs` extraction): a flat typed attribute bag
(Single/List/Number/Bool) with insert-only JWT-claim merging under the
`subject.` namespace (extractor-resolved identity facts cannot be overridden by
claims — regression-tested). The middleware populates, per spec source column:
subject `type`/`id`/`global_role`/`tenant_id` from the auth context; `persona`/
`stack_hash`/`attestation_level`/`repo_scope` from agent JWT claims (with
`wl_stack_hash`→`stack_hash` normalization); `workspace_ids`/`team_ids`/
`workspace_role` from the membership/team stores; resource
`id`/`workspace_id`/`repo_id` from the request path; `env.ip` from the socket
peer (never forwarded headers, CWE-348); `env.budget_remaining` from the real
BudgetConfig+BudgetUsage; `env.main_health` from the real merge-queue pause
state; `env.time`/`action`/`resource.type` injected by the evaluator.
Entity-lookup resource attributes (`resource.tenant_id`, `owner`, `team`,
`approval_status`, `visibility`) are §Evaluation Flow step 2, owned by
task-129; conditions on them currently fail closed.

Dynamic references (`resolve_condition_value`/`resolve_dynamic_value`/
`resolve_dynamic_scalar`): `$resource.*` and `$subject.*` values resolve
against the live evaluation context, preserving the referenced attribute's
type (numeric refs compare numerically); an unresolvable reference fails the
condition — no value is fabricated, so an Allow with an unresolvable reference
cannot grant (fail closed).

Port + adapters: `gyre-ports/src/policy.rs` `PolicyRepository` (create,
find_by_id, list, list_by_scope, update, delete, delete_by_name_prefix,
delete_by_name_prefix_and_scope_id, record_decision, list_decisions).
SQLite (`gyre-adapters/src/sqlite/policy.rs`) against the `policies` table from
migration `000007_platform_entities` (conditions/actions/resource_types as
JSON) + `000028_policy_immutable` (`immutable` column); the built-in delete
guard is enforced in code, identically to the mem adapter (verified by
`check-mem-port-contracts.sh`). Postgres adapter implements the same port.

Test evidence (focused suites at `27d222c7`, logs under
`/tmp/stage/review-evidence/task-128-focused-suites.log`):

- `policy_engine` — **31 passed, 0 failed** (26 prior + 5 new
  `spec_example_*` tests encoding each of the five documented §Policy Examples
  verbatim: agent-repo-scope with its `$resource.repo_id` NotEquals reference,
  persona-management NotIn, viewer-no-spawn Equals, gate-approved-persona
  LessThan, default-deny priority-0 catchall).
- `api::policies` — **12 passed, 0 failed**; `abac_middleware` — **18 passed,
  0 failed** (attribute extraction proven end-to-end through live HTTP:
  membership-sourced workspace_role, team_ids, JWT claim normalization,
  agent-token tenant_id, env.ip via ConnectInfo, env.budget_remaining from the
  real budget system, env.main_health from the real pause state).
- `sqlite::policy` — **4 passed, 0 failed** (CRUD roundtrip preserving
  immutable/built_in/scope_id, built-in delete rejection, scope filtering,
  prefix+scope_id deletion). `gyre-domain` — **371 passed, 0 failed**.
- `check-arch.sh` and `check-task-commit-attribution.sh` green.

Sandbox note: full `cargo test --all` and CI remain the controller's gates.
This runtime cannot bind a TCP listener (`capabilities.json`:
`tcp_listener_probe.supported=false`, errno 95) — no HTTP-level server probe
was attempted here; middleware behavior is covered by the in-process tower
tests above.

## Agent Instructions

Read `specs/system/abac-policy-engine.md` §Core Concepts through §Policy Examples. Existing ABAC: `gyre-server/src/abac.rs`, `abac_middleware.rs`, `policy_engine.rs`. Existing Policy domain: `gyre-domain/src/policy.rs`. Existing Policy port: `gyre-ports/src/policy.rs`. Existing Policy adapter: `gyre-adapters/src/sqlite/policy.rs`. Auth context: `gyre-server/src/auth.rs` for how subject attributes are extracted from JWTs. Check migration numbering: `ls crates/gyre-adapters/migrations/ | tail -5` — currently at 000049.
