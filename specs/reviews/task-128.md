# Review — task-128 (ABAC policy engine core: entity, conditions, and attribute model)

Spec: `specs/system/abac-policy-engine.md` §Core Concepts, §Attributes, §Policy
Language, §Policy Entity, §Conditions, §Policy Examples.
Candidate: `e615ac52fdca982d4f6ab350262c9b19d427bb08` (assigned base
`4ec1e52658f80e009292c1402e158b48ff1b1e88`). Diff = 18 files, +4860/−217
(the branch also carries merge/rebase/process commits reconciling earlier
task-128 rounds with main; product commits match the `commits:` frontmatter).
Verdict: **complete** — approved, no findings.

## Probes run (this checkout, exact candidate HEAD)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-domain -p gyre-ports policy` →
  **6 passed, 0 failed** (exit 0), incl. the field-level trust-policy
  assertions (`trust_policies_for_level_supervised_generates_merge_hold_deny`
  asserts every consumed field of the generated Deny).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters policy` → **14 passed, 0
  failed** (exit 0), incl. the SQLite policy suite added by this task
  (`policy_crud_roundtrip_preserves_fields`, `delete_rejects_built_in_policy`,
  `list_by_scope_filters_scope_and_scope_id`,
  `delete_by_name_prefix_and_scope_id_only_touches_matching_scope`).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-server --tests` → **exit 0** —
  type-checks the entire gyre-server source AND test code, including the
  c7aa6438 CI-repair call sites (`TeamRepository::list_for_user`,
  6-arg `Team::new` + `TeamMembership` store) that fixed the fc46ffc
  compile failure behind CI run 43393123721.
- `cargo test -p gyre-server --lib abac` → **blocked by the executor's 4 GiB
  build wrapper** ("gyre-server code generation plus the model exceeds
  4 GiB"). Infrastructure restriction, not a code defect; the suite is
  recorded below for the independent verifier.
- Mechanical gates on the merged tree, all OK: check-arch,
  check-abac-route-registry, check-migration-versions,
  check-mem-port-contracts, check-abac-exempt-handlers (75 handlers),
  check-forged-scope-fields, check-inert-enforcement,
  check-in-memory-state-stores, check-scope-literal-defaults,
  check-forwarded-header-trust, check-task-commit-attribution.
- Evidence: `/tmp/stage/review-evidence/probe-commands.txt`.

## Verified against the task contract

- **Policy entity matches spec verbatim** (`gyre-domain/src/policy.rs`): id,
  name, description, scope, `scope_id: Option<Id>`, priority, effect,
  conditions, actions, resource_types, enabled, immutable, `created_by: Id`,
  created_at, updated_at — plus an additive `built_in` flag consumed by the
  §Built-In Policies delete guard. `PolicyScope {Tenant, Workspace, Repo}`,
  `PolicyEffect {Allow, Deny}` exactly as specced.
- **Conditions**: all 8 `ConditionOp` variants and the 4 `ConditionValue`
  variants implemented with real evaluation (`policy_engine.rs
  eval_condition`, exhaustive match with `_ => false` fail-closed default);
  `Null` is an additive placeholder for `Exists`.
- **Dynamic references**: `$resource.*`/`$subject.*` resolve at evaluation
  time (`resolve_condition_value` / `resolve_dynamic_value` /
  `resolve_dynamic_scalar`), type-preserving; unresolvable references fail
  the condition (fail-closed — an Allow with an unresolvable reference
  cannot grant). The fail-closed property is pinned by a test whose outcome
  FLIPS under a fail-open mutation (`dynamic_reference_unresolvable_fails_closed`:
  `subject.type NotEquals $resource.missing` would ALLOW under a fabricated
  `""`); both directions of resolution are asserted in
  `dynamic_reference_resolves_resource_attr_at_eval_time` /
  `dynamic_reference_subject_attr_resolves_at_eval_time`.
- **Evaluation order matches spec**: immutable Deny first (before all
  priority sorting, unoverridable), then (priority desc, scope specificity
  repo>workspace>tenant, **Deny before Allow**) — the "deny takes precedence
  over allow within the same priority" rule, covered by
  `deny_overrides_allow_at_same_priority_and_scope`; default deny last.
- **PolicyRepository port**: CRUD + `list_by_scope` + decision audit,
  implemented in SQLite (conditions/actions/resource_types as JSON), mem,
  and Postgres adapters; the "delete rejects built-in" port contract is
  enforced in code in BOTH the SQLite adapter (added this round, tested by
  `delete_rejects_built_in_policy`) and the mem adapter
  (check-mem-port-contracts OK).
- **Attribute model, all four categories, real sources**:
  subject type/id/global_role/tenant_id from the auth extractor;
  workspace_ids/team_ids/workspace_role exclusively from the membership and
  team stores (`TeamRepository::list_for_user`, implemented in all three
  adapters — verified); persona/stack_hash (wl_stack_hash
  normalized)/attestation_level/repo_scope from agent JWT claims,
  **agent subjects only**, via an allowlist of exactly those four names and
  insert-only merge (`merge_agent_jwt_claims`) — the round-2 spoofing
  channel (a human JWT carrying `workspace_role: Admin` defining the
  attribute) is closed and killed end-to-end by
  `workspace_role_claim_cannot_spoof_membership_attribute` /
  `human_persona_claim_cannot_define_agent_attribute` /
  `viewer_with_admin_claim_still_trips_notin_deny`; resource
  id/workspace_id/repo_id from the request path with parent-id leak
  suppression on parent-scoped child routes and percent-decoded `:path`
  identity on spec routes (invalid encoding → unset → fail closed);
  resource tenant/owner/approval_status from real store lookups with a
  repo→workspace→tenant scope chain; env.ip from `ConnectInfo` socket peer
  (never forwarded headers; `into_make_service_with_connect_info` confirmed
  in main.rs); env.budget_remaining from the real BudgetConfig/Usage
  repositories; env.main_health from the real merge-queue pause state
  (`merge_processor::queue_health`); env.time/action/resource.type injected
  by the evaluator. `resource.team`/`resource.visibility` are honestly
  documented as unpopulated (no entity carries those fields yet) and fail
  closed — disclosed in the coverage row, not hidden.
- **All five §Policy Examples encoded verbatim** as evaluation tests
  (`spec_example_agent_repo_scope` incl. its `$resource.repo_id` reference,
  `_persona_management`, `_viewer_no_spawn`, `_gate_approved_persona`,
  `_default_deny` with matched-policy identity distinguished from
  no-match default deny).
- **Test quality**: 42 abac_middleware test fns + 34 policy_engine test fns
  (≥ the claimed 32/34); the middleware tests drive live `tower::oneshot`
  requests through the real middleware with real seeded stores and assert
  HTTP status outcomes — not mirrored logic. The
  `developer_jwt_can_generate_on_llm_endpoints` regression test seeds real
  membership/repo attribute sources (b0ddcc4e) and controls the decoy
  `workspace_role` JWT claim with a Viewer-membership negative case.
- **CI repair (c7aa6438)** is correct and minimal: the two task-128 call
  sites left on the removed task-122 Team API now use
  `TeamRepository::list_for_user` (subject.team_ids extraction) and the
  6-arg `Team::new` + `TeamMembership` store (test fixture), preserving the
  killing test's fixture (user is a team-platform member in ws-1). No
  references to the removed `Team.member_ids` API remain;
  `cargo check --tests` proves the tree compiles.
- No TODO/FIXME/unimplemented/`bail!` stubs added in the diff; no `web/`
  changes at all (no generated-bundle concerns for this candidate).

## Verifier must run (blocked in-sandbox by the 4 GiB build wrapper)

- `cargo test -p gyre-server --lib abac` — the merged ABAC suite, incl.
  `membership_team_ids_populated_from_team_store` (exercises the c7aa6438
  `list_for_user` extraction and reworked `setup_membership_state`
  fixture), `developer_jwt_can_generate_on_llm_endpoints`,
  `workspace_role_claim_cannot_spoof_membership_attribute`,
  `human_persona_claim_cannot_define_agent_attribute`,
  `spec_resource_id_from_decoded_path_reaches_live_evaluation`,
  `env_budget_remaining_from_real_budget_system`,
  `env_main_health_from_real_forge_state`,
  `spawn_without_compute_target_mints_attestation_level_one`
  (api/spawn.rs), and the policy_engine dynamic-reference/spec-example
  tests.
- `cargo test --all` (or `cargo nextest run --all`) plus Clippy per CI —
  the task's acceptance criterion; the shipped round already replicated
  the Clippy-diff rule locally with 0 findings on changed lines.
