# Review — task-128 (ABAC policy engine core: entity, conditions, attribute model)

Spec: `abac-policy-engine.md` §Core Concepts, §Attributes, §Policy Language,
§Policy Entity, §Conditions, §Policy Examples.
Base `d81b0fe4` → candidate `6fbd423c` (implementation checkpoint `6df5dd5c`,
process commit recording the frontmatter commit list).

## Round 1 — approved

Independent evidence (probe logs under `/tmp/stage/review-evidence/`):

- **Domain types match the spec exactly** (`gyre-domain/src/policy.rs`):
  `Policy` carries every specced field (incl. `scope_id: Option<Id>`,
  `immutable`, `created_by: Id`, `conditions`, plus the `built_in` flag the
  spec's Built-In Policies section requires); `PolicyScope` =
  {Tenant, Workspace, Repo}; `PolicyEffect` = {Allow, Deny}; `ConditionOp`
  has all 8 variants; `ConditionValue` = {String, StringList, Number, Bool}
  (+ `Null` as the value-side placeholder for `Exists`).
- **Dynamic references are real and fail closed** (`policy_engine.rs`
  `resolve_condition_value`/`resolve_dynamic_value`/`resolve_dynamic_scalar`):
  `$resource.*`/`$subject.*` resolve against the live evaluation context
  preserving the referenced attribute's type (string/list/number/bool);
  unresolvable references fail the condition, so an Allow with an unresolvable
  reference cannot grant. Behavioral tests cover resource-ref, subject-ref,
  list-element refs, numeric refs, and unresolvable-ref fail-closed.
- **Evaluation order matches the spec**: immutable Deny evaluated before all
  priority-based evaluation; remaining policies sorted by
  (priority desc, scope specificity repo>workspace>tenant, Deny before Allow);
  first match wins; default deny. All five §Policy Examples are encoded as
  verbatim evaluation tests (`spec_example_agent_repo_scope` including its
  `$resource.repo_id` reference, `persona_management`, `viewer_no_spawn`,
  `gate_approved_persona`, `default_deny`).
- **Attribute extraction is real, not structural** (`abac_middleware.rs`):
  subject identity from the auth extractor with insert-only JWT-claim merging
  (a claim cannot override extractor-resolved `subject.type`/`id`/
  `global_role`/`tenant_id` — behavioral test `merge_jwt_claims_never_overrides_
  existing_attributes`); `persona`/`stack_hash` (normalized from
  `wl_stack_hash`)/`attestation_level`/`repo_scope` from agent JWT claims
  (`mint_scoped` now mints real workspace/repo scope, persona, and
  attestation-level claims at spawn; `main.rs` wires
  `into_make_service_with_connect_info` so `env.ip` comes from the socket
  peer — no forwarded-header reads anywhere in the diff); membership-sourced
  `workspace_ids`/`team_ids`/`workspace_role` from the real membership/team
  stores; entity-lookup resource attributes (`resource.tenant_id`, `owner`,
  `approval_status`) from real store lookups per resource type (spec ledger
  with percent-decoded path, personas, tasks, agents, MRs, teams) with
  repo→workspace→tenant scope-chain tenant resolution and no fabricated
  defaults; `env.budget_remaining` from the real BudgetConfig/BudgetUsage
  system; `env.main_health` from the real merge-queue pause state
  (`merge_processor::queue_health`).
- **Port + adapters**: `PolicyRepository` (create/find/list/list_by_scope/
  update/delete/prefix-deletes/record_decision/list_decisions) unchanged and
  complete; SQLite, Postgres, and mem adapters updated consistently for the
  `Id`-typed `scope_id`/`created_by`; the port's built-in delete guard is now
  enforced in code in the SQLite and Postgres adapters (mem already had it);
  `check-mem-port-contracts.sh` passes. The `policies` table pre-exists
  (migrations 000007 + 000028) and covers the entity — correctly reused
  rather than duplicated (duplicate migration versions never run).
- **Tests are discriminating, not self-confirming**: middleware tests run
  live tower oneshot requests against real stores with both-direction
  assertions (e.g. `env_budget_remaining_from_real_budget_system` asserts OK
  at 600 remaining and 403 at 50; `env_main_health_from_real_forge_state`
  flips red/green; persona/spec owner+approval tests flip on approval state).
  The rewritten `spec_resource_tenant_resolves_via_scope_chain` (cross-tenant
  Deny at 810, `resource.tenant_id NotEquals $subject.tenant_id`) genuinely
  discriminates a real scope-chain lookup from a fabricated "default": only
  tenant t-1 ≠ "default" makes the deny fire; a fabricated/missing tenant
  falls through to the builtin Developer Allow → 200.
- **Mechanical invariants**: arch, abac-route-registry, abac-exempt-handlers
  (88 handlers), mcp-write-tools, migration-versions, dead-message-kinds,
  byte-slice-truncation, inert-enforcement, mem-port-contracts,
  forwarded-header-trust, in-memory-state-stores, unbounded-external-http,
  migration-sql-portability, forged-scope-fields, fail-open-ref-resolution,
  relative-path-defaults, lossy-secret-conversion,
  fabricated-scope-defaults, task-commit-attribution — all pass. Exemption
  files differ from base by line-number shifts only (no growth).
- **Prior durable findings resolved at this head**: repo-wide conflict-marker
  scan clean; the corrupt `elk.bundled-C7RdDx88.js` (unresolved nested merge
  conflict shipped as a production asset) is gone — `web/dist` was rebuilt
  from source byte-identical to base (`web/src`, `package.json`,
  `package-lock.json`, `vite.config.js` empty diff vs `d81b0fe4`); both
  bundles pass `node --check`, `index.html` references match on-disk assets,
  and the earlier exemption-file conflicts (`lossy-secret-conversion`,
  `fabricated-scope-defaults`) are absent — both checks pass.

Infrastructure restrictions (recorded for verification, not code defects):

- **crates.io is unreachable in this review sandbox** (`static.crates.io`
  blackholed at 198.18.0.7; 137/399 locked crates cached; no vendor dir or
  prebuilt target). Rust compile/test probes were impossible; deferred:
  `cargo test -p gyre-server --lib policy_engine`, `... abac_middleware`,
  `... auth`, `... api::policies`, `cargo test -p gyre-adapters --lib
  sqlite::policy`, full `cargo test --all`, and `cd web && npm ci && npm test`
  (web/src is unchanged from base; the prior run's single
  `ExplorerCanvas-performance.test.js` jsdom wall-clock failure was proven
  environmental by failing identically at pristine base). Independent
  syntax-level evidence: every candidate-changed `.rs` file parses under
  `rustfmt --edition 2021 --check` (formatting-only diffs, matching the
  repo's pre-existing drift on unchanged files).

Minor observations (pre-existing, no action required for this task):

- `check-abac-route-registry.sh` prints a duplicate-count notice (31 vs
  frozen baseline 22) but exits 0 — the script's FAIL flag is never
  consulted (pre-existing weakness). The production-code duplicate delta vs
  base is only the two `starts_with("/api/v1/workspaces/")` prefix literals
  in `extract_path_attributes` (path-matching helpers, not resolver
  entries); the rest is `#[cfg(test)]` grep noise. Same judgment as the
  prior reviews of 853ca54c/cf90f680.
- Several python-backed lint scripts are vacuous as wired (pre-commit
  `pass_filenames: false`; CI `|| true`; no default path list), and some
  exemption line numbers have drifted (e.g. `scope-literal-defaults`
  lib.rs entries, `warn-continue-creation` abac_middleware entry). The
  drift is inherited from the approved base (base entries 490/534 vs actual
  502/546; 728 vs 730) and does not change CI outcomes; not a candidate
  regression.
- One trailing whitespace at `specs/tasks/task-128.md:133` (Shipped prose,
  introduced by the checkpoint commit; the candidate tip commit itself is
  clean under `git diff --check HEAD^1 HEAD`).

Verdict: approved. `cargo test --all`, full web suite, and GitHub CI remain
verification/publication gates as the task itself records.
