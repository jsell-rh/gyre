# Review — task-128 (ABAC policy engine core: entity, conditions, attribute model)

Candidate: `8a989715fc25319221dae501967967191a19616a` (base `e402e1ed620c0407906528548621734d776d43a6`).
Scope: `abac-policy-engine.md` §Core Concepts–§Policy Examples (coverage rows 2–7), including the
final rebase-repair round (merge `ef27e8b1f` + `35f090dfa`).

Verdict: **approved** — no findings. Evidence below; server-suite execution deferred to the
verifier per the 4 GiB build budget (cgroup `memory.max` = 4 GiB confirmed), with the required
probe list recorded.

## Probes run (this review, independent)

- `bash scripts/{check-arch,check-migration-versions,check-abac-route-registry,
  check-abac-exempt-handlers,check-mem-port-contracts,check-inert-enforcement,
  check-dead-message-kinds,check-byte-slice-truncation,check-relative-path-defaults,
  check-fail-open-ref-resolution,check-fabricated-scope-defaults,
  check-lossy-secret-conversion,check-scope-literal-defaults,
  check-forwarded-header-trust,check-in-memory-state-stores,
  check-unbounded-external-http,check-migration-sql-portability,
  check-forged-scope-fields,check-mcp-write-tools,check-task-commit-attribution}.sh`
  → 19/20 exit 0. The single failure (`check-task-commit-attribution.sh`) is a working-tree
  artifact: the review harness reset `specs/tasks/task-128.md` to the assigned contract
  (`commits: []`) after checkout. The COMMITTED candidate carries the full list
  (35f090dfa, 198830867, 16e740051, 7502a9eec, 5327b8daa, 8d5494e94, d779f1e89, 29ad56fb6),
  covering every task-labeled product commit — verified via `git show HEAD:specs/tasks/task-128.md`.
  Verifier should re-run this gate on the committed tree.
- `cargo test -p gyre-domain` → 8 passed, 0 failed (includes the trust-policy field-level tests).
- `cargo test -p gyre-adapters` (full) → **466 passed, 6 ignored, 0 failed** (matches the shipped
  claim exactly; includes sqlite policy CRUD/scope/delete-guard tests).
- `cargo check -p gyre-server --tests` → exit 0, zero warnings (type-checks all middleware /
  policy_engine / repo_access / spawn test code).
- `cargo clippy -p gyre-domain -p gyre-ports` → no warnings in task-touched files (the 263
  `double_must_use` warnings are pre-existing `async_trait` macro noise in gyre-ports, untouched
  by this diff).
- Diff inspection base→candidate for all 17 changed files; conflict-marker and
  `todo!`/`unimplemented!` sweeps over the four core files: clean.

## Acceptance criteria verification

- **Policy entity**: every spec field present (`gyre-domain/src/policy.rs`), plus `built_in`
  (needed by §Built-In Policies delete guard — disclosed in coverage notes). `scope_id:
  Option<Id>`, `created_by: Id`, `immutable` all match.
- **PolicyScope / PolicyEffect**: exact variants, serde snake_case.
- **ConditionOp**: all 8 variants. **ConditionValue**: String/StringList/Number/Bool (+ `Null`
  as the `Exists` value-side placeholder — disclosed; untagged serde order is correct).
- **Dynamic references**: `$resource.*`/`$subject.*` resolve at evaluation time preserving the
  referenced attribute's type (`resolve_condition_value` / `resolve_dynamic_value`); unresolvable
  references fail the condition (fail closed). The fail-closed property is mutation-verified by
  `dynamic_reference_unresolvable_fails_closed`, whose second case (`subject.type NotEquals
  $resource.missing`) flips to Allow under a fail-open/empty-string-fabricating mutation — a
  genuine killing case, not a mirrored-logic assertion.
- **PolicyRepository port**: CRUD + `list_by_scope` + decision audit; the built-in delete-guard
  port contract is enforced in code in BOTH sqlite and mem adapters (postgres updated too);
  `check-mem-port-contracts.sh` passes.
- **SQLite adapter + migration**: `policies` table with conditions/actions/resource_types stored
  as JSON; adapter tests pass dynamically (13/13 policy tests, incl. field roundtrip, scope
  filter, delete-rejects-built-in).
- **Attribute extraction**: all four categories sourced from real systems — auth extractor
  (type/id/global_role/tenant_id), allowlisted agent-JWT claims (persona/stack_hash/
  attestation_level/repo_scope, gated on `subject.type == agent` — the round-2 spoofing fix),
  membership/team stores (workspace_role/workspace_ids/team_ids), request path (resource.id/
  workspace_id/repo_id with parent-id leak suppression), entity lookups (spec ledger, personas,
  tasks, agents, MRs, teams; scope chain repo→workspace→tenant), env from ConnectInfo socket
  peer (CWE-348), real budget system, real merge-queue pause state. `resource.team` /
  `resource.visibility` are unpopulated because no entity carries those fields — honestly
  disclosed in coverage row 3; conditions on them fail closed. Verified: the only `visibility`
  in gyre-domain is code-symbol visibility (extractors); no resource entity has a team field.
- **`cargo test --all`**: deferred to verifier (see below). Domain + adapters suites pass here;
  server tests type-check clean.

## Rebase-repair round (the delta under review this time)

`35f090dfa` restores the repo→workspace scope chain that the straight conflict resolution
dropped: `extract_entity_attributes` now calls `set_workspace_and_tenant_from_repo` on
repo-addressed routes so `resource.workspace_id` is backfilled from the addressed repo's own
workspace BEFORE the membership block resolves `subject.workspace_role` against it. Ordering
verified in `abac_middleware`: path attrs → entity attrs (scope-chain backfill) → membership →
env → evaluate. Unresolvable repo id leaves attributes unset (fail closed, logged on error).
The base's (task-121) repo-access behavior is guarded by
`api::repo_access::tests::workspace_admin_manages_overrides_through_production_abac`, which
drives the PRODUCTION router (`api_router().layer(abac_middleware)`) and asserts the Allow
decision's `matched_policy == builtin-repo-access-override-management` — it fails (403) if the
backfill regresses. The deny-side twin (`non_admin_member_cannot_manage_overrides_through_
production_abac`) is also present. Registry entries for `/repos/:id/access-overrides/:user_id`
map PUT/DELETE to the repo `admin` action; `builtin-repo-access-override-management` (Allow,
`subject.workspace_role In [Owner, Admin]`, action admin, resource repo) is seeded. All route
registry gates pass.

## Server tests the verifier must execute

- `cargo test --all` (final acceptance criterion)
- `cargo test -p gyre-server --lib abac_middleware`
- `cargo test -p gyre-server --lib policy_engine`
- `cargo test -p gyre-server --lib api::repo_access` (kills a repo→workspace scope-chain regression)
- `cargo test -p gyre-server --lib api::spawn` (attestation_level mint for no-target spawns)
- `cargo test -p gyre-server --lib git_http`, `workspace_ownership` (merge-conflict surfaces)
- `bash scripts/check-task-commit-attribution.sh` on the committed tree (working-tree reset here)

## Test-inflation check

None found. The middleware tests drive the real router via tower oneshot with real stores and
assert on policy-decision records; the engine tests include mutation-aware killing cases; the
repo_access tests assert the matched policy id, not just a status code. Dry-run
`merge_jwt_claims` is confined to the what-if endpoint (`api/policies.rs evaluate_policy`) and
documented as never-live.
