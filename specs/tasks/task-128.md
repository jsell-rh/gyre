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
commits: ["35f090dfac66db4afa8c4e2df562ad12667a521a", "19883086799ffc176645bed2f3831144f2701d05", "16e740051d3f7e5a54c4b3c0c11f8d8b10034e8c", "7502a9eec991513e7e31d51b12fa00a3e6bf77e1", "5327b8daaa10dc4f9768b2be6498c00c36602ab4", "8d5494e942495e37544806afdec8f1b93009c08b", "d779f1e8912e3711d35d9b1353ef72e5a3390eb7", "29ad56fb6bb5bb7a27302272753228cb6c902de2", "9e06a9d3dac428631eab2b69c13bdc5719eba0ac"]
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

ABAC policy engine core per `abac-policy-engine.md` §Core Concepts–§Policy
Examples; coverage rows 2–7 of `specs/coverage/system/abac-policy-engine.md`
are `implemented` with per-row evidence notes. This round repairs the merge
onto base 199c1bb78 (task-203 search access scoping + delivery-readiness)
without regressing either side.

### Behavior (retained implementation)

- **Domain types** (`gyre-domain/src/policy.rs`): `Policy` carries every spec
  field (`id`, `name`, `description`, `scope`, `scope_id: Option<Id>`,
  `priority`, `effect`, `conditions`, `actions`, `resource_types`,
  `enabled`, `immutable`, `created_by: Id`, `created_at`, `updated_at`) plus
  `built_in` (required by §Built-In Policies). `PolicyScope` = {Tenant,
  Workspace, Repo}; `PolicyEffect` = {Allow, Deny}; `ConditionOp` has all 8
  variants; `ConditionValue` = {String, StringList, Number, Bool} plus `Null`
  as the value-side placeholder for `Exists`.
- **Attribute model** (`policy_engine.rs` `AttributeContext` +
  `abac_middleware.rs` extraction): typed attribute bag covering all four
  spec categories. Subject facts come from the auth context and the
  membership/team stores and are never claim-definable (JWT-claim merging is
  allowlisted to the four agent-claim names and gated on
  `subject.type == agent`). Resource identity from the request path,
  entity-lookup attributes from real store lookups; env attributes from
  real systems (budget, forge pause state, socket peer per CWE-348, clock).
- **Dynamic references**: `$resource.*`/`$subject.*` resolve at evaluation
  time preserving the referenced attribute's type; unresolvable references
  fail the condition (fail closed).
- **PolicyRepository port** (CRUD + list_by_scope + decision audit) with
  SQLite adapter (`policies` table; conditions/actions/resource_types as
  JSON) and mem adapter; built-in delete guard enforced in code in both.
- **Scope-chain backfill** (prior rebase repair, retained): repo-addressed
  routes complete `resource.workspace_id`/`resource.tenant_id` from the
  addressed repo's own workspace (`set_workspace_and_tenant_from_repo`),
  fail closed on unresolvable repo ids, so repo `admin` policies gated on
  `subject.workspace_role` can match for workspace Admins.

### This round (merge repair onto 199c1bb78, commit 9e06a9d3d)

The merge kept this branch's 8-arg `mint_orchestrator` (which adds the
`persona` claim) while taking the base's task-203 search test call sites
(7 args), breaking compilation of `gyre-server` tests, plus one rustfmt
collapse in `abac_middleware.rs`:

- `crates/gyre-server/tests/search_wiring.rs` (2 sites) and
  `crates/gyre-server/src/mcp.rs` lib tests (2 sites): pass `persona` using
  the production mapping (`orchestrator_persona` in `api/orchestrator.rs`):
  `repo_orchestrator` → `repo-orchestrator`, `workspace_orchestrator` →
  `workspace-orchestrator`. Search scoping keys on `workspace_id`/`repo_id`
  claims; persona only populates `subject.persona`, so test semantics are
  unchanged.
- `abac_middleware.rs`: rustfmt-only collapse of the
  `state.repos.find_by_id(...)` match head; no semantic change.

### Test evidence

- `cargo check -p gyre-server --tests` (build wrapper, SKIP_WEB_BUILD=1):
  exit 0 — covers both previously failing test targets.
- `rustfmt --check` on all 15 `.rs` files changed vs base 199c1bb78:
  exit 0.
- Server test execution is deferred to the independent verifier (4 GiB
  budget; evidence in `/tmp/stage/review-evidence/task-128-merge-repair.md`).
  Required probes:
  - `cargo test -p gyre-server --test search_wiring`
  - `cargo test -p gyre-server --lib mcp_search_is_access_scoped_to_the_agent_token`
  - `cargo test -p gyre-server --lib abac_middleware`
  - `cargo test -p gyre-server --lib policy_engine`
  - `cargo test -p gyre-server --lib api::repo_access`
  - `cargo test -p gyre-server --lib git_http`
