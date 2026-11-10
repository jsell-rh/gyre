# Review — task-128 (ABAC policy engine core: entity, conditions, attribute model)

Spec: `specs/system/abac-policy-engine.md` §Core Concepts–§Policy Examples (coverage rows 2–7).
Candidate `a4b9d967f7fb17f0ccaafa4dd1bfce6673286fae` on base `dbf9edaeae5c8a5441bd9ff50c9dff3cec99172a` (base is an ancestor; `git diff base..candidate` is exactly the task's net work). Product commits: `29ad56fb` (core), `d779f1e89` (round 1), `8d5494e94` (round 2), `5327b8daa` (round 3) — all recorded in the task frontmatter; remaining task-128-labeled commits are process/docs/merges only.

Verdict: **complete** — approved with independent evidence; no findings.

## Probes run (evidence under `/tmp/stage/review-evidence/`)

- `cargo test -p gyre-server --lib` (cold build) → **1694 passed / 0 failed** (matches the shipped claim; includes 60 abac-focused tests).
- `cargo test -p gyre-adapters --lib policy` → **9 passed / 0 failed** (SQLite policy CRUD roundtrip, built-in delete guard, scope filtering).
- `cargo check -p gyre-cli` → clean.
- All 21 mechanically-enforced invariant scripts pass (`check-arch`, `check-abac-route-registry` incl. frozen duplicate count, `check-mem-port-contracts`, `check-abac-exempt-handlers`, `check-inert-enforcement`, `check-scope-literal-defaults`, `check-fabricated-scope-defaults`, `check-forwarded-header-trust`, `check-forged-scope-fields`, `check-migration-versions`, `check-dead-message-kinds`, `check-task-commit-attribution`, `check-mcp-write-tools`, `check-migration-sql-portability`, `check-lossy-secret-conversion`, `check-relative-path-defaults`, `check-byte-slice-truncation`, `check-in-memory-state-stores`, `check-fail-open-ref-resolution`, `check-unbounded-external-http`, `check-unwired-port-methods`).
- `git diff base..candidate -- scripts/` is **empty** — no exemption-file growth relative to base.

### Mutation probes (source restored after each; tree verified clean at `a4b9d967f`)

1. **Fail-open dynamic-reference resolver** (fabricate `""` for unresolvable `$ref` in `resolve_dynamic_value`): `dynamic_reference_unresolvable_fails_closed` FAILS (Allow granted where Deny expected) — the round-1 killing case (`subject.type NotEquals $resource.missing`) genuinely discriminates fail-closed from fail-open.
2. **Revert round-2 anti-spoofing fix** (merge ALL JWT claim keys into `subject.*`, remove the agent gate): `workspace_role_claim_cannot_spoof_membership_attribute` FAILS end-to-end (200 OK on the spoofed request vs 403 expected) — the claim-spoofing channel is real and the test kills it.
3. **Disable round-3 `:path` → decoded `resource.id`** population: `spec_resource_id_from_decoded_path_reaches_live_evaluation` FAILS (200 vs 403) — the spec-path resource identity genuinely reaches live evaluation, and the retargeted-deny control (200) proves it carries the addressed spec's path, not a constant.

## Verified against the task contract

- **Policy entity matches spec exactly** (`gyre-domain/src/policy.rs`): all fields including `scope_id: Option<Id>`, `immutable`, `created_by: Id`, `created_at`/`updated_at`; `PolicyScope` {Tenant, Workspace, Repo}; `PolicyEffect` {Allow, Deny}; `ConditionOp` all 8 variants; `ConditionValue` String/StringList/Number/Bool (+`Null` placeholder for `Exists`, documented extension). The String→Id tightening is propagated consistently through adapters, API, mem store, and tests.
- **Attributes (§Attributes)**: four categories with real sources — subject type/id/global_role/tenant_id from the auth extractor; persona/stack_hash/attestation_level/repo_scope from agent JWT claims only (allowlisted to those four names, gated on `subject.type == agent`; `wl_stack_hash` normalized); workspace_ids/team_ids/workspace_role from the membership/team stores only; resource id/workspace_id/repo_id from the request path (`{resource_type}_id` child-param precedence, parent-id suppression on parent-scoped child routes, percent-decoded `:path` for spec/meta-spec routes — invalid encoding fails closed); resource tenant_id/owner/approval_status from real store lookups (spec ledger, personas, tasks, agents, MRs, teams; repo→workspace→tenant chain); env.ip from `ConnectInfo` socket peer (never forwarded headers, CWE-348); env.budget_remaining from the real budget system; env.main_health from the real merge-queue pause state. `resource.team`/`resource.visibility` are honestly disclosed as unpopulated (no entity carries those fields yet; conditions on them fail closed) — disclosure, not fakery.
- **Dynamic references**: `$resource.*`/`$subject.*` resolve at evaluation time preserving type; unresolvable references fail the condition (Allow cannot grant) — mutation-verified.
- **Port + adapters**: `PolicyRepository` (CRUD + `list_by_scope` + decision audit) with SQLite/Postgres/mem implementations; conditions/actions/resource_types stored as JSON; built-in delete guard enforced in code in BOTH SQLite and Postgres adapters (matching the mem adapter, per `check-mem-port-contracts`); schema (migration 000007 + 000028) already covers every spec field, so no new migration was needed and none was added.
- **Evaluation**: immutable Deny first, then priority order with scope specificity (repo > workspace > tenant) and Deny-before-Allow tie-break, then default deny. All five §Policy Examples are encoded as verbatim tests with both positive and negative controls (`spec_example_*`).
- **Dry-run claim merge**: the generic insert-only `merge_jwt_claims` is used only by `api/policies.rs::evaluate_policy` (caller-supplied hypothetical attributes for what-if evaluation; grants nothing) — the live access-decision path uses only the allowlisted, agent-gated merge.
- **Merge resolutions** (fefde294, 7747376, b5a872087, dbf9edaea): no task-128 behavior regressed; the spawn.rs conflict resolution keeps both the base's `wl_image_hash` digest resolution and task-128's scoped claims (`mint_scoped` with workspace/repo/persona/attestation_level), with attestation level derived from the real compute-target type; `scripts/` untouched relative to base.
- **Coverage honesty**: rows 2–7 flipped to `implemented` with per-attribute evidence notes; SUMMARY.md mechanically regenerated.

Test-inflation check: none found — the killing tests run end-to-end through live tower oneshot requests with non-vacuous controls, and each of the three mutation probes above was independently re-verified this round to fail when the corresponding production behavior is disabled.

Scope check: no out-of-scope edits — no new routes (`api/mod.rs` untouched), no script/exemption changes, no spec rewrites; docs/coverage changes are limited to the task's own sections.
