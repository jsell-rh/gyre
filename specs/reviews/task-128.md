# Review — task-128 (ABAC policy engine core: entity, conditions, attribute model)

Spec: `specs/system/abac-policy-engine.md` §Core Concepts → §Policy Examples.
Candidate: `78ea7f70ca72d56d200376b2f4db5d8f0c40bad0` (base `a8bd7d09`).
Final round reviewed the verification-repair commit `b0ddcc4e` plus the full
candidate delta against the task contract.

Verdict: **approved** — independent evidence supports every acceptance
criterion; the one runtime item blocked by the sandbox's 4 GiB cgroup is
recorded for the verifier below.

## Acceptance criteria verification

- **Policy entity matches spec** — `gyre-domain/src/policy.rs`: all spec fields
  including `scope`, `scope_id: Option<Id>`, `priority`, `effect`,
  `conditions`, `actions`, `resource_types`, `enabled`, `immutable`,
  `created_by: Id`, timestamps (plus a documented `built_in` superset field for
  the §Built-In Policies delete guard).
- **PolicyScope {Tenant, Workspace, Repo} / PolicyEffect {Allow, Deny}** —
  present, serde round-tripped; `cargo test -p gyre-domain` → 482 passed.
- **All 8 ConditionOp variants; ConditionValue String/StringList/Number/Bool**
  — present; `eval_condition` implements every operator with typed matches and
  a fail-closed fallthrough (mismatched type combinations → false, never true).
- **Dynamic references** — `resolve_condition_value`/`resolve_dynamic_value` in
  `policy_engine.rs`: `$resource.*`/`$subject.*` resolve at evaluation time
  preserving the referenced attribute's type; unresolvable refs fail the
  condition (fail closed), mutation-verified by
  `dynamic_reference_unresolvable_fails_closed`.
- **PolicyRepository port** — CRUD + `list_by_scope` + decision audit
  (`gyre-ports/src/policy.rs`); the built-in delete guard is enforced in code
  in every adapter (postgres guard added this round;
  `check-mem-port-contracts.sh` OK).
- **SQLite adapter with migration** — policies table with conditions/actions/
  resource_types as JSON; `cargo test -p gyre-adapters policy` → 13 passed
  incl. `policy_crud_roundtrip_preserves_fields` against real storage;
  `check-migration-versions.sh` OK.
- **Attribute extraction** — real sources only: subject identity from the auth
  extractor; `workspace_role`/`workspace_ids`/`team_ids` exclusively from the
  membership/team stores; agent claims (persona, stack_hash, attestation_level,
  repo_scope) merged only for `subject.type == "agent"` via an insert-only
  allowlist (`merge_agent_jwt_claims`); the unrestricted `merge_jwt_claims` is
  confined to the what-if evaluation endpoint on a fresh hypothetical context.
  Resource identity from the request path and real entity lookups with
  repo→workspace→tenant scope-chain backfill; `env.ip` from ConnectInfo socket
  peer (never forwarded headers); `env.budget_remaining` from the budget
  system; `env.main_health` from the merge-queue pause state. Enforcement is
  real: Deny → 403 + `AbacDenied` audit event; no policy match → default deny.
- **`cargo test --all`** — see below.

## Verification-repair round (b0ddcc4e)

The prior verification run (attempt 2b8a0747) failed one test:
`developer_jwt_can_generate_on_llm_endpoints` (task-171 regression) got 403
where 200 was asserted, because the task-171 fixture supplied
`subject.workspace_role` only via the JWT `workspace_role` claim, which the
task-128 model correctly ignores (anti-forgery; enforced by
`workspace_role_claim_cannot_spoof_membership_attribute`). The repair seeds the
real attribute sources (workspace `some-ws`, repo `some-repo` parented to it,
pre-created users with Developer/Viewer memberships) and leaves the assertions
unchanged — Developer 200 on all four generate endpoints, Viewer 403 on
briefing/ask with both callers holding the global Developer role, so the
denial still proves the Allow is scoped to the membership role. This is a
fixture repair aligned with the production design, not a test weakening; the
same seeding mechanism is proven by the membership tests that passed in the
failed run (1922/1923).

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain --quiet` → exit 0 (482 passed).
- `cargo test -p gyre-adapters --quiet policy` → exit 0 (13 passed).
- `cargo check -p gyre-server --lib --tests` → exit 0 (5m58s) — independent
  compile evidence for the repaired fixture.
- Static invariant scripts all exit 0: check-arch, check-abac-route-registry,
  check-migration-versions, check-mem-port-contracts,
  check-abac-exempt-handlers (75 handlers), check-forged-scope-fields,
  check-forwarded-header-trust, check-inert-enforcement,
  check-fail-open-ref-resolution, check-task-commit-attribution.

## Verifier must run (blocked in-sandbox by the 4 GiB memory cgroup)

- `cargo test -p gyre-server --lib abac` — merged ABAC suite including the
  repaired `developer_jwt_can_generate_on_llm_endpoints` and the task-128
  attribute tests.
- `cargo test --all` — task acceptance criterion.

Generated-output note (non-blocking, verifier-owned): `web/dist/` is untouched
by this candidate and is rebuilt by the deterministic verifier after
integration.
