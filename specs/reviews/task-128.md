# Review — task-128: ABAC policy engine core (entity, conditions, attribute model)

Candidate: `c76ab637ba051bd84299ab206e424d966ad0ef3f` (base `6a9739e519ae473e66dd0d9d403f3108f6784431`)
Reviewer: independent review agent (stage: review)
Verdict: **approved**

## Scope

`specs/system/abac-policy-engine.md` §Core Concepts–§Policy Examples:
attribute model, Policy entity, PolicyScope/PolicyEffect, Condition with all
8 operators and 4 value kinds, dynamic references, PolicyRepository port +
adapters, attribute extraction from auth context, evaluation semantics.

## What was inspected

Full diff base→candidate (24 files). All 12 attributed commits present in
the branch. Production changes reviewed hunk-by-hunk:

- `gyre-domain/src/policy.rs`: `Policy` matches the spec struct exactly
  (`scope_id: Option<Id>`, `created_by: Id`, `immutable`, conditions);
  `PolicyScope` {Tenant, Workspace, Repo}; `PolicyEffect` {Allow, Deny};
  `ConditionOp` has all 8 variants; `ConditionValue` String/StringList/
  Number/Bool (+`Null` as the `Exists` value-side placeholder, harmless).
- `policy_engine.rs`: dynamic references (`$resource.*`/`$subject.*`)
  resolve at evaluation time preserving the referenced attribute's type;
  unresolvable references fail the condition (fail closed). Evaluation:
  immutable Deny first, then priority desc, scope specificity
  (repo>workspace>tenant), Deny-before-Allow tie-break, default deny —
  matches §Policy Language / §Composition rule 3. `merge_jwt_claims` is
  insert-only so claims cannot override extractor-resolved identity facts.
- `abac_middleware.rs`: per-spec source column extraction — subject
  identity/global_role/tenant_id from auth context; persona/stack_hash/
  attestation_level/repo_scope from (server-minted, signature-validated)
  agent JWT claims with `wl_stack_hash`→`stack_hash` normalization;
  workspace_ids/team_ids/workspace_role from the real membership/team
  stores; resource id/workspace_id/repo_id from the request path;
  entity-lookup attributes (resource.tenant_id via scope chain
  repo→workspace→tenant, owner, approval_status) from real store lookups
  per resource type; env.ip from ConnectInfo socket peer (never forwarded
  headers — CWE-348); env.budget_remaining from BudgetConfig/BudgetUsage;
  env.main_health from the real merge-queue pause state; env.time/action/
  resource.type injected by the evaluator. Missing lookups leave attributes
  unset → conditions fail closed.
- Adapters: `scope_id`/`created_by` carried as `Id`; built-in delete guard
  added in code to BOTH SQLite and Postgres adapters (port contract
  "Fails if the policy is built-in"), matching the mem adapter's existing
  guard; new SQLite adapter tests cover the roundtrip, the guard, and
  scope-filtered listing. No new migration (table already exists in
  `000007_platform_entities` + `000028_policy_immutable`) — correct, since
  a duplicate migration would violate the migration-numbering invariant.
- `spawn.rs`: worker JWT now minted via `mint_scoped` with
  workspace_id/repo_id derived server-side from the loaded repo entity
  (404 if the repo does not exist — no caller-forged scope in the JWT).
- `main.rs`: `into_make_service_with_connect_info` wired so ConnectInfo
  (env.ip source) exists in production, matching the middleware reads.

## Probes run (logs in /tmp/stage/review-evidence/)

Focused suites at the candidate head, all green and matching the shipped
claims: `policy_engine::` 31/31, `abac_middleware::` 23/23, `auth::tests`
36/36, `api::policies` 12/12, `sqlite::policy` 4/4.

Discriminating mutation probes (source restored and re-verified after each):

1. `resolve_condition_value` disabled (returns the unresolved value):
   `policy_engine::` 5 failures — `dynamic_reference_resolves_resource_
   attr_at_eval_time`, `dynamic_reference_subject_attr_resolves_at_eval_
   time`, `dynamic_reference_number_and_list_resolution`,
   `dynamic_reference_scalar_number_coerces_for_string_compare`,
   `spec_example_agent_repo_scope`. The dynamic-reference behavior is
   genuinely test-covered, not self-confirming.
2. `extract_entity_attributes` disabled (immediate return):
   `abac_middleware::` 5 failures — `spec_entity_attributes_reach_live_
   evaluation`, `spec_approval_status_reaches_live_evaluation`,
   `spec_resource_tenant_resolves_via_scope_chain`,
   `task_entity_scope_backfills_workspace_and_tenant`,
   `persona_entity_attributes_reach_live_evaluation`. The entity-lookup
   scope-chain extraction is genuinely test-covered.

Mechanical invariants pass at this head: check-arch, check-abac-route-
registry, check-abac-exempt-handlers, check-mcp-write-tools, check-mem-
port-contracts, check-forwarded-header-trust, check-forged-scope-fields,
check-fabricated-scope-defaults, check-scope-literal-defaults,
check-inert-enforcement, check-lossy-secret-conversion,
check-relative-path-defaults, check-in-memory-state-stores. All exemption
files unchanged in entry count (only line-number shifts from the diff).

The resumed-run fix to `spec_resource_tenant_resolves_via_scope_chain`
(cross-tenant Deny at 810 instead of a same-tenant Allow) is correct: the
builtin Developer Allow at 800 makes a non-matching Allow non-discriminating,
and the Deny form only fires on a real scope-chain tenant lookup.

## Notes (non-blocking, for the verifier / future hardening)

- `cargo test --all` was not run here (the executor wrapper rejects
  workspace-wide invocations); it remains a verification gate. All focused
  suites plus the full gyre-server test-binary set compiled and passed
  their filtered runs.
- `spawn.rs` `mint_scoped` attestation comment says "No target (None) is a
  plain local subprocess: level 1", but with no compute target the
  `attestation_level` claim is absent (not `1`). Fail-closed direction —
  conditions on the attribute deny rather than grant — so safe, but the
  comment could mislead.
- Membership-sourced attributes overwrite claim values only when a
  membership exists; a membership-less user's IdP-issued claim named
  `workspace_role` would persist as `subject.workspace_role`. Bounded by
  IdP trust (JWTs are signature-validated; the server-minted agent claims
  carry no such field). Spec sources workspace_role from Membership; a
  future hardening could clear spec-membership-sourced keys after claim
  merge when no membership backs them.

## Verdict

Real production implementations behind every acceptance criterion:
entity/model match the spec, all operators/value kinds present, dynamic
references resolve live and fail closed, port + adapters enforce the
documented contracts in every adapter, and attribute extraction runs
against real stores with discriminating end-to-end tests. Approved.
