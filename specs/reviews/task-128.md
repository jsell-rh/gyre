# Review — task-128 (ABAC policy engine core: entity, conditions, and attribute model)

Spec: `specs/system/abac-policy-engine.md` §Core Concepts, §Attributes, §Policy Language, §Policy Entity, §Conditions, §Policy Examples.
Candidate under review: `853ca54c4976723a97889fad0e1ed207d7a41982` (base `110180fbb248d6fa408492c0f9a3ec4a5a724372`). All 12 commits listed in the task's `commits:` frontmatter are ancestors of the candidate and new relative to base; `check-task-commit-attribution.sh` passes.
Verdict: **needs-revision** — the task's own implementation is real and verified below, but the assigned candidate commit ships unresolved merge-conflict markers in three files, two of which fail frozen-baseline mechanical invariants (pre-commit hooks).

## Round 1

### Verified working (independent evidence, logs in /tmp/stage/review-evidence/task-128-review-evidence.txt)

Focused suites at the candidate (`/tmp/stage/bin/cargo`, `GYRE_PIPELINE_STAGE=review`):

- `cargo test -p gyre-server --lib policy_engine` → 31/31 (all five §Policy Examples encoded verbatim, incl. `spec_example_agent_repo_scope` with its `$resource.repo_id` dynamic reference; immutable-deny precedence; priority/scope/deny composition; insert-only JWT-claim merge).
- `cargo test -p gyre-server --lib abac_middleware` → 23/23, including the reworked `spec_resource_tenant_resolves_via_scope_chain` (cross-tenant Deny at 810 vs builtin Developer Allow at 800 — the discriminating form described in the resume note).
- `cargo test -p gyre-adapters --lib sqlite::policy` → 4/4 (CRUD roundtrip preserves every field incl. `scope_id: Option<Id>`/`created_by: Id`; built-in delete guard rejects and the row survives; `list_by_scope`; `delete_by_name_prefix_and_scope_id`).
- `cargo test -p gyre-server --lib api::policies` → 12/12.

Mutation probes (production behavior temporarily disabled, then restored — working tree clean afterwards):

- Disabling `extract_entity_attributes` (early return) → 5 middleware tests fail (spec entity attrs, approval_status, scope-chain tenant, task backfill, persona attrs). The entity-lookup extraction is genuinely exercised, not mirrored by tests.
- Disabling `$ref` resolution in `resolve_condition_value` (treat as literal) → 5 engine tests fail, including the spec-example test. Dynamic references genuinely resolve at evaluation time; unresolvable references fail closed.

Contract checks against the task's acceptance criteria:

- `Policy` struct carries every spec field (incl. `immutable`, `scope`, `conditions`) plus `built_in` (required by §Built-In Policies); `PolicyScope` {Tenant, Workspace, Repo}; `PolicyEffect` {Allow, Deny}; all 8 `ConditionOp` variants implemented in `eval_condition`; `ConditionValue` String/StringList/Number/Bool (+`Null` as the `Exists` value-side placeholder — additive, not a contradiction).
- Attribute model: subject type/id/global_role/tenant_id from auth context; persona/stack_hash (`wl_stack_hash` normalized)/attestation_level/repo_scope from agent JWT claims; workspace_ids/team_ids/workspace_role from membership/team stores; resource id/workspace_id/repo_id from the path; tenant/owner/approval_status from real store lookups per resource type through the repo→workspace→tenant scope chain; env.ip from socket peer (CWE-348 discipline); env.budget_remaining from the real budget system; env.main_health from real merge-queue pause state; env.time/action/resource.type injected by the evaluator. Claims cannot override extractor-set identity facts (insert-only merge, tested).
- `PolicyRepository` port unchanged in shape (CRUD + scope listing + decisions); built-in delete guard now enforced in code in SQLite, Postgres, and mem adapters (port contract). No new migration (policies table pre-exists as `000007_platform_entities` + `000028_policy_immutable`; conditions/actions/resource_types as JSON) — correct, since adding a duplicate-numbered migration is itself an invariant violation.
- `bash scripts/check-arch.sh`, `check-abac-exempt-handlers.sh`, `check-scope-literal-defaults.sh`, `check-inert-enforcement.sh`, `check-relative-path-defaults.sh`, `check-task-commit-attribution.sh` → all OK.

### Findings (blockers)

The final checkpoint commit `853ca54c` committed an unresolved rebase/merge working tree (the recorded interrupted merge of `c76ab637` onto `ed57c72` — the conflict files match exactly). The 12 task-attributed commits do not contain conflict markers; the markers exist only at the candidate head. Three files carry them:

1. **`web/dist/assets/elk.bundled-C7RdDx88.js` (corrupted production asset).** The committed bundle begins with six conflict-marker lines (`<<<<<<< HEAD:...OwdVv5y8.js`, nested `<<<<<<<< HEAD:...CZ1SLFkJ.js`, `========`, `>>>>>>>> 6a9739e5...`, `=======`, `>>>>>>> 110180fbb...`). The two conflicting contents import `./index-DauSjDxg.js` and `./index-D4CX8rVo.js`, neither present in `web/dist/assets` (only `index-CLIRvdnY.js` exists). The served asset is a JS syntax error; `SKIP_WEB_BUILD=1` deployments ship it broken. Base version of the same path is clean. Fix: rebuild web/dist from current web source and commit the resolved bundle (a fresh `npm run build` in this sandbox produced a clean file).
2. **`scripts/lossy-secret-conversion-exemptions.txt` (frozen baseline broken).** Conflict markers at lines 6–9 with a stale entry for `api/spawn.rs:681` — that site now uses `String::from_utf8` with skip-and-warn, so the entry is obsolete. `bash scripts/check-lossy-secret-conversion.sh` → exit 1, "grew to 1 entries (baseline: 0)". This is a pre-commit hook; the candidate fails the repo's mechanical invariants. Fix: take the base side (no entry).
3. **`scripts/fabricated-scope-defaults-exemptions.txt` (frozen baseline broken).** Conflict markers at lines 8–12 with stale entries for `api/spawn.rs:663` and `constraint_check.rs:1357` — both sites already implement the no-fabrication fix. `bash scripts/check-fabricated-scope-defaults.sh` → exit 1, "grew to 7 entries (baseline: 5)". Also a pre-commit hook. Fix: take the base side (no entries).

### Minor (non-blocking)

- `check-abac-route-registry.sh` prints "FAIL: 31 duplicate path entries (baseline: 22)" but exits 0 — its `FAIL=1` (set at script line 85) is never consulted; that script weakness is pre-existing. The count growth is grep noise from this candidate's new `#[cfg(test)]` route registrations/request URIs plus the `starts_with("/api/v1/workspaces/")` prefix literal; no new production resolver duplicates were added (verified by diffing production-code-only duplicates against base). No action required for this task; noted so a future script fix doesn't retro-trip on it.
- The coverage matrix (`specs/coverage/system/abac-policy-engine.md`) still lists task-128's six sections as `task-assigned`. Consistent with the review stage — flip to implemented/verified only when the section actually ships (per GOAL.md, progress is measured by shipped behavior, and the candidate as it stands cannot ship).

### Deferred to verification

- Full `cargo test --all` and CI (the candidate will fail pre-commit/CI on findings 2–3 until the conflicts are resolved).
- This sandbox cannot bind a TCP listener (per the implementer's capabilities note); no live HTTP server probe. Middleware behavior is covered by the in-process tower oneshot tests plus the mutation probes above.

### Verdict

**needs-revision.** The ABAC core implementation satisfies the task contract (entity, conditions, attribute model, dynamic references, port/adapter, extraction — all proven with discriminating tests), but the assigned candidate commit itself is not shippable: it contains unresolved merge-conflict markers in two frozen exemption files (both mechanical invariant checks exit 1) and a corrupted committed web bundle. Resolution is mechanical (take the base side of both exemption conflicts; rebuild web/dist) and does not require re-implementing any task-128 behavior.
