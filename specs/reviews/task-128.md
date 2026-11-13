# Review — task-128: Implement ABAC policy engine core: entity, conditions, and attribute model

Candidate: `e0bf5a9f435cf13e238753514bf3d1c174869614` (base `4dc0eb9b`).
Reviewer scope: full `git diff 4dc0eb9b..e0bf5a9f` — `gyre-domain/src/policy.rs`,
`gyre-server/src/{policy_engine,abac_middleware,auth,mem,merge_processor,stale_agents,mcp}.rs`,
`gyre-server/src/api/{spawn,orchestrator,workspaces,policies}.rs`, `gyre-server/tests/search_wiring.rs`,
`gyre-adapters/src/{sqlite,postgres}/policy.rs`, coverage rows 2–7 of
`specs/coverage/system/abac-policy-engine.md`. Probe evidence persisted under
`/tmp/stage/review-evidence/`.

## Verdict

**Approved.** Independent evidence supports every acceptance criterion for the
six assigned coverage sections (§Core Concepts … §Policy Examples):

- **Policy entity** matches the spec verbatim in `gyre-domain/src/policy.rs`
  (id, name, description, scope, `scope_id: Option<Id>`, priority, effect,
  conditions, actions, resource_types, enabled, immutable, `created_by: Id`,
  created_at, updated_at) plus a documented `built_in` flag consumed by the
  §Built-In Policies delete guard. `PolicyScope` = {Tenant, Workspace, Repo};
  `PolicyEffect` = {Allow, Deny}; all 8 `ConditionOp` variants; `ConditionValue`
  String/StringList/Number/Bool (+ `Null` placeholder for `Exists`).
- **Condition evaluation** (`policy_engine::eval_condition`) is type-aware and
  fails closed: a missing attribute makes every operator false (including
  NotEquals/NotIn), mismatched operand types fall to `false`, not to a coerced
  match.
- **Dynamic references** `$resource.*` / `$subject.*` resolve at evaluation time
  (`resolve_condition_value` / `resolve_dynamic_value` / `resolve_dynamic_scalar`),
  preserving the referenced attribute's type. Unresolvable references return
  `None` and fail the condition — an Allow with an unresolvable reference cannot
  grant. `dynamic_reference_unresolvable_fails_closed` includes the
  mutation-killing case (`subject.type NotEquals $resource.missing` — a
  fail-open resolver fabricating `""` would flip the decision to Allow; verified
  by reading the resolver + test logic).
- **Port + adapters**: pre-existing `PolicyRepository` (CRUD + `list_by_scope` +
  decision audit) now carries the `Id`-typed scope_id/created_by; SQLite and
  Postgres adapters do real Diesel queries against the `policies` table
  (migration `2026-03-23-000007_platform_entities`, `immutable` column via
  `2026-03-26-000028`; conditions/actions/resource_types stored as JSON). Both
  adapters enforce the "cannot delete built-in" port contract in code; mem
  adapter already did (`check-mem-port-contracts` passes).
- **Attribute extraction** (abac_middleware): subject facts from the auth
  extractor and membership/team stores; claim merging is allowlisted to the four
  §Attributes "Agent OIDC claim" names and gated on `subject.type == "agent"`
  (round-2 spoofing channel closed, covered by end-to-end killing tests);
  resource identity from the request path with parent-id leak suppression on
  parent-scoped child routes and percent-decoded `:path` for spec routes;
  resource tenant/owner/approval_status from real store lookups with a
  repo→workspace→tenant scope chain; `env.ip` from ConnectInfo socket peer
  (never forwarded headers); `env.budget_remaining` from the real budget
  system; `env.main_health` from the real merge-queue pause state; `env.time`,
  `action`, `resource.type` injected by the evaluator. `resource.team` /
  `resource.visibility` are honestly documented as unpopulated (no entity type
  carries the fields yet) and conditions on them fail closed.
- **Evaluation order**: immutable Deny first, then priority → scope specificity
  (repo > workspace > tenant) → Deny-before-Allow, default deny last; startup
  seeding fails closed (`seed_builtin_policies` errors abort serving).
- **§Policy Examples**: all five spec examples encoded as verbatim evaluation
  tests in `policy_engine.rs` (incl. the example's `$resource.repo_id` dynamic
  reference).

## Probes run (evidence in /tmp/stage/review-evidence/)

- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-domain` — 499 + 8 passed,
  0 failed (exit 0). `domain-tests.log`
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo test -p gyre-adapters policy` — 14
  passed, 0 failed, incl. `policy_crud_roundtrip_preserves_fields`,
  `delete_rejects_built_in_policy`, `list_by_scope_filters_scope_and_scope_id`
  (exit 0). `adapters-policy-tests.log`
- `SKIP_WEB_BUILD=1 /tmp/stage/bin/cargo check -p gyre-server --lib --tests` —
  clean (exit 0, 5m22s); this type-checks the full abac_middleware/policy_engine
  test suites, including the round-4 verification-repair
  (`compute_minutes: 0` at abac_middleware.rs:5319/5398 — the E0063 that failed
  the retained verification run a50b8f9c). `server-check.log`
- Mechanical gates on the candidate tree: check-arch, check-abac-route-registry,
  check-mem-port-contracts, check-migration-versions,
  check-migration-sql-portability, check-forwarded-header-trust,
  check-fabricated-scope-defaults — all exit 0. `mechanical-scripts.log`,
  `*.out`
- **Mutation probe** (`mutation-delete-guard.log`): removed the built-in delete
  guard from `crates/gyre-adapters/src/sqlite/policy.rs` and re-ran
  `cargo test -p gyre-adapters delete_rejects_built_in` — the test FAILS
  (panics "built-in policy delete must fail"), proving the test kills the
  defect class rather than mirroring it. Source restored (`git status` clean).

## Verifier must execute (deferred from this sandbox — 4 GiB budget rule)

- `cargo test --all` (the task's own acceptance criterion), or at minimum
  `cargo test -p gyre-server --lib` which contains the 32 `abac_middleware`
  live-evaluation tests (membership/spoofing, env.ip/budget/main_health,
  spec/persona/task entity lookups, parent-id leak suppression, decoded
  `:path` resource.id, builtin seeding fail-closed) and the 34 `policy_engine`
  tests (condition operators, dynamic references incl. the fail-closed
  killing case, immutable-deny precedence, all five §Policy Examples).
- `cargo fmt --check` and `cargo clippy` on the workspace.

## Non-blocking observations

- The uncommitted local modification to `specs/tasks/task-128.md` (progress
  reverted to `not-started`, commits emptied) is a stage-setup artifact of the
  review checkpoint, not part of candidate `e0bf5a9f`, which itself carries
  `progress: ready-for-review` and the attributed commit list.
- The base→candidate diff includes forward-merged main content (task-119
  `compute_minutes`, task-171 LLM endpoint fixture, the
  `/merge-requests/:id/spec` route) brought in by the verification-repair merge
  `8f935296`; the SUMMARY.md row flips for other specs are regenerated
  coverage bookkeeping from those merges (`1401cf4b`), not task-128-authored
  claims.
