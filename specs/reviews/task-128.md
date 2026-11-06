# Review — task-128 (ABAC policy engine core: entity, conditions, and attribute model)

Spec: `specs/system/abac-policy-engine.md` §Core Concepts through §Policy Examples
(coverage sections 2–7; sections 8–10 Evaluation Engine/Flow/Composition are
task-129's scope).
Candidate: `e8540e9e` on `pipeline/task-128/fdcad00a10404d57a165e774cc09323b-4`,
base `7f8b5d9c`. Work surface recovered via checkpoint `29ad56fb` (listed in
`commits:` frontmatter); the candidate-own commits beyond the checkpoint
(`9f90f83f`, `e8540e9e`) touch only `specs/tasks/task-128.md`.
Verdict: **complete** (approved).

## Verification performed (this review round)

- Checkout verified at the exact candidate SHA with a clean tree.
- Base→candidate diff reviewed file-by-file (22 files; the product surface is
  the 15 crate files listed in `/tmp/stage/review-evidence/review-summary.txt`).
- Repo invariant scripts run in-sandbox, all passing: check-migration-versions,
  check-arch, check-abac-route-registry, check-abac-exempt-handlers,
  check-mem-port-contracts, check-inert-enforcement, check-scope-literal-defaults,
  check-task-commit-attribution.
- Frozen baselines: exemption-file changes are pure line-number shifts (no
  entries added/removed); resolver duplicate entries and total mapping count
  identical to base; the duplicate migration version `000056` exists identically
  at the base (pre-existing, not this candidate's — the candidate adds no
  migration; policy tables use `000007_platform_entities` +
  `000028_policy_immutable`).
- Task-128 surface byte-identity: `git diff 6df5dd5 e8540e9e` over all 15 task
  crate files is empty except `mem.rs`, whose only delta is the task-156
  MetaSpecRepository same-version archive guard (not exercised by any
  policy/ABAC/auth test). The focused suites the implementer reports as passing
  at `6df5dd5` therefore ran against a byte-identical task surface.
- Test-count cross-check: policy_engine 31, abac_middleware 23, api::policies
  12, sqlite::policy 4 — all match the claimed suite sizes; auth.rs contains 33
  test fns vs the notes' "36/36" (trivial notes discrepancy, not a contract
  item).

## Verified real (no findings)

- **Policy entity matches the spec exactly** — every field including
  `immutable`, `scope`, `scope_id: Option<Id>`, `conditions`, `created_by: Id`,
  timestamps; `built_in` is a justified extension required by the spec's
  §Built-In Policies section. `PolicyScope` {Tenant, Workspace, Repo},
  `PolicyEffect` {Allow, Deny}, all 8 `ConditionOp` variants, `ConditionValue`
  {String, StringList, Number, Bool} (+`Null` as the value-side placeholder for
  `Exists`).
- **Dynamic references are real and fail closed.** `resolve_condition_value` /
  `resolve_dynamic_value` / `resolve_dynamic_scalar` resolve `$resource.*` /
  `$subject.*` against the live context preserving the referenced attribute's
  type; unresolvable references fail the condition, so an Allow with an
  unresolvable reference cannot grant. Behavioral tests cover positive,
  negative, type-preservation, list-in-scalar-position, and unresolvable cases.
- **Port + adapters are real storage, not stand-ins.** `PolicyRepository` with
  CRUD + `list_by_scope`; the SQLite adapter runs real Diesel SQL against the
  `policies` table with conditions/actions/resource_types as JSON, and the
  built-in delete guard is enforced in code in all three adapters (SQLite,
  Postgres, mem) — contract parity confirmed by check-mem-port-contracts and by
  reading each guard.
- **Attribute extraction is from real sources end-to-end.** Subject identity
  from the auth extractor with insert-only JWT-claim merging (claims cannot
  override extractor-resolved identity facts — kill-tested by
  `merge_jwt_claims_never_overrides_existing_attributes`); `wl_stack_hash` →
  `stack_hash` and numeric `attestation_level` normalization; memberships and
  team store for `workspace_ids`/`team_ids`/`workspace_role`; entity lookups
  (spec ledger, personas, tasks, agents, MRs, teams) for
  `resource.tenant_id`/`owner`/`approval_status` with the repo→workspace→tenant
  scope chain; `env.ip` from the socket peer only (CWE-348 — `main.rs` switched
  to `into_make_service_with_connect_info`); `env.budget_remaining` from the
  real BudgetConfig/BudgetUsage; `env.main_health` from the real merge-queue
  pause state. `resource.visibility`/`resource.team` have no carrying entity
  today (verified: only code-graph symbol `Visibility` exists) and are
  documented to fail closed — accurate, not hollow.
- **Middleware enforces, not audits.** Deny → 403; every decision is recorded
  to the audit log. Policies load from the shared store on every request (no
  decision cache). The evaluation order implements immutable-Deny-first, then
  priority with scope-specificity and Deny-before-Allow tie-breaks, then
  default deny.
- **All five §Policy Examples are encoded as verbatim behavioral tests**
  (`spec_example_agent_repo_scope` including the `$resource.repo_id` reference,
  `persona_management`, `viewer_no_spawn`, `gate_approved_persona`,
  `spec_default_deny`). The middleware tests are live tower oneshot requests
  through the real middleware (membership-sourced role denial, entity-lookup
  scope chain proven by a cross-tenant deny at 810 that only fires if the
  tenant is really looked up rather than fabricated, budget both-direction
  flip, forge-state health). Worker JWTs now carry repo scope + attestation
  level + persona claims (`mint_scoped`), so agent subject attributes are real
  on the wire.
- **API surface is guarded**: reserved `trust:`/`builtin:` name prefixes
  rejected on create and update; built-in and immutable policies reject
  modification and deletion; user-created policies are always
  `built_in: false, immutable: false`.

## Notes (not findings against this task)

- **Scope/scope_id applicability filtering at evaluation** (spec §Evaluation
  Flow: "collect applicable policies" by scope chain) is **task-129's coverage**
  (sections 8–10), not task-128's. Today `abac_middleware` loads all policies
  and `evaluate` filters by action/resource_type only, so a policy stored with
  `scope: Workspace, scope_id: Some(ws-A)` applies in any workspace unless it
  self-scopes via a `resource.workspace_id` condition. The shipped built-ins
  are all tenant-scope/`scope_id: None`, and trust policies are the main
  exposed case. Task-129 must implement scope-chain collection; flagging here
  so it is not lost.
- The builtin `agent-repo-scope` policy uses `Exists` on `subject.repo_scope`
  (disabled by default) rather than the spec example's
  `NotEquals $resource.repo_id`; the spec-example semantics are tested verbatim
  in `policy_engine::tests`. §Built-In Policies is task-129's section.
- `eval_condition` type-combination gaps (`NotEquals` on Bool, `In` on a
  list-valued attribute) evaluate false — fail-closed direction, unused by the
  spec examples.
- Rust suites could not be executed in this sandbox (crates.io unreachable —
  independently reproduced; see
  `/tmp/stage/review-evidence/transport-restriction-cargo-offline.txt`).
  Deferred to verification: the five focused suites plus `cargo test --all`.
  Compile risk of the base merge is mitigated by the byte-identity above and by
  the merge touching no task-128 file except the mem.rs guard.

Findings:

- (none)
