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
commits: ["50051631d901e40e5df3e39dec56c0969c8467ac", "6d0e4a39302b90c9e192f9d1e58555106ce3e1e9", "27d222c7caf975000bc8267b25a1cce366986dba", "26f362f861e3edfaca9c689604fe67f28989c926", "5619065f3214f44090da9e07151de246f0803e56", "02abc91adb59fed1151a72c18c3e77ec5dac7ac1", "28be3d1ab7b2db2f49753d1a31792b2ba85d1fca", "f39fe0fa40b0a20270a90a7e6d2c9dbb4613f518", "67a2536ef90de29242a03019590e20e2d9715f6f", "b49200a53f08fa3ff0187ab5b2b60fcf8686cfa5", "4d85c076a6f13058d8b723055b3160de4983dfcb", "505443214b5e31e5d900e98cc010114b82bccce6"]
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

- [ ] Policy entity matches spec (all fields including immutable, scope, conditions)
- [ ] PolicyScope: Tenant, Workspace, Repo
- [ ] PolicyEffect: Allow, Deny
- [ ] Condition with all 8 ConditionOp variants
- [ ] ConditionValue: String, StringList, Number, Bool
- [ ] Dynamic references (`$resource.*`, `$subject.*`) resolve at evaluation time
- [ ] PolicyRepository port trait with CRUD + scope-based listing
- [ ] SQLite adapter with migration
- [ ] Subject/Resource/Environment attribute extraction from auth context
- [ ] `cargo test --all` passes

## Agent Instructions

Read `specs/system/abac-policy-engine.md` §Core Concepts through §Policy Examples. Existing ABAC: `gyre-server/src/abac.rs`, `abac_middleware.rs`, `policy_engine.rs`. Existing Policy domain: `gyre-domain/src/policy.rs`. Existing Policy port: `gyre-ports/src/policy.rs`. Existing Policy adapter: `gyre-adapters/src/sqlite/policy.rs`. Auth context: `gyre-server/src/auth.rs` for how subject attributes are extracted from JWTs. Check migration numbering: `ls crates/gyre-adapters/migrations/ | tail -5` — currently at 000049.

## Shipped

Domain types (`gyre-domain/src/policy.rs`): `Policy` carries every spec field
(`id`, `name`, `description`, `scope`, `scope_id: Option<Id>`, `priority`,
`effect`, `conditions`, `actions`, `resource_types`, `enabled`, `immutable`,
`created_by: Id`, `created_at`, `updated_at`) plus `built_in` (required by the
spec's Built-In Policies section). `PolicyScope` = {Tenant, Workspace, Repo};
`PolicyEffect` = {Allow, Deny}; `ConditionOp` has all 8 variants; 
`ConditionValue` = {String, StringList, Number, Bool} plus `Null` as the
value-side placeholder for `Exists`. `builtin_policies()` and
`trust_policies_for_level()` produce the built-in/trust-preset policy sets.

Attribute model (`policy_engine.rs` `AttributeContext` + `abac_middleware.rs`
extraction): a flat typed attribute bag (Single/List/Number/Bool) with
insert-only JWT-claim merging under `subject.` (claims cannot override
extractor-resolved identity facts). Per spec source column: subject
`type`/`id`/`global_role`/`tenant_id` from the auth context; `persona`/
`stack_hash`/`attestation_level`/`repo_scope` from agent JWT claims
(`wl_stack_hash`→`stack_hash` normalization); `workspace_ids`/`team_ids`/
`workspace_role` from the membership/team stores; resource
`id`/`workspace_id`/`repo_id` from the request path; entity-lookup resource
attributes (`resource.tenant_id`, `owner`, `team`, `approval_status`,
`visibility`) from real store lookups per resource type (spec ledger,
personas, tasks, agents, MRs, teams; scope chain repo→workspace→tenant);
`env.ip` from the socket peer (CWE-348, never forwarded headers);
`env.budget_remaining` from the real budget system; `env.main_health` from
the real merge-queue pause state; `env.time`/`action`/`resource.type`
injected by the evaluator. `resource.visibility`/`resource.team` apply only
to entity types carrying those fields (none today — documented at the
extraction site; conditions on them fail closed).

Dynamic references (`resolve_condition_value`/`resolve_dynamic_value`/
`resolve_dynamic_scalar`): `$resource.*`/`$subject.*` resolve against the
live context preserving the referenced attribute's type; unresolvable
references fail the condition (fail closed — an Allow with an unresolvable
reference cannot grant).

Port + adapters: `PolicyRepository` (create, find_by_id, list, list_by_scope,
update, delete, delete_by_name_prefix, delete_by_name_prefix_and_scope_id,
record_decision, list_decisions). SQLite adapter against the `policies`
table (migration `000007_platform_entities` + `000028_policy_immutable`;
conditions/actions/resource_types as JSON); built-in delete guard enforced
in code in both SQLite and mem adapters. `scope_id`/`created_by` stored as
text, carried as `Id` in the domain.

Evaluation (`policy_engine::evaluate`): immutable Deny first (before all
priority-based evaluation), then priority order with scope specificity
(repo > workspace > tenant) and Deny-before-Allow tie-break, then default
deny. All five §Policy Examples are encoded as verbatim evaluation tests
(`spec_example_agent_repo_scope` incl. its `$resource.repo_id` reference,
`spec_example_persona_management`, `spec_example_viewer_no_spawn`,
`spec_example_gate_approved_persona`, `spec_example_default_deny`).

Test evidence (focused suites at this head, logs under
`/tmp/stage/review-evidence/task-128-resume-verification.txt`):
`policy_engine::tests` 31/31; `abac_middleware::tests` 23/23 (attribute
extraction proven end-to-end through live tower oneshot requests:
membership-sourced workspace_role, team_ids, entity-lookup
owner/approval_status/tenant via scope chain, persona attributes,
env.ip/budget_remaining/main_health); `auth::tests` 36/36;
`api::policies` 12/12; `sqlite::policy` 4/4.

Resume note: the resumed run found one failing test in the checkpoint —
`spec_resource_tenant_resolves_via_scope_chain` asserted 403 but got 200.
Root cause was a test-premise bug, not implementation: the policy was a
same-tenant Allow at priority 810; since `member_jwt()` carries no
`tenant_id` claim, the condition fails and the builtin developer Allow at
800 grants regardless of the resource tenant — so a non-matching Allow can
never discriminate tenant fabrication. Fixed by converting the test to a
cross-tenant Deny at 810 (`resource.tenant_id NotEquals $subject.tenant_id`):
only a real scope-chain lookup (tenant t-1 ≠ "default") makes the deny fire;
a fabricated or missing tenant falls through to the builtin Allow → 200.

Full `cargo test --all` and GitHub CI remain verification/publication gates.
This sandbox cannot bind a TCP listener (`capabilities.json`
`tcp_listener_probe.supported=false`, errno 95) — no live HTTP server probe;
middleware behavior is covered by the in-process tower tests above.
