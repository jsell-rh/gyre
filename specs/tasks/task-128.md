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
commits: ["19883086799ffc176645bed2f3831144f2701d05", "16e740051d3f7e5a54c4b3c0c11f8d8b10034e8c", "7502a9eec991513e7e31d51b12fa00a3e6bf77e1", "5327b8daaa10dc4f9768b2be6498c00c36602ab4", "8d5494e942495e37544806afdec8f1b93009c08b", "d779f1e8912e3711d35d9b1353ef72e5a3390eb7", "29ad56fb6bb5bb7a27302272753228cb6c902de2"]
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
are `implemented` with per-row evidence notes. This round repairs the latest
review finding on the retained implementation (round 5: no-target spawn
attestation).

### Behavior

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
  allowlisted to the four agent-claim names — persona, stack_hash,
  attestation_level, repo_scope — and gated on `subject.type == agent`).
  Resource identity from the request path, entity-lookup attributes from
  real store lookups; env attributes from real systems (budget, forge pause
  state, socket peer per CWE-348, clock).
- **Dynamic references**: `$resource.*`/`$subject.*` resolve at evaluation
  time preserving the referenced attribute's type; unresolvable references
  fail the condition (fail closed).
- **PolicyRepository port** (CRUD + list_by_scope + decision audit) with
  SQLite adapter (`policies` table; conditions/actions/resource_types as
  JSON) and mem adapter; built-in delete guard enforced in code in both.
- **This round (review repair, commit 198830867)**: `POST
  /api/v1/agents/spawn` with no compute target resolved — the default
  local-subprocess path — minted a worker JWT with no `attestation_level`
  claim (the `Option::map` on the resolved target dropped the no-target
  case to `None`), contradicting the adjacent comment and
  identity-security.md §Task-Scoped JWT Claims (every spawn-minted JWT
  carries attestation_level; no target = raw subprocess = level 1). The
  missing claim left `subject.attestation_level` unset at ABAC evaluation,
  so Deny policies keyed on the least-attested class (e.g. the spec's own
  gate-approved-persona deny on attestation_level < 3) silently missed
  raw-subprocess agents. The no-target case now mints `Some(1)` via
  `map_or`.

### Test evidence

- New regression test `spawn_without_compute_target_mints_attestation_level_one`
  (`crates/gyre-server/src/api/spawn.rs`): full `POST /api/v1/agents/spawn`
  through the router on the mem state with no compute target configured;
  decodes the returned worker JWT with the signing key's own `validate()`
  (real EdDSA verification) and asserts `attestation_level == Some(1)`.
- `cargo check -p gyre-server --tests` passes (exit 0, 7m32s, wrapper-run,
  `SKIP_WEB_BUILD=1`); no new warnings from the changed files.
- Server test execution is deferred to the independent verifier (build
  wrapper blocks server test compilation in this stage). Required probes:
  - `cargo test -p gyre-server spawn_without_compute_target_mints_attestation_level_one`
  - `cargo test -p gyre-server agent_jwt` (claim-merge and live-evaluation
    suite around the minted claims, including
    `agent_jwt_persona_and_attestation_level_reach_live_evaluation`)
- Evidence saved under `/tmp/stage/review-evidence/` (check log + probe
  commands).
