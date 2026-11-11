# Review — task-128 (ABAC policy engine core: entity, conditions, attribute model)

Candidate: `7c4190578de314b7116e786f43f1487bd2314789` (base `cba416b46`,
strict descendant — all intermediate merged bases verified ancestors of the
assigned base, so the diff is purely task-128 work: 16 files, +4616/−134).
Verdict: **needs-revision** — one bookkeeping finding (F1); all production-code
probes passed and no code defect, fake implementation, or weak test was found.

## Probes run (evidence: /tmp/stage/review-evidence/, see probe-log.md)

- `cargo test -p gyre-domain` → 466 passed (exit 0).
- `cargo test -p gyre-adapters --lib policy` → 9 passed (CRUD roundtrip with
  JSON-serialized conditions/actions/resource_types, built-in delete guard,
  scope filtering) (exit 0).
- `cargo check -p gyre-server` → clean (exit 0, 5m44s).
- `cargo fmt --check -p gyre-domain -p gyre-adapters` → clean.
- `cargo clippy -p gyre-domain` / `-p gyre-adapters --lib` → exit 0 (remaining
  warnings are pre-existing, on base-era lines).
- Invariants: check-arch, check-migration-versions, check-mem-port-contracts,
  check-inert-enforcement, check-abac-route-registry → all exit 0.
- check-task-commit-attribution → exit 0 at the candidate's own frontmatter
  (the failure against the working tree is the review guard's restored
  `commits: []` contract, not a candidate defect; verified by restoring the
  candidate's file, re-running, and restoring the guard state byte-identical).
- Stub/fakery sweep of the +4616 diff lines: no `todo!`/`unimplemented!`/
  stubs; `auth.rs::test_helpers` (JWT signer) is `#[cfg(test)]`-gated.

## Verified (no findings)

- **Domain entity matches spec**: `Policy` carries every specced field
  (`scope_id: Option<Id>`, `immutable`, `conditions`, `created_by: Id`, …)
  plus `built_in` (needed by §Built-In Policies delete guard); `PolicyScope`
  {Tenant, Workspace, Repo}, `PolicyEffect` {Allow, Deny}, all 8
  `ConditionOp` variants, `ConditionValue` {String, StringList, Number, Bool}
  (+ documented `Null` placeholder for `Exists`). Port `PolicyRepository`
  (CRUD + `list_by_scope` + decision audit) with SQLite, Postgres, and mem
  adapters; the built-in delete-guard port contract is enforced in code in
  BOTH the SQLite and mem adapters (check-mem-port-contracts exit 0).
- **Dynamic references** `$resource.*`/`$subject.*` resolve at evaluation time
  preserving the referenced attribute's type; unresolvable references fail the
  condition (fail closed), with an explicit mutation-kill case
  (`subject.type NotEquals $resource.missing` in
  `dynamic_reference_unresolvable_fails_closed`) that flips under a fail-open
  resolver — the killing-test discipline the goal demands.
- **Attribute extraction is real and spoof-resistant**: subject facts from the
  auth extractor and membership/team stores; JWT-claim merging allowlisted to
  the four agent-claim names and gated on `subject.type == "agent"`
  (`merge_agent_jwt_claims` + `normalize_agent_jwt_attributes`, both
  insert-only). Roles come only from JWKS-validated tokens, so the agent gate
  is anchored in the token issuer. Killing tests:
  `workspace_role_claim_cannot_spoof_membership_attribute`,
  `human_persona_claim_cannot_define_agent_attribute`,
  `viewer_with_admin_claim_still_trips_notin_deny` — live tower oneshot with
  properly-signed spoofed-claim JWTs asserting FORBIDDEN.
- **Resource/env attributes**: parent-scoped child routes don't leak the
  parent id into `resource.id` (regression tests for repos/mr/agents parented
  routes); `:path` spec routes percent-decode into `resource.id` (invalid
  encoding → unset, fail closed); entity lookups (spec ledger, persona, task,
  agent, MR, team) populate `resource.tenant_id`/`owner`/`approval_status`
  from real stores via the repo→workspace→tenant scope chain;
  `resource.team`/`resource.visibility` are honestly unpopulated (no domain
  entity carries those fields — verified by grep; conditions on them fail
  closed). `env.ip` from ConnectInfo socket peer (never forwarded headers),
  `env.budget_remaining` from the real budget system, `env.main_health` from
  the real merge-queue pause state — each proven by a dedicated
  live-evaluation test.
- **Evaluation engine**: immutable Deny first, priority → scope specificity →
  Deny-before-Allow ordering, default deny; policy store errors degrade to
  default deny (fail closed). All five §Policy Examples encoded verbatim as
  tests with both matching and non-matching subjects.
- **Rebase repair (spawn.rs)**: the base's `launch_agent_process`/
  `AgentLaunchResult` structure kept verbatim; task-128's
  `mint_worker_scoped` (workspace/repo scope, attestation levels from the
  resolved compute target, `wl_image_hash`) re-applied at the same call site;
  `mint_orchestrator` persona threading applied consistently in
  orchestrator.rs, mcp.rs, and stale_agents.rs. `cargo check -p gyre-server`
  confirms the merged tree type-checks.

## Finding F1 (needs-revision)

**`specs/coverage/SUMMARY.md` regressed in the final merge resolution.** The
merge 2bbd0d6 resolved the SUMMARY.md conflict by taking the base's numbers
wholesale, so the candidate reports
`| abac-policy-engine.md | 19 | 2 | 0 | 17 | 0 | 0 | 0% |` while its own
`specs/coverage/system/abac-policy-engine.md` has rows 2–7 `implemented`
(6 implemented, 11 task-assigned, 35%). The branch's pre-merge SUMMARY
(f5c2a6efa) had the correct `| 19 | 2 | 0 | 11 | 6 | 0 | 35% |`. The
mechanical regenerator `scripts/update-coverage-summary.sh` ("Called by
auditor, PM, and bootstrap") was not run after the merge. **Fix: run the
script and commit.** Bookkeeping-only — it understates (never overstates)
coverage, but the summary is the artifact the auditor/PM consume, and this
task's own round-2 repair was for coverage honesty.

## Verifier must run (deferred per the 4 GiB build-budget rule)

- `cargo test -p gyre-server --lib policy_engine` (34 tests)
- `cargo test -p gyre-server --lib abac_middleware` (41 live tests)
- `cargo test -p gyre-server --test task134_review_probe` (7 tests, pre-existing)
- `cargo test -p gyre-server --lib auth merge_processor spawn` and
  `api::workspaces` suites touched by the diff
- `cargo fmt --check` / `cargo clippy` on gyre-server; `cargo test --all`
  (task acceptance criterion); web tests unaffected (no `web/` changes).
