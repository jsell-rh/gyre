# Review: TASK-128 — ABAC policy engine core: entity, conditions, and attribute model

**Reviewer:** Independent review agent
**Date:** 2026-04-08
**Candidate:** `4a94fcaf6cd6f8bdf6425f5ca747e99dfe5de634` (base `7c6ac232ad1e43c977540034381e41c47548aa81`)
**Verdict:** approved — task contract met with real implementations

---

## Scope reviewed

`specs/system/abac-policy-engine.md` §Core Concepts, §Attributes, §Policy
Language, §Policy Entity, §Conditions, §Policy Examples (the task's six
coverage sections). §Evaluation Engine scope-chain collection is task-129's
contract and was not held against this candidate.

## What was inspected

- `gyre-domain/src/policy.rs` — `Policy` now carries every spec field with
  `scope_id: Option<Id>` / `created_by: Id`; `PolicyScope` {Tenant, Workspace,
  Repo}, `PolicyEffect` {Allow, Deny}, all 8 `ConditionOp` variants,
  `ConditionValue` {String, StringList, Number, Bool} (+ additive `Null`
  placeholder for `Exists`). Additive `built_in` field is required by the
  spec's §Built-In Policies and pre-existed.
- `policy_engine.rs` — dynamic references (`resolve_condition_value` /
  `resolve_dynamic_value` / `resolve_dynamic_scalar`): `$resource.*` /
  `$subject.*` resolve at evaluation time preserving the referenced type;
  unresolvable references fail the condition (fail closed — an Allow cannot
  grant on a missing attribute). Deny-before-Allow tie-break added to
  `policy_sort_key` per §Policy Language. `evaluate` injects `action`,
  `resource.type`, `env.time`.
- `abac_middleware.rs` — full attribute extraction: subject identity/global
  role/tenant from the auth extractor (insert-only JWT-claim merge so claims
  cannot override extractor-resolved identity facts); `persona`,
  `stack_hash` (normalized from `wl_stack_hash`), `attestation_level`,
  `repo_scope` from agent-JWT claims; `workspace_ids`/`team_ids`/
  `workspace_role` from the real membership/team stores; resource identity
  from the request path; entity-lookup resource attributes (`tenant_id`,
  `owner`, `team`, `approval_status`, `visibility` where carried) via real
  store lookups through the repo→workspace→tenant scope chain, with no
  fabricated defaults (missing lookups leave attributes unset → conditions
  fail closed); `env.ip` from `ConnectInfo` only (CWE-348; `main.rs` now
  serves with connect-info); `env.budget_remaining` from the real budget
  system; `env.main_health` from the real merge-queue pause state.
- `auth.rs` — `persona`/`attestation_level` claims, `mint_scoped` for
  worker JWTs so `subject.repo_scope` is real on every spawned-agent request;
  all `mint_orchestrator` call sites (orchestrator.rs, stale_agents.rs,
  mcp.rs) updated.
- `sqlite/policy.rs` + `postgres/policy.rs` — Id-typed field mapping; the
  port contract "delete returns an error for built-in policies" is now
  enforced in code in BOTH adapters (mem guard already existed), satisfying
  `check-mem-port-contracts.sh`. Policies table migrations pre-exist
  (000007 + 000028); adding a new one would have been wrong.
- Exemption-file edits are line-number shifts only — no new entries.

## Probes run (logs in /tmp/stage/review-evidence)

- `cargo test -p gyre-server --lib policy_engine` — 31/31 PASS
- `cargo test -p gyre-server --lib abac_middleware` — 23/23 PASS
- `cargo test -p gyre-server --lib auth::tests` — 36/36 PASS
- `cargo test -p gyre-server --lib api::policies` — 12/12 PASS
- `cargo test -p gyre-adapters --lib sqlite::policy::tests` — 4/4 PASS
- **Discrimination probe:** temporarily disabled the two new production
  behaviors (`normalize_agent_jwt_attributes` call and
  `extract_entity_attributes` call) → 7 of the new middleware tests FAIL
  (claim normalization, entity owner/approval_status/tenant lookups,
  scope-chain resolution). Restored the file; `git status` clean, HEAD
  unchanged at the candidate; suite re-run 23/23 PASS. The tests are not
  self-confirming.
- Mechanical checks all PASS at the candidate: check-arch,
  check-mem-port-contracts, check-migration-versions,
  check-forwarded-header-trust, check-inert-enforcement,
  check-fabricated-scope-defaults, check-scope-literal-defaults,
  check-abac-exempt-handlers, check-abac-route-registry,
  check-dead-message-kinds, check-lossy-secret-conversion,
  check-relative-path-defaults, check-mcp-write-tools.
- All 12 commits listed in the task's `commits:` frontmatter are ancestors
  of the candidate.

## Notes

- The resumed-run fix to `spec_resource_tenant_resolves_via_scope_chain`
  (cross-tenant Deny at 810 instead of a same-tenant Allow) is correct and
  makes the test genuinely discriminating: only a real scope-chain lookup
  (tenant t-1 ≠ "default") fires the deny; a fabricated tenant falls through
  to the builtin Allow at 800 and the test would fail.
- `agent_jwt_persona_and_attestation_level_reach_live_evaluation` passes via
  raw claim merge even with `normalize_agent_jwt_attributes` disabled
  (persona/attestation_level merge raw as well as being normalized); the
  test still proves the attributes reach live evaluation through the
  discriminating-Deny sandwich, so it is not weakened by this overlap.
- Full `cargo test --all` and CI remain the verification stage's gate, per
  the shipped note; this review ran the focused suites above.
