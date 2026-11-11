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
commits: ["16e740051d3f7e5a54c4b3c0c11f8d8b10034e8c", "7502a9eec991513e7e31d51b12fa00a3e6bf77e1", "5327b8daaa10dc4f9768b2be6498c00c36602ab4", "8d5494e942495e37544806afdec8f1b93009c08b", "d779f1e8912e3711d35d9b1353ef72e5a3390eb7", "29ad56fb6bb5bb7a27302272753228cb6c902de2"]
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

## Agent Instructions

Read `specs/system/abac-policy-engine.md` §Core Concepts through §Policy Examples. Existing ABAC: `gyre-server/src/abac.rs`, `abac_middleware.rs`, `policy_engine.rs`. Existing Policy domain: `gyre-domain/src/policy.rs`. Existing Policy port: `gyre-ports/src/policy.rs`. Existing Policy adapter: `gyre-adapters/src/sqlite/policy.rs`. Auth context: `gyre-server/src/auth.rs` for how subject attributes are extracted from JWTs. Check migration numbering: `ls crates/gyre-adapters/migrations/ | tail -5` — currently at 000049.

## Shipped

ABAC policy engine core per `abac-policy-engine.md` §Core Concepts–§Policy
Examples; coverage rows 2–7 of `specs/coverage/system/abac-policy-engine.md`
are `implemented` with per-row evidence notes.

### Behavior

- **Domain types** (`gyre-domain/src/policy.rs`): `Policy` carries every
  spec field (`id`, `name`, `description`, `scope`,
  `scope_id: Option<Id>`, `priority`, `effect`, `conditions`, `actions`,
  `resource_types`, `enabled`, `immutable`, `created_by: Id`, `created_at`,
  `updated_at`) plus `built_in` (required by §Built-In Policies).
  `PolicyScope` = {Tenant, Workspace, Repo}; `PolicyEffect` = {Allow,
  Deny}; `ConditionOp` has all 8 variants; `ConditionValue` = {String,
  StringList, Number, Bool} plus `Null` as the value-side placeholder for
  `Exists`. `builtin_policies()` seeds the fail-closed defaults.
- **Attribute model** (`policy_engine.rs` `AttributeContext` +
  `abac_middleware.rs` extraction): a typed attribute bag covering all
  four spec categories. Subject facts come from the auth context and the
  membership/team stores and are never claim-definable: JWT-claim merging
  into the live context is allowlisted to the four agent-claim names
  (persona, stack_hash, attestation_level, repo_scope —
  `merge_agent_jwt_claims`) and gated on `subject.type == agent`. Resource
  identity from the request path (parent-scoped child routes never leak
  the parent id into `resource.id`; `:path`-addressed spec routes populate
  the decoded registry path), entity-lookup attributes (tenant_id, owner,
  approval_status) from real store lookups; `resource.team`/`visibility`
  remain unpopulated until an entity carries those fields (conditions on
  them fail closed). Env: `env.ip` from the socket peer (CWE-348),
  `env.budget_remaining` from the real budget system, `env.main_health`
  from the real merge-queue pause state, `env.time` from the clock.
- **Dynamic references**: `$resource.*`/`$subject.*` resolve at evaluation
  time preserving the referenced attribute's type; unresolvable references
  fail the condition (fail closed — mutation-verified by the
  `subject.type NotEquals $resource.missing` case in
  `dynamic_reference_unresolvable_fails_closed`).
- **Port + adapters**: `PolicyRepository` (CRUD, `list_by_scope`, decision
  audit). SQLite adapter against the `policies` table
  (conditions/actions/resource_types as JSON) plus migration; mem adapter
  enforces the same contracts in code (built-in delete guard in both).
- **Evaluation** (`policy_engine::evaluate`): immutable Deny first, then
  priority order with scope specificity (repo > workspace > tenant) and
  Deny-before-Allow tie-break, then default deny. All five §Policy
  Examples are encoded as verbatim evaluation tests
  (`spec_example_agent_repo_scope` incl. its `$resource.repo_id`
  reference, `spec_example_persona_management`,
  `spec_example_viewer_no_spawn`, `spec_example_gate_approved_persona`,
  `spec_example_default_deny`).
- **Worker JWT scoping** (`auth.rs` `mint_worker_scoped`): spawned workers
  carry `workspace_id`/`repo_id`/`persona`/`attestation_level`/
  `wl_image_hash` claims so agent subject attributes are real on every
  request; pre-minted by `api/spawn.rs`.

Review-repair rounds 1–4 (commits d779f1e8, 8d5494e9, 5327b8da, 7502a9ee,
16e7400) fixed parent-id leakage into `resource.id`, JWT claim spoofing of
membership-sourced attributes, missing `resource.id` on `:path` spec
routes, formatting/lint findings, and parent MR/agent id leakage on
merge-requests/agents child routes — each with killing tests named in the
commit messages and per-row evidence in the coverage file.

### Rebase repair (this round, merge c8d076fe7)

Merged new base cfeefbd (task-091 HSI, incl. the task-134 gate-agent
route allow-list) into the branch; no behavior change to either side:

- `abac_middleware.rs`: the two sides' shared route-pattern consts are
  unified — each `/api/v1/...` literal is defined exactly once (frozen
  duplicate count unchanged; `check-abac-route-registry.sh` passes) — and
  the reviewer/validator gate-agent allow-lists are retained verbatim.
- `auth.rs`: both scoped-mint paths survive as distinct functions — the
  gate-agent `mint_scoped(scope)` from the base (used by
  `gate_executor.rs` and `tests/task134_review_probe.rs`) and the worker
  repo-scope mint (renamed `mint_worker_scoped`; `api/spawn.rs` and the
  middleware tests updated) — with the gate-agent mint now initializing
  the `persona`/`attestation_level` claim fields task-128 added to
  `AgentJwtClaims`.

Probes at this head: `cargo check --workspace` clean; `cargo check -p
gyre-server --all-targets` clean; `scripts/check-arch.sh` and
`scripts/check-abac-route-registry.sh` pass. Per the build-budget rule the
server test binaries run in the independent verifier — the relevant suites
are the `abac_middleware` tests (worker repo-scope claims:
`agent_jwt_claims_reach_live_evaluation`, persona/attestation:
`persona_attestation_*`, gate-agent allow-list:
`review_scoped_token_*`/`validation_scoped_token_*`) and
`tests/task134_review_probe.rs`.
