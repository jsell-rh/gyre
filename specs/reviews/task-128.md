# Review — task-128 (Implement ABAC policy engine core: entity, conditions, and attribute model)

Spec: `abac-policy-engine.md` §Core Concepts, §Attributes, §Policy Language, §Policy Entity, §Conditions, §Policy Examples.
Candidate under review: `cf90f6800ecc5cb282f14402c0956049554a008c` (base `3c39ab1f796bcf52ac0b95d60a4101e27aa69fe8`).
Verdict: **needs-revision** (one blocking finding; the ABAC core work itself is real and verified).

## Round 1

Mechanical invariants (all exit 0, logs in `/tmp/stage/review-evidence/check-*.txt`):
check-arch, check-abac-route-registry (see F2), check-mcp-write-tools,
check-migration-versions, check-byte-slice-truncation, check-relative-path-defaults,
check-fail-open-ref-resolution, check-mem-port-contracts, check-lossy-secret-conversion,
check-fabricated-scope-defaults, check-scope-literal-defaults, check-inert-enforcement,
check-forwarded-header-trust, check-in-memory-state-stores, check-unbounded-external-http,
check-abac-exempt-handlers, check-dead-message-kinds, check-migration-sql-portability.
Exemption files in the diff are line-number shifts only — no new entries.

Focused test runs (this sandbox, fresh build; logs in `/tmp/stage/review-evidence/task-1-*.txt`;
the implementer's claimed log file was absent from this fresh sandbox, so everything was re-run):

- `cargo test -p gyre-server --lib policy_engine::` → **31 passed, 0 failed**.
- `cargo test -p gyre-server --lib abac_middleware::` → **23 passed, 0 failed**.
- `cargo test -p gyre-server --lib auth::` → **36 passed, 0 failed**.
- `cargo test -p gyre-server --lib api::policies` → **12 passed, 0 failed**.
- `cargo test -p gyre-adapters sqlite::policy` → **4 passed, 0 failed**.

Verified working (no findings on the task's own scope):

- **Domain types match the spec struct exactly** (`gyre-domain/src/policy.rs`): all
  fields including `immutable`, `scope`, `conditions`, `scope_id: Option<Id>`,
  `created_by: Id`; `PolicyScope` {Tenant, Workspace, Repo}; `PolicyEffect`
  {Allow, Deny}; `ConditionOp` all 8 variants; `ConditionValue`
  String/StringList/Number/Bool (+`Null` as the value-side placeholder for `Exists`).
- **Dynamic references are real and fail-closed** (`policy_engine.rs`
  `resolve_condition_value`/`resolve_dynamic_value`/`resolve_dynamic_scalar`):
  `$resource.*`/`$subject.*` resolve against the live evaluation context
  preserving the referenced attribute's type (string/list/number/bool); an
  unresolvable reference makes the condition false — an Allow with an
  unresolvable reference cannot grant. Tested both directions
  (`dynamic_reference_resolves_resource_attr_at_eval_time` re-evaluates the
  same policy against a different resource and the outcome flips).
- **Attribute model is populated from real sources per the spec's source column**
  (`abac_middleware.rs`): subject `type`/`id`/`global_role`/`tenant_id` from the
  auth extractor; `persona`/`stack_hash`/`attestation_level`/`repo_scope` from
  agent JWT claims with `wl_stack_hash`→`stack_hash` normalization and
  insert-only claim merging (a claim cannot override an extractor-resolved
  identity fact — regression-tested); `workspace_ids`/`team_ids`/
  `workspace_role` from the membership/team stores; resource
  `id`/`workspace_id`/`repo_id` from the request path; `resource.tenant_id`,
  `owner`, `approval_status` from real store lookups per resource type (spec
  ledger, personas, tasks, agents, MRs, teams) with the scope chain
  repo→workspace→tenant; `env.ip` from the socket peer only (CWE-348;
  `main.rs` now serves with `into_make_service_with_connect_info` so the
  peer extension actually exists); `env.budget_remaining` from the real budget
  config/usage ports; `env.main_health` from the real merge-queue pause state;
  `env.time`/`action`/`resource.type` injected by the evaluator. Missing
  attributes are left unset and conditions on them fail closed; no
  `"default"` fabrication anywhere in the chain.
- **Middleware tests are discriminating end-to-end probes**, not mirrored
  logic: live `tower::oneshot` requests through `abac_middleware` with real
  membership/team/entity stores. `spec_resource_tenant_resolves_via_scope_chain`
  uses a cross-tenant Deny at priority 810 that only fires when the
  scope-chain lookup genuinely resolves tenant `t-1` — a fabricated or
  missing `resource.tenant_id` falls through to the builtin Developer Allow
  at 800 and returns 200, so the test kills exactly the fabrication failure
  mode. `membership_workspace_role_denies_write_for_workspace_viewer`,
  `membership_team_ids_populated_from_team_store`,
  `task_entity_scope_backfills_workspace_and_tenant`,
  `persona_entity_attributes_reach_live_evaluation` (owner/approval flip),
  and the env.ip/budget/main_health tests follow the same shape.
- **Evaluation order matches the spec**: immutable Deny first (before all
  priority-based evaluation), then priority with scope specificity
  (repo > workspace > tenant) and Deny-before-Allow tie-break
  (`deny_overrides_allow_at_same_priority_and_scope`), then default deny.
  All five §Policy Examples are encoded as verbatim evaluation tests
  including the `$resource.repo_id` reference in the agent-repo-scope example.
- **Port contract parity**: the built-in delete guard ("Returns an error if
  the policy is built-in") is now enforced in code in BOTH the SQLite and
  Postgres adapters (previously mem-only), with adapter tests asserting the
  rejection is real (policy survives) — closes the mem/sqlite contract
  divergence class tracked by check-mem-port-contracts.
- **Worker JWT scope is real**: `spawn.rs` now mints worker JWTs via
  `mint_scoped` carrying `workspace_id`/`repo_id` (and a compute-target-derived
  `attestation_level`), so `subject.repo_scope` exists on every agent request
  rather than only for orchestrators.

Findings:

- [x] **F1 (blocking): `web/dist/assets/elk.bundled-C7RdDx88.js` is an unresolved
  (nested) merge conflict committed as a production asset — the same defect
  recorded as a durable finding against earlier candidate 853ca54c, carried
  forward unfixed into cf90f680.** `git show cf90f680:web/dist/assets/elk.bundled-C7RdDx88.js`
  begins with `<<<<<<< HEAD:...elk.bundled-OwdVv5y8.js` /
  `<<<<<<<< HEAD:...elk.bundled-CZ1SLFkJ.js` / `========` / `>>>>>>>> 6a9739e5...` /
  `=======` / `>>>>>>> 110180fbb...`; the two conflicting sides import
  `./index-DauSjDxg.js` and `./index-D4CX8rVo.js`, neither of which exists in
  `web/dist/assets` (only `index-CLIRvdnY.js` does). The base version of the
  path is clean (0 markers), so the corruption was introduced inside the
  reviewed range (commit 853ca54c committed an unresolved rebase/merge working
  tree). The served asset is a JS syntax error and `SKIP_WEB_BUILD=1`
  deployments ship it broken. Fix: rebuild `web/dist` from current web source
  and commit the resolved bundle.
- [ ] **F2 (minor, no CI failure today): check-abac-route-registry.sh duplicate
  count is 31 vs frozen baseline 22** (prints FAIL, exits 0 — the FAIL flag is
  never consulted; pre-existing script weakness unchanged by this candidate).
  Verified the growth is not new production dead-code resolver mappings: the
  production-code-only duplicate delta vs base is just the pre-existing
  `starts_with("/api/v1/workspaces/")` prefix literals at
  abac_middleware.rs:563/581; the rest is `#[cfg(test)]` route registrations
  and request URIs the script's whole-file grep counts. Informational; will
  trip any future fix that makes the script honor FAIL.

Non-findings (checked, judged acceptable):

- Coverage matrix rows for the six assigned sections remain `task-assigned`
  rather than `implemented` — conservative pre-review labeling, consistent
  with "the coverage matrix reflects reality; it does not define it"; the
  underlying behavior is verified above.
- `resource.visibility`/`resource.team` are documented as unpopulated for
  entity types that carry no such fields today (none do), with conditions on
  them failing closed — documented at the extraction site; the spec's
  attribute table lists sources, not a mandate that every entity type carry
  every field.
- `state.policies.list().await.unwrap_or_default()` on store failure yields an
  empty policy set → default deny (403), fail-closed; acceptable.
- The one test the implementer's resume note describes as fixed
  (`spec_resource_tenant_resolves_via_scope_chain`) is genuinely
  discriminating in its current cross-tenant-Deny form (see above); the
  original same-tenant-Allow premise was indeed non-discriminating.

Summary: the task's own contract — Policy entity, conditions, dynamic
references, attribute extraction, port + adapters, evaluation semantics — is
implemented for real and independently verified by focused suites and
end-to-end middleware probes, all green at the candidate. The revision
blocker is unrelated to the ABAC code but is inside the reviewed diff: the
committed `web/dist` bundle is still the unresolved merge conflict from the
interrupted rebase recorded in the durable findings, shipping a broken
production asset. Rebuild and commit a clean `web/dist` (or drop the corrupted
artifact from the diff) and this is approvable.

— Reviewer, independent review round for candidate cf90f680
