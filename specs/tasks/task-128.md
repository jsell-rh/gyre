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
commits: ["5327b8daaa10dc4f9768b2be6498c00c36602ab4", "8d5494e942495e37544806afdec8f1b93009c08b", "d779f1e8912e3711d35d9b1353ef72e5a3390eb7", "29ad56fb6bb5bb7a27302272753228cb6c902de2", "7502a9eec991513e7e31d51b12fa00a3e6bf77e1"]
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

### Review-repair round 3 (commit 5327b8daa)

Repaired the two round-3 review findings:

- **Code (spec-path resource.id)** — `resource.id` was never populated on
  `:path`-addressed spec routes: `extract_path_attributes` only derives
  `resource.id` from an `:id` or `{resource_type}_id` path param, and every
  `/api/v1/specs/:path*` route has neither, while the spec's §Attributes
  table defines `resource.id` (source "Request path") with the example
  `system/identity-security.md` — a spec path. No policy could condition
  on the addressed spec's identity (conditions failed closed — a
  capability gap, not a security hole). Fix: a `:path` param names the
  addressed entity; the percent-DECODED registry path becomes
  `resource.id` (spec routes, the repo-parented spec-assertions route,
  and the meta-spec blast-radius route); invalid percent-encoding leaves
  it unset (fail closed, same discipline as the entity-lookup arms).
  Killed end-to-end by
  `spec_resource_id_from_decoded_path_reaches_live_evaluation` — a Deny
  at 810 keyed on `resource.id Equals "system/design.md"` on
  `GET /api/v1/specs/system%2Fdesign.md` returns 403 fixed / 200 with
  the defect (the raw still-encoded param value would not match the
  decoded constant, so the deny firing also proves the decoding); a
  retargeted Deny on a different spec path must not fire (200), proving
  the attribute carries the addressed spec's path, not a constant. Unit
  coverage in `path_attributes_path_param_names_addressed_spec` (decoded
  id on the document/history/assertions routes; invalid encoding unset).
- **Code (doc/code divergence)** — `normalize_agent_jwt_attributes`
  documented "never overwrites an attribute already present in the
  context" — exactly the invariant the round-2 anti-spoofing fix relies
  on — but all three setters were unconditional, with no test catching
  the divergence (a future reordering or different claims source would
  silently violate the stated guarantee). Fix: `contains_key` guards
  matching the insert-only `merge_agent_jwt_claims` that runs
  immediately before it; pinned by
  `normalize_agent_jwt_attributes_never_overrides_existing_attributes`.
  No live behavior change: in the live path the only values the guards
  now protect were defined by the same verified token.
- **Process** — coverage row 3 (§Attributes) evidence note extended with
  the `:path` → decoded `resource.id` population (the round-3 gap is
  closed, not just disclosed); test route patterns derived from
  `SPECS_PATH_ROUTE`/`REPOS_ID_ROUTE` consts so the frozen duplicate
  `/api/v1/...` literal count stays at 17.

### Verification-repair round (commit 7502a9eec, after the base-65f71e657
merge)

Repaired the verification findings from the prior candidate (attempt
0b0e5cd7, checks.sh exit 1; all architecture/registry/invariant scripts
and the web build had passed — the failures were rustfmt and clippy
only):

- **rustfmt** — the changed-lines check flagged formatting drift in
  `postgres/policy.rs`, `sqlite/policy.rs`, `abac_middleware.rs`,
  `api/spawn.rs`, `api/workspaces.rs`, `mem.rs`, and `policy_engine.rs`;
  all are now rustfmt-clean (`rustfmt --check` passes on every touched
  file).
- **clippy `empty_line_after_doc_comments`** (abac_middleware) — the
  `extract_path_attributes` doc block was detached from its fn by the
  two path-prefix consts inserted between them; the consts now sit
  above the doc block so it attaches directly to the function it
  documents.
- **clippy `unused_must_use`** (abac_middleware tests) — the seven bare
  `h.block_on(seed_builtin_policies(&state));` statements dropped the
  `anyhow::Result`; they now bind it with `.expect("seed")` like every
  sibling call in the file, so a seeding failure fails the test instead
  of being silently swallowed.
- **clippy `too_many_arguments`** (auth.rs `mint_orchestrator`, 9/7) —
  added the same `#[allow(clippy::too_many_arguments)]` attribute the
  sibling `mint_with_workload` already carries.

No behavior change. Probes run at this round's head (build wrapper
blocks server test binaries here, so compilation-level probes only):
`cargo check -p gyre-server --lib` and `cargo check -p gyre-server
--tests` both clean; `rustfmt --check` clean on all touched files.

### Test evidence (run at merge head ec1f694d5, after the base-dbf9edaea
rebase)

- `cargo test -p gyre-server --lib` — **1694 passed / 0 failed**
  (includes `abac_middleware` 37, `policy_engine` 34; the five §Policy
  Example tests, the membership/entity-lookup attribute tests, and the
  round-1/2/3 tests run end-to-end through live tower oneshot requests
  with non-vacuous controls). Mutation-probe transcripts for rounds 1-3
  are retained under `/tmp/stage/review-evidence/`.
- `cargo check -p gyre-cli` passes (covers the base's gyre-cli +
  gyre-common protocol changes the server lib tests do not exercise).
- All mechanically-enforced repo invariant scripts pass at this head,
  including `check-abac-route-registry` (duplicate count 17 = frozen
  baseline), `check-arch`, `check-mem-port-contracts`,
  `check-fabricated-scope-defaults`, `check-forged-scope-fields`,
  `check-forwarded-header-trust`, `check-inert-enforcement`,
  `check-scope-literal-defaults`, `check-lossy-secret-conversion`,
  `check-relative-path-defaults`, `check-byte-slice-truncation`,
  `check-abac-exempt-handlers`, `check-mcp-write-tools`,
  `check-migration-versions`, `check-migration-sql-portability`,
  `check-in-memory-state-stores`, `check-fail-open-ref-resolution`,
  `check-dead-message-kinds`, `check-unbounded-external-http`,
  `check-unwired-port-methods`, and `check-task-commit-attribution`.

Full `cargo test --all` (integration/e2e binaries) and GitHub CI remain
verification/publication gates. For the current head (7502a9eec, after
the base-65f71e657 merge and the verification-repair round) the verifier
must execute:

- `cargo test -p gyre-server --lib -- abac policy_engine` (the focused
  ABAC suites; the base merge touched `abac_middleware.rs` additively —
  new resolver entries for search suggest/facets and commit-signature
  verification routes, `bearer_token` on test auth contexts — and this
  round's changes are formatting, doc-comment attachment, `.expect`
  bindings in tests, and one `#[allow]` attribute);
- `cargo test -p gyre-server --lib` in full (prior rounds: 1694 passed /
  0 failed at ec1f694d5; the base contributed additional tests since);
- `cargo check -p gyre-cli`;
- the repo invariant scripts (all passed at the prior candidate's head;
  nothing in this round touches registries, migrations, or exemption
  files).

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
- **Rebase (5c0239639):** merged base 7747376 (task-177 rendering + the
  task-093/task-099 fix completions). Four text conflicts resolved by
  taking the base's side on the three exemption files (their entries
  tracked fixes now present in the merged tree: orchestrator spawn
  handlers carry per-handler tenant containment, the void
  `validate_persona` sites are gone, the 9 task-099 auth.rs/lib.rs
  scope-literal sites are fixed with `FROZEN_EXEMPTION_COUNT=12`
  matching) and regenerating `specs/coverage/SUMMARY.md` mechanically
  from the merged coverage files — the base's copy was stale relative to
  its own coverage files. No task-128 source conflicts; post-merge
  probes: `cargo test -p gyre-server --lib` 1558 passed / 0 failed
  (1516 on the pre-merge head; the base contributed the additional
  tests), and `check-abac-exempt-handlers` (88 handlers),
  `check-inert-enforcement`, `check-scope-literal-defaults`,
  `check-arch`, `check-abac-route-registry`, `check-mem-port-contracts`,
  `check-forwarded-header-trust`, `check-fabricated-scope-defaults` all
  pass.
- **Rebase (ec1f694d5):** merged base dbf9edaea (task-117 compute target
  model: Kubernetes/local/SSH/container compute, spec-lifecycle configs,
  task spec_ref, and the task-109/115/117/174 task-file updates). Single
  source conflict in `api/spawn.rs` at the worker-JWT mint site: the base
  added the pre-mint `wl_image_hash` image-digest resolution plus a
  `mint_with_workload` call, while task-128 had switched that site to
  `mint_scoped` (workspace_id/repo_id/persona/attestation_level claims).
  Resolution keeps BOTH: the base's digest-resolution block runs before
  minting, `mint_scoped` gained a `wl_image_hash: Option<String>` parameter
  (set in claims; other `wl_*` workload claims stay on the post-spawn
  attestation record per the base's design), and the spawn site passes the
  resolved digest through it. The three test-side `mint_scoped` callers in
  `abac_middleware.rs` were updated for the new parameter. `SUMMARY.md`
  was again mechanically regenerated — the base's copy was stale relative
  to its own coverage files (agent-runtime 5, spec-lifecycle row shifts).
  Post-merge probes: `cargo test -p gyre-server --lib` 1694 passed /
  0 failed (1651 pre-merge; the base contributed the additional tests),
  `cargo check -p gyre-cli` clean, and all invariant scripts listed in
  the test-evidence section pass. Evidence transcript:
  `/tmp/stage/review-evidence/rebase-dbf9edaea-merge.txt`.
- **Rebase (a60b86a17):** merged base b5a872087 (task-108 admin user
  impersonation, task-104 audit event types, task-205 manifest-driven
  Concept Views, task-208 profile changes, and their verified lineage —
  511 commits). Single text conflict in `specs/coverage/SUMMARY.md`,
  resolved by mechanical regeneration via
  `scripts/update-coverage-summary.sh` from the merged coverage files;
  every source file (`abac.rs`, `abac_middleware.rs`, `auth.rs`,
  `mcp.rs`, `mem.rs`, `merge_processor.rs`, `api/*`) auto-merged with no
  task-128 conflicts. Post-merge probes: `cargo test -p gyre-server
  --lib` 1651 passed / 0 failed (1570 pre-merge; the base contributed
  the additional tests, including 2 new abac_middleware tests for its
  impersonation surface), `cargo check -p gyre-cli` clean, and all
  invariant scripts listed in the test-evidence section pass. Evidence
  transcript: `/tmp/stage/review-evidence/rebase-b5a872087-merge.txt`.