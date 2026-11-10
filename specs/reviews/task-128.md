# Review — task-128 (ABAC policy engine core: entity, conditions, and attribute model)

Spec: `specs/system/abac-policy-engine.md` §Core Concepts, §Attributes, §Policy Language, §Policy Entity, §Conditions, §Policy Examples (coverage rows 2-7).
Candidate: `b2920232e84c8e3d8aed5ff18cd438b4a9f0feb6` (merge head `a60b86a17` rebased onto base `b5a872087`, plus process commit). Assigned commits: `29ad56fb6`, `d779f1e89`, `8d5494e94`, `5327b8daa`.
Verdict: **approved** (round 4; rounds 1-3 findings verified repaired — see below).

## What was verified

**Domain types** (`gyre-domain/src/policy.rs`): `Policy` carries every spec field (id, name, description, scope, `scope_id: Option<Id>`, priority, effect, conditions, actions, resource_types, enabled, immutable, `created_by: Id`, created_at, updated_at) plus `built_in` for the §Built-In Policies delete guard. `PolicyScope` = {Tenant, Workspace, Repo}; `PolicyEffect` = {Allow, Deny}; `ConditionOp` has all 8 variants; `ConditionValue` = {String, StringList, Number, Bool} + `Null` as the value-side placeholder for `Exists` (spec leaves the Exists value unspecified). Untagged serde ordering (StringList before String) is correct for JSON policy round-trips.

**Port + adapters**: `PolicyRepository` (create/find/list/list_by_scope/update/delete/delete_by_name_prefix[_and_scope_id]/record_decision/list_decisions) implemented in SQLite (conditions/actions/resource_types as JSON columns; built-in delete guard enforced in code) and mem (same guard). `check-mem-port-contracts` passes.

**Attribute extraction** (`abac_middleware.rs`): subject type/id/global_role/tenant_id from the auth extractor; persona/stack_hash/attestation_level/repo_scope from agent JWT claims only (allowlisted to those four names via `merge_agent_jwt_claims`, gated on `subject.type == "agent"`, insert-only, with `normalize_agent_jwt_attributes` also `contains_key`-guarded); workspace_ids/team_ids/workspace_role from the membership/team stores only; resource id/workspace_id/repo_id from the request path (with round-1 parent-id suppression and round-3 percent-decoded `:path` → `resource.id`); resource tenant_id/owner/approval_status from real store lookups per resource type with scope chain repo→workspace→tenant; env.ip from ConnectInfo socket peer (never forwarded headers); env.budget_remaining from the real budget system; env.main_health from the real merge-queue pause state. The generic `merge_jwt_claims` survives only in the dry-run evaluate API (caller-supplied hypothetical attributes; verified no live access decision consumes it). `resource.team`/`resource.visibility` are unpopulated and honestly disclosed as such in the coverage note — no entity type carries those fields; conditions on them fail closed.

**Evaluation** (`policy_engine.rs`): immutable Deny pre-pass, then priority order with scope specificity (repo > workspace > tenant) and Deny-before-Allow tie-break, then default deny. All five §Policy Examples encoded as verbatim tests. Dynamic `$resource.*`/`$subject.*` references resolve at evaluation time preserving type; unresolvable references fail the condition (fail-closed).

## Probes (evidence: `/tmp/stage/review-evidence/`)

- `cargo test -p gyre-server --lib` → **1651 passed / 0 failed** (policy_engine 34, abac_middleware 37 — the latter end-to-end through live tower oneshot requests).
- `cargo test -p gyre-domain --lib policy` → 6 passed; `cargo test -p gyre-adapters --lib policy` → 9 passed; `cargo check -p gyre-cli` → clean.
- All 21 mechanically-enforced invariant scripts pass at the candidate.
- Six mutation probes, each applied to production code at the candidate, focused test run recorded, code restored (tree clean, HEAD re-verified after every probe):
  1. Fail-open `$ref` resolver (fabricating `""` for missing refs) → killed by `dynamic_reference_unresolvable_fails_closed`.
  2. Reverting the live path to the generic claim merge (the round-2 spoofing defect) → killed by `workspace_role_claim_cannot_spoof_membership_attribute` AND `human_persona_claim_cannot_define_agent_attribute`.
  3. Raw (non-decoded) `:path` as `resource.id` (round-3 defect) → killed by `spec_resource_id_from_decoded_path_reaches_live_evaluation` and `path_attributes_path_param_names_addressed_spec`.
  4. Dropping the repos-parent `resource_type != "repo"` guard (round-1 defect) → killed by `repos_parented_collection_route_does_not_leak_parent_repo_id` and `path_attributes_repos_parented_child_routes`.
  5. Skipping the immutable-Deny pre-pass → killed by `immutable_deny_blocks_even_when_high_priority_allow_matches` and `builtin_require_human_spec_approval_blocks_agent_approve`.
  6. Inverting the Deny-before-Allow tie-break → killed by `deny_overrides_allow_at_same_priority_and_scope`.

The prior rounds' fixes are therefore not just present but pinned by killing tests — the mutant transcripts for the exact defect classes of rounds 1-3 all fail the suite.

## Non-blocking observations (recorded for task-129, not defects of this task)

- The live middleware collects policies via `state.policies.list()` with no scope/scope_id filtering, so a Workspace-scoped policy is evaluated against requests outside its workspace (Repo-scoped policies likewise). This is pre-existing base behavior and the spec's scope-chain collection ("Collect applicable policies: repo-scoped, workspace-scoped, tenant-scoped") lives in §Evaluation Flow / §Policy Composition — coverage rows 9-10, assigned to task-129. `evaluate_merge_abac` in merge_processor already filters workspace-scoped policies, confirming the split of responsibility. Task-129 must add scope-chain collection (or scope filtering at the middleware call site).
- `mint_scoped` maps Kubernetes compute targets to attestation level 1 pending runtime-reported pod attestations — disclosed in-code as tracked by task-129.
- Domain `builtin_policies()`'s dormant `agent-repo-scope` entry uses an `Exists`-based condition and `enabled: false`; it is never seeded (the startup seed splices only `builtin:require-human-spec-approval` from the domain set), and §Built-In Policies is coverage row 13, outside this task.

Full workspace `cargo test --all` and CI remain verification/publication gates per the standing review constraints.
