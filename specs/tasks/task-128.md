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
commits: ["5b879846c9e653f8e006be9e57029ad0123d6ce5", "29ad56fb6bb5bb7a27302272753228cb6c902de2", "9e06a9d3dac428631eab2b69c13bdc5719eba0ac", "35f090dfac66db4afa8c4e2df562ad12667a521a", "19883086799ffc176645bed2f3831144f2701d05", "16e740051d3f7e5a54c4b3c0c11d8d8b10034e8c", "7502a9eec991513e7e31d51b12fa00a3e6bf77e1", "5327b8daaa10dc4f9768b2be6498c00c36602ab4", "8d5494e942495e37544806afdec8f1b93009c08b", "d779f1e8912e3711d35d9b1353ef72e5a3390eb7"]
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

Rebase repair: resolved the merge of the published task-128 head
(bf6a1f6e) onto current main (44a01390c, task-171 LLM Endpoint
Contract) without regressing either side. The retained task-128
implementation (policy entity/conditions/attributes, dynamic
references, PolicyRepository port + adapters, middleware attribute
extraction, and its test suite) is unchanged; only the two conflicted
files were resolved.

### Conflict resolution

- `crates/gyre-server/src/abac_middleware.rs`: both sides added content
  at the same anchors (task-128's attribute/evaluation tests after
  `builtin_require_human_spec_approval_seeded`; task-171's
  `developer-generate-access` builtin policy and
  `developer_jwt_can_generate_on_llm_endpoints` regression test).
  Resolution keeps BOTH: HEAD's task-128 test block verbatim, plus
  main's builtin `developer-generate-access` Allow policy (priority
  800, `generate` action on explorer_view/spec/workspace) inserted
  before `builtin-readonly-get-only`, and main's regression test
  inserted after `builtin_require_human_spec_approval_seeded`. Diff vs
  the pre-merge head is exactly main's two additions (155 lines), so
  no task-128 behavior changed.
- `specs/coverage/SUMMARY.md`: regenerated with
  `scripts/dev-coverage.py` from the merged coverage matrices (the
  repo's canonical generator), fixing a stale TOTAL row: rows now sum
  consistently (802/220/0/240/179/163, 58%).

### Verification evidence

- `cargo check -p gyre-server --tests` — clean (covers the edited test
  module, the merged auth/mcp/mem/spawn/policy_engine sources, and the
  integration tests).
- `cargo check -p gyre-cli --tests` — clean.
- `scripts/check-migration-versions.sh`, `check-arch.sh`,
  `check-abac-route-registry.sh`, `check-abac-exempt-handlers.sh`,
  `check-mcp-write-tools.sh`, `check-fabricated-scope-defaults.sh`,
  `check-forwarded-header-trust.sh`, `check-inert-enforcement.sh` —
  all OK on the merged tree.

### Verifier must run (deferred: gyre-server test compilation exceeds
the 4 GiB in-sandbox budget)

- `cargo test -p gyre-server --lib abac` — the merged ABAC suite,
  including `developer_jwt_can_generate_on_llm_endpoints` (task-171
  regression proving the builtin generate Allow seeds and evaluates
  through the task-128 middleware) and the task-128 attribute tests
  (`membership_*`, `agent_jwt_*`, `env_budget_remaining_*`,
  `env_main_health_*`, `spec_*`, `persona_entity_*`).
- `cargo test --all` per the task's acceptance criterion.
