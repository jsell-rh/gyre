# Review — task-128 (ABAC policy engine core: entity, conditions, attribute model)

Spec: `specs/system/abac-policy-engine.md` §Core Concepts, §Attributes, §Policy Language, §Policy Entity, §Conditions, §Policy Examples (coverage rows 2–7).
Candidate: `cb8b17f5a` on `pipeline/task-128/2d76bf76a5fd45228978603fb3238837-1`; assigned base `44a01390c` (main, task-171 LLM endpoint).
Verdict: **approved** — no findings. Server test-suite execution is deferred to the independent verifier (see "Deferred probes"); everything runnable in this sandbox was run and passed.

## Scope and shape of the diff

Base→candidate is 18 files (+4732/−209): the retained task-128 implementation from the published branch head `bf6a1f6e` plus the merge of that head onto current main (`5b879846c`). Merge-resolution exactness verified independently: `git diff bf6a1f6e cb8b17f5a -- crates/gyre-server/src/abac_middleware.rs` contains **exactly** main's two task-171 additions (the `builtin-developer-generate-access` Allow policy at priority 800 and the `developer_jwt_can_generate_on_llm_endpoints` regression test) and nothing else; the task-128 middleware/engine implementation is unchanged by the merge. `mint_with_workload` (partially superseded by `mint_worker_scoped` in spawn) is still live at three other call sites — no dead code.

## What was verified against the task contract

- **Policy entity** (`gyre-domain/src/policy.rs`): every spec field present — id, name, description, scope, `scope_id: Option<Id>`, priority, effect, conditions, actions, resource_types, enabled, immutable, `created_by: Id`, created_at, updated_at — plus two documented supersets: `built_in` (§Built-In Policies delete guard) and `ConditionValue::Null` (placeholder for `Exists`). `PolicyScope {Tenant, Workspace, Repo}` and `PolicyEffect {Allow, Deny}` match verbatim.
- **Conditions**: all 8 `ConditionOp` variants are implemented in `eval_condition` (not just declared) — Equals/NotEquals (string+number+bool), In/NotIn (StringList), GreaterThan/LessThan (numeric), Contains (list membership + substring), Exists. `ConditionValue` carries String/StringList/Number/Bool.
- **Dynamic references** (`policy_engine.rs`): `$resource.*`/`$subject.*` resolve at evaluation time preserving the referenced attribute's type; unresolvable references fail the condition (fail closed — an Allow with an unresolvable reference cannot grant). The fail-closed property is mutation-hardened: `dynamic_reference_unresolvable_fails_closed` includes the `NotEquals $resource.missing` flip case that kills a fail-open resolver fabricating `""`.
- **PolicyRepository port**: full CRUD + `list_by_scope` + decision audit; the built-in delete guard (port contract "Returns an error if the policy is built-in") is now enforced in code in BOTH the SQLite and Postgres adapters (mem already guarded) — `check-mem-port-contracts.sh` passes. The `policies` table (migrations 000007/000028/000057/000061) already exists in base; the `Option<Id>`/`Id` domain change round-trips through the TEXT columns.
- **Attribute extraction** (abac_middleware): subject type/id/global_role/tenant_id from the auth extractor; workspace_role/workspace_ids/team_ids from the real membership/team stores; persona/stack_hash (wl_stack_hash normalized)/attestation_level/repo_scope from agent JWT claims, **allowlisted to those four names and gated on subject.type == "agent"** (the round-2 anti-spoofing fix — a verified-but-caller-chosen human claim cannot define membership- or agent-sourced attributes); resource id/workspace_id/repo_id derived from the request path with parent-scoped child-route and decoded-`:path` handling; resource tenant_id/owner/approval_status from real store lookups per resource type; env.ip from ConnectInfo socket peer (no forwarded-header trust, CWE-348); env.budget_remaining from the real budget config/usage; env.main_health from the real merge-queue pause state; action/resource.type/env.time injected by the evaluator. Default deny stands when no policy matches.
- **Evaluation order**: immutable Deny first, then priority → scope specificity → Deny-before-Allow tie-break ("deny takes precedence over allow within the same priority"), default deny — matches §Policy Language.
- **Tests are discriminating, not inflated**: the spoofing tests (`workspace_role_claim_cannot_spoof_membership_attribute`, `human_persona_claim_cannot_define_agent_attribute`) drive live tower oneshot requests with a validly-signed hostile JWT and assert 403; `env_budget_remaining_from_real_budget_system` flips the decision when only budget usage changes (kills any stubbed/hardcoded attribute); all five §Policy Examples are encoded as verbatim evaluation tests including the `$resource.repo_id` dynamic reference.
- **Coverage matrix** (`specs/coverage/system/abac-policy-engine.md` rows 2–7 → implemented): notes are evidence-linked and honest — including the explicit disclosure that `resource.team` and `resource.visibility` are unpopulated.

## Non-blocking observations (recorded, not findings)

- `resource.team` / `resource.visibility` (§Attributes, source "Entity field") are not extracted. Verified: no ABAC-addressed domain entity carries a team or visibility field (the `visibility` hits in gyre-domain are call-graph/spec-assertion internals). Conditions on the absent attributes fail closed, the coverage note discloses it, and the extraction arms document where to set them when the fields appear. Not a fake implementation — there is no entity field to source from yet.
- `Policy.built_in` and `ConditionValue::Null` are supersets of the spec struct, both documented in-code and in the coverage note (built_in is required by the port's delete contract; Null is the Exists placeholder).

## Probes run (evidence in /tmp/stage/review-evidence)

| Probe | Exit | Result |
|---|---|---|
| `cargo test -p gyre-domain` (via /tmp/stage/bin/cargo) | 0 | all lib tests pass (1m32s cold) |
| `cargo test -p gyre-adapters --lib policy` | 0 | 13 passed incl. `policy_crud_roundtrip_preserves_fields`, `delete_rejects_built_in_policy`, `list_by_scope_filters_scope_and_scope_id` against real SQLite (6m10s cold) |
| `cargo check -p gyre-server` | 0 | candidate type-checks (5m12s cold) |
| `bash scripts/check-abac-route-registry.sh` | 0 | OK (route-literal → const refactor keeps the registry check green) |
| `bash scripts/check-arch.sh` | 0 | OK |
| `bash scripts/check-mem-port-contracts.sh` | 0 | OK |
| `bash scripts/check-task-commit-attribution.sh` (in a clean worktree of `cb8b17f5a`) | 0 | OK — all 9 task-labeled product commits recorded in `commits:` frontmatter. Note: the same script fails in the review checkout only because the harness reverted `specs/tasks/task-128.md` to the assigned contract (progress not-started, empty commits); the candidate tree itself passes. |

## Deferred probes (independent verifier must run)

The executor's wrapper rejects gyre-server test compilation in this sandbox (4 GiB budget). Required, in order:

1. `cargo test -p gyre-server --lib policy_engine` — 34 tests incl. dynamic-reference fail-closed, deny-tie-break, all §Policy Example evaluations.
2. `cargo test -p gyre-server --lib abac_middleware` — 86 tests incl. the spoofing, path-attribute, entity-lookup, env-attribute, and merge-resolution (`developer_jwt_can_generate_on_llm_endpoints`) suites.
3. `cargo test -p gyre-server --lib` (auth `mint_worker_scoped`/attestation tests, spawn `spawn_without_compute_target_mints_attestation_level_one`, mcp, merge_processor, stale_agents persona plumbing).
4. `cargo test -p gyre-server --test search_wiring` (updated mint_orchestrator call sites).
5. `cargo test --all` (task AC) and `cargo clippy --all-targets` (prior repair rounds addressed rustfmt/clippy; re-confirm).
6. `cd web && npm ci && npm test` — not required by this task (no web/ changes in the diff), but part of the standard verifier gate.
