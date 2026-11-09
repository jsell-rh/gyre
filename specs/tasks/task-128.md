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
commits: ["d779f1e8912e3711d35d9b1353ef72e5a3390eb7", "29ad56fb6bb5bb7a27302272753228cb6c902de2"]
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

Domain types (`gyre-domain/src/policy.rs`): `Policy` carries every spec
field (`id`, `name`, `description`, `scope`, `scope_id: Option<Id>`,
`priority`, `effect`, `conditions`, `actions`, `resource_types`,
`enabled`, `immutable`, `created_by: Id`, `created_at`, `updated_at`) plus
`built_in` (required by the spec's Built-In Policies section).
`PolicyScope` = {Tenant, Workspace, Repo}; `PolicyEffect` = {Allow, Deny};
`ConditionOp` has all 8 variants; `ConditionValue` = {String, StringList,
Number, Bool} plus `Null` as the value-side placeholder for `Exists`.
`builtin_policies()` and `trust_policies_for_level()` produce the
built-in/trust-preset policy sets.

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
injected by the evaluator.

Dynamic references: `$resource.*`/`$subject.*` resolve against the live
context preserving the referenced attribute's type; unresolvable references
fail the condition (fail closed — an Allow with an unresolvable reference
cannot grant). Mutation-verified: a fail-open resolver fabricating `""` for
missing refs is killed by the `subject.type NotEquals $resource.missing`
case in `dynamic_reference_unresolvable_fails_closed`.

Port + adapters: `PolicyRepository` (create, find_by_id, list, list_by_scope,
update, delete, delete_by_name_prefix, delete_by_name_prefix_and_scope_id,
record_decision, list_decisions). SQLite adapter against the `policies`
table (conditions/actions/resource_types as JSON); built-in delete guard
enforced in code in both SQLite and mem adapters.

Evaluation (`policy_engine::evaluate`): immutable Deny first, then priority
order with scope specificity (repo > workspace > tenant) and
Deny-before-Allow tie-break, then default deny. All five §Policy Examples
are encoded as verbatim evaluation tests (`spec_example_agent_repo_scope`
incl. its `$resource.repo_id` reference, `spec_example_persona_management`,
`spec_example_viewer_no_spawn`, `spec_example_gate_approved_persona`,
`spec_example_default_deny`).

### Review-repair round (commit d779f1e89)

Repaired the three round-1 review findings:

- **Code** — `extract_path_attributes` set `resource.id` to the PARENT REPO
  id on `/repos/:id/...` routes with non-repo resource types (gates,
  worktrees, abac-policy, dependencies, graph, specs): the repos prefix
  lacked the `resource_type != "repo"` guard the workspace branch has, and
  `/repos/:id/worktrees/:worktree_id` never surfaced the worktree id. Fix:
  a path param named `{resource_type}_id` (`:worktree_id`, `:gate_id`,
  `:dependency_id`, `:team_id`) names the addressed entity wherever it
  appears and takes precedence over `:id`; on parent-scoped child routes
  the `:id` names the parent (already exposed as `resource.repo_id`) and no
  `resource.id` is fabricated from it. Also corrects the
  merge-request-parented dependency child route the same way. Route
  patterns hoisted into consts (`REPOS_GATES_ROUTE`, `REPOS_GATES_ID_ROUTE`,
  `REPOS_WORKTREES_ID_ROUTE`) so the frozen duplicate-literal count stays at
  17.
- **Test** — `dynamic_reference_unresolvable_fails_closed` gained the
  demanded killing case (`subject.type NotEquals $resource.missing` on an
  Allow policy must Deny — a fabricated `""` resolution would flip
  NotEquals to true and ALLOW).
- **Process** — coverage rows 2-7 of
  `specs/coverage/system/abac-policy-engine.md` flipped to `implemented`
  with evidence notes; SUMMARY.md regenerated.

### Test evidence (run at head d779f1e89)

- Mutation probes (transcripts under `/tmp/stage/review-evidence/`):
  removing the repos-prefix guard fails 2 of the 3 new middleware tests;
  removing child-param precedence as well (full round-1 behavior) fails all
  3; the fail-open `resolve_dynamic_value` mutant is killed by the new
  policy-engine killing case (previously survived the entire suite).
- `cargo test -p gyre-server --lib` — **1481 passed / 0 failed** (includes
  `abac_middleware` 28, `policy_engine` 34; the five §Policy Example tests,
  the membership/entity-lookup attribute tests, and the three new
  round-1 tests run end-to-end through live tower oneshot requests with
  non-vacuous controls).
- `cargo test -p gyre-domain -p gyre-common -p gyre-ports -p gyre-adapters
  --lib` — 116 + 436 + 8 + 387 passed / 0 failed / 12 ignored.
- All repo invariant scripts pass at this head, including
  `check-abac-route-registry` (duplicate count 17 = frozen baseline),
  `check-arch`, `check-task-commit-attribution`, `check-mem-port-contracts`,
  and the security-pattern checks; repo-wide conflict-marker scan clean
  (the single grep hit is a quoted command in a task doc).

Full `cargo test --all` (integration/e2e binaries) and GitHub CI remain
verification/publication gates.
