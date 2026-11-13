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
commits: ["b0ddcc4ece0906d90bb46d682ed1123f472a19d9", "9e06a9d3dac428631eab2b69c13bdc5719eba0ac", "35f090dfac66db4afa8c4e2df562ad12667a521a", "19883086799ffc176645bed2f3831144f2701d05", "16e740051d3f7e5a54c4b3c0c11f8d8b10034e8c", "7502a9eec991513e7e31d51b12fa00a3e6bf77e1", "5327b8daaa10dc4f9768b2be6498c00c36602ab4", "8d5494e942495e37544806afdec8f1b93009c08b", "d779f1e8912e3711d35d9b1353ef72e5a3390eb7", "29ad56fb6bb5bb7a27302272753228cb6c902de2", "c7aa6438caa32e59887a419ecfc67e0bfaf57ea0"]
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

ABAC policy engine core, retained from the task-128 branch and kept
reconciled with current main: the `Policy` entity with `PolicyScope`
(Tenant/Workspace/Repo), `PolicyEffect` (Allow/Deny),
`immutable`/`built_in` flags, `Condition` with all 8 `ConditionOp`
variants and the 4 `ConditionValue` variants, dynamic
`$resource.*`/`$subject.*` references resolved at evaluation time, the
`PolicyRepository` port with CRUD + scope-chain listing, SQLite and mem
adapters with migration, and Subject/Resource/Environment attribute
extraction in the middleware (auth context, memberships, teams, entity
lookup, request path, socket peer, budget and merge-queue state).
`subject.workspace_role` and the other membership-keyed names are
sourced exclusively from the membership store — a JWT claim never
defines them.

### CI repair (this round, commit c7aa6438)

PR head fc46ffc failed Clippy, Test, and e2e at run 43393123721 —
all three exit 101 with one shared root cause: a compile error in the
merged tree. The task-122 main merge (4ec1e526) replaced
`Team.member_ids` with the separate `TeamMembership` store (Team now
carries `slug`/`lead`), and the fc46ffc candidate merge left two
task-128 call sites on the removed API:

- `abac_middleware.rs` `subject.team_ids` extraction scanned
  `team.member_ids`; it now uses
  `TeamRepository::list_for_user(workspace, user)` — the
  workspace- and membership-scoped per-user team lookup — which all
  three adapters implement (verified in mem, SQLite, Postgres).
- `setup_membership_state` (test helper) built the team via the old
  4-arg `Team::new` + `team.add_member`; it now uses the 6-arg
  `Team::new` (id, workspace, name, slug, lead, now) and records
  membership through `teams.add_member(&TeamMembership::new(...))`,
  preserving the `membership_team_ids_populated_from_team_store`
  killing test's fixture (user is a `team-platform` member in ws-1, so
  the `subject.team_ids` Deny still fires).

No policy-entity, condition, or evaluation changes were needed — the
task-128 core was untouched by the task-122 model change apart from
these two membership-access sites.

### Test evidence

- `cargo clippy --all-targets --all-features -- -W clippy::all`
  (SKIP_WEB_BUILD=1, via /tmp/stage/bin/cargo) — 0 errors, 0 warnings
  on lines changed vs. both candidate bases (4ec1e526 and ccfd4bc5),
  replicating scripts/check-clippy-diff.py's rule locally.
- `cargo test -p gyre-domain -p gyre-ports -p gyre-common` — 635
  passed, 0 failed (exit 0).
- `cargo test -p gyre-adapters` — 501 passed, 0 failed, 6 ignored
  (exit 0, includes the SQLite policy repository suite).
- `cargo fmt --check -p gyre-server` — clean.
- Mechanical gates on the merged tree: check-arch, hierarchy lint,
  check-task-commit-attribution, check-abac-route-registry,
  check-abac-exempt-handlers, check-mcp-write-tools,
  check-migration-versions, check-migration-sql-portability,
  check-dead-message-kinds, check-byte-slice-truncation,
  check-relative-path-defaults, check-fail-open-ref-resolution,
  check-mem-port-contracts, check-fabricated-scope-defaults,
  check-lossy-secret-conversion, check-scope-literal-defaults,
  check-inert-enforcement, check-forged-scope-fields,
  check-forwarded-header-trust, check-in-memory-state-stores,
  check-unbounded-external-http — all OK.
- Evidence saved under /tmp/stage/review-evidence.

### Verifier must run (gyre-server test execution exceeds the
in-sandbox 4 GiB budget — blocked by the build wrapper; test
compilation itself passed via clippy --all-targets)

- `cargo test -p gyre-server --lib abac` — the merged ABAC suite,
  including `membership_team_ids_populated_from_team_store` (exercises
  this round's `list_for_user` extraction and the reworked
  `setup_membership_state` fixture), `developer_jwt_can_generate_on_llm_endpoints`,
  and the task-128 attribute tests (`membership_*`, `agent_jwt_*`,
  `env_budget_remaining_*`, `env_main_health_*`, `spec_*`,
  `persona_entity_*`, `*_claim_cannot_spoof_*`).
- `cargo nextest run --all` (the CI Test job) and the e2e release
  build, per the task's acceptance criterion.
