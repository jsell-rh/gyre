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
are `implemented` with per-row evidence notes.

### Behavior

- **Domain types** (`gyre-domain/src/policy.rs`): `Policy` carries every spec
  field (`id`, `name`, `description`, `scope`, `scope_id: Option<Id>`,
  `priority`, `effect`, `conditions`, `actions`, `resource_types`,
  `enabled`, `immutable`, `created_by: Id`, `created_at`, `updated_at`) plus
  `built_in` (required by §Built-In Policies). `PolicyScope` = {Tenant,
  Workspace, Repo}; `PolicyEffect` = {Allow, Deny}; `ConditionOp` has all 8
  variants; `ConditionValue` = {String, StringList, Number, Bool} plus `Null`
  as the value-side placeholder for `Exists`. `builtin_policies()` seeds the
  fail-closed defaults.
- **Attribute model** (`policy_engine.rs` `AttributeContext` +
  `abac_middleware.rs` extraction): a typed attribute bag covering all four
  spec categories. Subject facts come from the auth context and the
  membership/team stores and are never claim-definable: JWT-claim merging
  into the live context is allowlisted to the four agent-claim names
  (persona, stack_hash, attestation_level, repo_scope —
  `merge_agent_jwt_claims`) and gated on `subject.type == agent`. Resource
  identity from the request path (parent-scoped child routes never leak the
  parent id into `resource.id`; `:path`-addressed spec routes populate the
  decoded registry path), entity-lookup attributes (tenant_id, owner,
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
  (conditions/actions/resource_types as JSON) plus migration; the mem-side
  wiring enforces the same contracts in code (built-in delete guard in
  both adapters).
- **Evaluation** (`policy_engine::evaluate`): immutable Deny first, then
  priority order with scope specificity (repo > workspace > tenant) and
  Deny-before-Allow tie-break, then default deny. All five §Policy Examples
  are encoded as verbatim evaluation tests
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

### Prior rounds (context)

Rebase merges of successive assignment bases (task-118 spawn refactor,
cba416b46) re-applied the task-128 worker JWT scoping (`mint_worker_scoped`)
on top of the base's shared `launch_agent_process`; a SUMMARY.md
bookkeeping regression from one wholesale-take resolution was caught by
review and fixed with the mechanical regenerator. Candidate 7c41905 was
independently approved (record 74488cd42), then reconciled with main at
91cff7e6f.

### Rebase repair (merge db858a534, this round)

The published PR again conflicted with its target: main advanced to
7e9b3cfee (task-176 Meta-specs Preview Loop, plus task-163 dependency-graph
storage underneath). Merged 7e9b3cfee into the branch. Only conflict:
`specs/coverage/SUMMARY.md` TOTAL row — resolved by mechanical regeneration
(`scripts/update-coverage-summary.sh`), which correctly combines both sides
per-file rows (task-176's ui-layout row 37 → implemented; task-128's six
abac-policy-engine implemented rows retained; TOTAL 802/220/0/248/171/163,
57%). All other paths auto-merged: task-176's source (`abac_middleware.rs`
route mapping, `api/mod.rs` routes, `specs_assist.rs`, `llm_defaults.rs`,
web/src) and its verified `web/dist` assets came in from main unchanged; the
task-128 ABAC implementation is untouched. No production code changed this
round beyond the merge itself.

Probes at head db858a534 (evidence: /tmp/stage/review-evidence/):
`cargo check -p gyre-server` clean (7m03s); `cargo test -p gyre-domain`
466/466 passed; `cargo test -p gyre-adapters --lib policy` 13/13 passed
(CRUD roundtrip, built-in delete guard, scope filtering, plus task-163's
policy-adjacent storage tests); `check-abac-route-registry.sh`,
`check-arch.sh`, `check-migration-versions.sh`, `check-mem-port-contracts.sh`,
`check-inert-enforcement.sh`, and `check-task-commit-attribution.sh` all
pass (the last with the restored commits list). Per the build-budget rule
the server test binaries run in the independent verifier — the relevant
suites are the `policy_engine` lib tests (8 operators, 4 value types,
dynamic-reference fail-closed mutation-kill cases, all five §Policy
Examples), the `abac_middleware` lib tests (live attribute extraction:
`agent_jwt_claims_reach_live_evaluation`, `persona_attestation_*`,
`review_scoped_token_*`/`validation_scoped_token_*`, env attribute
sources, spec-path resource.id), `tests/task134_review_probe.rs`, and the
`auth` mint_worker_scoped round-trip tests.
