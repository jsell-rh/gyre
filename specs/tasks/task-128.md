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
commits: ["d779f1e8912e3711d35d9b1353ef72e5a3390eb7", "29ad56fb6bb5bb7a27302272753228cb6c902de2", "8d5494e942495e37544806afdec8f1b93009c08b"]
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
extraction): a flat typed attribute bag (Single/List/Number/Bool). Raw
JWT-claim merging into the live context is ALLOWLISTED to the four
agent-claim names (persona, stack_hash, attestation_level, repo_scope —
`merge_agent_jwt_claims`) and gated on `subject.type == agent`; the
generic insert-only `merge_jwt_claims` remains for the dry-run evaluation
API only (caller-supplied hypothetical attributes, no live access
decision). Per spec source column: subject `type`/`id`/`global_role`/
`tenant_id` from the auth context; `persona`/`stack_hash`/
`attestation_level`/`repo_scope` from agent JWT claims, agent subjects only
(`wl_stack_hash`→`stack_hash` normalization); `workspace_ids`/`team_ids`/
`workspace_role` from the membership/team stores — never claim-definable;
resource `id`/`workspace_id`/`repo_id` from the request path;
entity-lookup resource attributes (`resource.tenant_id`, `owner`,
`approval_status`) from real store lookups per resource type (spec ledger,
personas, tasks, agents, MRs, teams; scope chain repo→workspace→tenant) —
`resource.team` and `resource.visibility` are unpopulated (no entity type
carries those fields yet; conditions on them fail closed meanwhile);
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

### Review-repair round 1 (commit d779f1e89)

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

### Review-repair round 2 (commit 8d5494e94)

Repaired the two round-2 review findings:

- **Code (claim spoofing)** — membership-sourced subject attributes
  (`workspace_role`, `workspace_ids`, `team_ids`) were spoofable via raw
  JWT claims on requests whose membership block does not overwrite them
  (membership-less caller, unaddressed workspace, repo-parented route):
  `merge_jwt_claims` (insert-only) ran before membership extraction, and
  the membership block only overwrites `subject.workspace_role` when the
  request addresses a workspace the caller belongs to. A verified JWT's
  claim CONTENT is caller-chosen, so a human token carrying
  `{"workspace_role": "Admin"}` defined the attribute and satisfied
  membership-keyed Allow policies (reproduced end-to-end by the reviewer:
  200 OK on POST /api/v1/repos/repo-1/specs/assist), and evaded Denies
  keyed on the attribute. Same class: `persona`/`attestation_level` were
  merged/normalized from claims for HUMAN subjects although §Attributes
  sources them from the "Agent OIDC claim". Fix: raw-claim merging into
  the live context is now allowlisted to the four agent-claim names
  (`merge_agent_jwt_claims`: persona, stack_hash, attestation_level,
  repo_scope) and gated on `subject.type == agent`; the generic
  insert-only merge remains for the dry-run evaluation API only, where
  the caller explicitly supplies hypothetical attributes and no live
  access decision is made.
- **Process (coverage honesty)** — coverage row 3 (§Attributes)
  overstated the entity-lookup extraction: no code path sets
  `resource.team` or `resource.visibility` (no entity type carries those
  fields). The evidence note now scopes the claim to the attributes
  actually populated (tenant_id, owner, approval_status) and records
  team/visibility as unpopulated pending entity fields, per the
  no-fake-completion rule; row 2's claim-merge wording updated to the
  allowlist semantics.

Killing tests demanded by the review, all mutation-verified (transcripts
under `/tmp/stage/review-evidence/`):

- `workspace_role_claim_cannot_spoof_membership_attribute` — a
  membership-less ReadOnly user with a test-signed JWT carrying
  `workspace_role: Admin` must NOT satisfy a workspace_role-keyed Allow
  on the repo-parented specs/assist route (against the pre-fix merge the
  request got 200; now builtin-default-deny 403).
- `human_persona_claim_cannot_define_agent_attribute` — a human token's
  `persona`/`attestation_level` claims must not satisfy an
  agent-claim-keyed Allow (200 pre-fix, 403 now).
- `viewer_with_admin_claim_still_trips_notin_deny` — a real workspace
  Viewer whose token carries `workspace_role: Admin` still trips the
  persona-management-style `NotIn [Owner, Admin]` deny on a
  workspace-addressed route (kills a post-extraction overwrite merge:
  that mutant flips the extracted Viewer role to Admin and grants the
  write).
- `merge_agent_jwt_claims_allowlisted_to_agent_claim_names` and
  `merge_agent_jwt_claims_never_overrides_existing_attributes` (unit
  level): only the four agent-claim names merge; membership-sourced and
  identity names are never claim-definable; pre-set values stand.

### Test evidence (run at head 8d5494e94, after the round-2 repair)

- Mutation probes (transcripts under `/tmp/stage/review-evidence/`):
  restoring the pre-fix ungated raw-claim merge fails both round-2
  killing tests (200-instead-of-403); a post-extraction overwrite merge
  fails the NotIn-deny guard; earlier probes still hold (repos-prefix
  guard removal fails the round-1 middleware tests; the fail-open
  `resolve_dynamic_value` mutant is killed by the round-1 killing case).
- `cargo test -p gyre-server --lib` — **1516 passed / 0 failed**
  (includes `abac_middleware` 32, `policy_engine` 34; the five §Policy
  Example tests, the membership/entity-lookup attribute tests, and the
  round-1/round-2 tests run end-to-end through live tower oneshot
  requests with non-vacuous controls).
- All mechanically-enforced repo invariant scripts pass at this head,
  including `check-abac-route-registry` (duplicate count 17 = frozen
  baseline; the specs/assist route pattern hoisted into
  `REPOS_SPECS_ASSIST_ROUTE`), `check-arch`, `check-mem-port-contracts`,
  `check-fabricated-scope-defaults`, `check-forged-scope-fields`,
  `check-forwarded-header-trust`, `check-inert-enforcement`, and the
  other security-pattern checks.

Full `cargo test --all` (integration/e2e binaries) and GitHub CI remain
verification/publication gates.

### Earlier repair rounds

- **Round 1 (d779f1e89):** `extract_path_attributes` no longer leaks the
  parent repo id as `resource.id` on repos-parented child routes
  (`{resource_type}_id` child params name the addressed entity; route
  patterns hoisted into consts to hold the frozen duplicate count);
  `dynamic_reference_unresolvable_fails_closed` gained the
  `subject.type NotEquals $resource.missing` killing case; coverage rows
  2-7 flipped to `implemented` with evidence.
- **Rebase (95fdd661d):** merged base fefde294 (task-089 + task-103);
  took the base's task-103 fix for the JWT `claims.tenant_id`
  fabricated-scope exemption; `mint_scoped` sets the base's new `jti`
  field per mint (`minted_jwt_carries_unique_jti` contract holds); no
  task-128 behavior altered by the merge.
