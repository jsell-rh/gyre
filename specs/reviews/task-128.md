# Review — task-128 (ABAC policy engine core: entity, conditions, and attribute model)

Spec: `specs/system/abac-policy-engine.md` §Core Concepts, §Attributes, §Policy Language,
§Policy Entity, §Conditions, §Policy Examples (coverage rows 2–7).
Candidate: `f5c2a6efaf1130a93b49960062e7b565f5947124` (base `cfeefbda975211a6ac24a17e5920be6f8b23008b`).
Product commits: `29ad56fb`, `d779f1e8`, `8d5494e9`, `5327b8da`, `7502a9ee`, `16e740051`
(+ merges/process commits; all six recorded in the committed frontmatter).

Verdict: **approved** (round 5). Evidence: `/tmp/stage/review-evidence/`.

## What was reviewed

Full diff base..candidate across `gyre-domain/src/policy.rs`, `gyre-server/src/policy_engine.rs`
(+744), `gyre-server/src/abac_middleware.rs` (+3.5k incl. tests), `auth.rs`, `api/spawn.rs`,
`api/orchestrator.rs`, `api/policies.rs`, `api/workspaces.rs`, `mcp.rs`, `mem.rs`,
`merge_processor.rs`, `stale_agents.rs`, both policy adapters, coverage files, task file.

## Verification (independent probes, this round)

- `cargo test -p gyre-domain policy` → 6/6 passed (p1-domain-policy.txt).
- `cargo test -p gyre-adapters --lib sqlite::policy` → 4/4 passed against real SQLite:
  CRUD roundtrip preserving every field (incl. `scope_id: Option<Id>`, `created_by: Id`,
  conditions JSON), built-in delete rejection (policy survives), scope-filtered listing,
  prefix+scope delete isolation (p2-adapters-policy.txt).
- `cargo check -p gyre-server` → clean (p3); `cargo check -p gyre-server --tests` → clean —
  the full abac_middleware/policy_engine test modules type-check (p4).
- All 19 mechanical invariant scripts pass (p5-invariant-scripts.txt), including
  `check-abac-route-registry.sh` (the diff re-spells shared route literals as consts —
  duplicate count unchanged), `check-mem-port-contracts.sh` (built-in delete guard now
  enforced in code in BOTH sqlite and postgres adapters, matching the mem adapter),
  `check-task-commit-attribution.sh` (passes at the committed candidate; the main-checkout
  failure is caused solely by the executor's working-tree restore of the pristine assigned
  task doc — verified via a detached worktree at the candidate SHA).
- Mutation-reasoning on the fail-closed dynamic-reference killing case (read-level trace,
  server test binaries deferred to the verifier per the 4 GiB budget rule):
  `subject.type NotEquals $resource.missing` flips to Allow under a fail-open resolver that
  fabricates `""` — the assertion expecting Deny is a genuine killing case, not
  self-confirming.

## Findings — none blocking

1. **Domain entity matches spec exactly**: all spec fields present with spec types
   (`scope_id: Option<Id>`, `created_by: Id`, `immutable`, `conditions`, …) plus the
   pre-existing `built_in` (required by §Built-In Policies delete guard, out of this task's
   sections but load-bearing). `PolicyScope` {Tenant, Workspace, Repo}, `PolicyEffect`
   {Allow, Deny}, all 8 `ConditionOp` variants, `ConditionValue` String/StringList/Number/Bool
   (+ pre-existing `Null` placeholder for `Exists`). The policies table (migration 000007 +
   000028 immutable) already covers the schema; the Id-type change is storage-invariant
   (TEXT on both sides), so no new migration was needed — verified, not a gap.
2. **Attribute model is real, not structural**. Subject: type/id/global_role/tenant_id from
   the auth extractor; persona/stack_hash (wl_stack_hash normalized)/attestation_level/
   repo_scope from agent-JWT claims, gated on `subject.type == "agent"` and allowlisted to
   exactly those four names (`merge_agent_jwt_claims`); workspace_ids/team_ids/
   workspace_role from the membership/team stores. The round-2 spoofing channel (a
   verified human JWT carrying `workspace_role: "Admin"` defining the membership-sourced
   attribute on routes the membership block cannot overwrite) is closed by allowlist+gate
   and killed end-to-end by `workspace_role_claim_cannot_spoof_membership_attribute`,
   `human_persona_claim_cannot_define_agent_attribute`, and
   `viewer_with_admin_claim_still_trips_notin_deny` (which also kills any reordering that
   merges claims after membership extraction). Resource: id/workspace_id/repo_id from the
   request path with correct parent-scoped-child suppression (derived from the pattern via
   `prefix_entity_type`, covering merge-requests/agents prefixes — round-4 fix), decoded
   `:path` identities for spec routes, entity-lookup tenant/owner/approval_status from real
   stores per resource type. Env: ip from ConnectInfo socket peer (main.rs uses
   `into_make_service_with_connect_info` — real in production, never forwarded headers),
   budget_remaining from BudgetConfig/BudgetUsage, main_health from the merge-queue pause
   state, time/action/resource.type injected by the evaluator.
3. **Dynamic references are genuine**: `$resource.*`/`$subject.*` resolve at evaluation time
   preserving the referenced attribute's type; unresolvable references fail the condition
   (fail closed — an Allow with an unresolvable reference cannot grant); list references in
   scalar position do not fabricate a scalar. All five §Policy Examples are encoded as
   verbatim evaluation tests including the example's `$resource.repo_id` reference.
4. **Evaluation-order semantics** (within this task's §Policy Language scope): immutable
   Deny first, priority/scope-specificity/Deny-before-Allow ordering, default deny.
5. **Tests are behavioral, not inflated**: middleware tests drive live `tower::oneshot`
   requests through the real middleware with real signed JWTs and real stores; priority
   laddering in the env/claim tests closes the self-confirming path (e.g. the stack-hash
   test places a Deny between the builtin agent Allow and the keyed Allow). Server test
   binaries were not compiled in this review (budget rule) — the verifier must run
   `cargo test -p gyre-server --lib policy_engine`, `--lib abac_middleware`, `--lib auth::`,
   `--test task134_review_probe`, and `cargo test --all` (task AC).

## Non-blocking notes (recorded for downstream owners)

- `resource.team` and `resource.visibility` are unpopulated: no domain entity carries those
  fields today (verified by grep). Conditions on them fail closed, and the coverage row
  discloses this honestly. When an entity gains the fields, its extraction arm must set them.
- Policy scope applicability (filtering `scope`/`scope_id` against the request's
  tenant/workspace/repo chain) is NOT implemented in `evaluate` — a workspace-scoped policy
  currently evaluates for requests in any workspace. This pre-exists the base and is
  assigned to task-129 (§Evaluation Engine/Flow/Composition, coverage rows 8–10), not to
  task-128's sections; flagged here so task-129's reviewer checks it.
- Working-tree note: the checkout's `specs/tasks/task-128.md` is the executor-restored
  pristine assigned contract (`commits: []`); the committed candidate carries the full
  six-commit frontmatter and `## Shipped` section, and the attribution gate passes there.
