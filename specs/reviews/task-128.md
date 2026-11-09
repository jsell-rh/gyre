# Review: TASK-128 — Implement ABAC policy engine core: entity, conditions, and attribute model

**Reviewer:** Independent review (repair round)
**Date:** 2026-11-09
**Verdict:** approved (this round; round-1 findings repaired and verified)

**Candidate under review:** `97c78554ca6db618a2edfeaea7d5e92e143b1d69` (base `4118553f107cb5176b22b94770dd6b2dcc4bd46d`)
**Commits:** `29ad56fb6` (core, via checkpoint), `d779f1e89` (round-1 review fixes), plus merge/process commits (`8ca9afafb`, `718b18404`, `6e1eef810`, `97c78554c`)
**Spec:** `abac-policy-engine.md` §Core Concepts – §Policy Examples (coverage rows 2–7)

**Verification performed this round** (evidence under `/tmp/stage/review-evidence/`):

- Read the full base→candidate diff (domain policy types, SQLite/PG adapters, `policy_engine.rs`, `abac_middleware.rs`, auth/spawn/orchestrator/stale_agents/merge_processor/mcp changes, exemption files, coverage matrix).
- `cargo test -p gyre-server --lib -- policy_engine abac_middleware` — 62 passed / 0 failed (policy_engine 34 + abac_middleware 28).
- `cargo test -p gyre-server --lib` — **1481 passed / 0 failed**.
- `cargo test -p gyre-domain -p gyre-common -p gyre-ports -p gyre-adapters --lib` — 116 + 436 + 0 + 387 passed / 0 failed / 12 ignored.
- **Mutation probe 1 (fail-open resolver):** mutated `resolve_dynamic_value` to fabricate `""` for unresolvable `$refs` → `dynamic_reference_unresolvable_fails_closed` FAILS (`Allow` vs expected `Deny` on the `subject.type NotEquals $resource.missing` killing case). The round-1 test gap is closed; the mutant no longer survives.
- **Mutation probe 2 (round-1 code defect):** removed the `resource_type != "repo"` guard on the repos path prefix in `extract_path_attributes` → `repos_parented_collection_route_does_not_leak_parent_repo_id` FAILS (403 vs 200). The repair is test-protected.
- All repo invariant scripts pass at this head: check-arch, check-abac-route-registry (incl. frozen duplicate-literal baseline), check-task-commit-attribution, check-mem-port-contracts, check-migration-versions, check-inert-enforcement, check-fabricated-scope-defaults, check-scope-literal-defaults, check-lossy-secret-conversion, check-forwarded-header-trust, check-dead-message-kinds, check-byte-slice-truncation, check-relative-path-defaults, check-fail-open-ref-resolution, check-in-memory-state-stores, check-unbounded-external-http, check-forged-scope-fields, check-abac-exempt-handlers, check-mcp-write-tools.
- Exemption-file diffs are line-number drift only (code moved); no new entries.
- Repo-wide conflict-marker scan clean; the pre-existing `agent_id == "system"` ABAC bypass is unchanged from base and documented as the superuser escape hatch.

**Round-1 findings — repair verification:**

- [x] **F1 (code): parent repo id leaking into `resource.id` on repos-parented child routes** — fixed in `d779f1e89`: the repos prefix now has the same `resource_type != "repo"` guard the workspace prefix had; a path param named `{resource_type}_id` (`:worktree_id`, `:gate_id`, `:dependency_id`) names the addressed entity wherever it appears and takes precedence over `:id`; parent-scoped collection routes fabricate no `resource.id`. Verified by source read + live end-to-end middleware tests (`repos_parented_collection_route_does_not_leak_parent_repo_id`, `repos_parented_child_route_surfaces_worktree_id_as_resource_id`, each with a non-vacuous control on fresh state) + mutation probe 2.
- [x] **F2 (test): fail-closed dynamic-reference property not provable** — fixed: `dynamic_reference_unresolvable_fails_closed` gained the `subject.type NotEquals $resource.missing` killing case; verified by mutation probe 1 that a fail-open resolver now flips the suite to FAIL.
- [x] **F3 (process): coverage rows 2–7** — flipped to `implemented` with substantive evidence notes in `specs/coverage/system/abac-policy-engine.md`; SUMMARY.md regenerated consistently.

**Independent acceptance-criteria check (not relying on the implementer's labels):**

- Policy entity carries every spec field (plus `built_in`, justified by the spec's §Built-In Policies delete guard); `PolicyScope` {Tenant, Workspace, Repo}; `PolicyEffect` {Allow, Deny}; all 8 `ConditionOp` variants; `ConditionValue` {String, StringList, Number, Bool} (+`Null` placeholder for `Exists`) — `gyre-domain/src/policy.rs`.
- Dynamic `$resource.*`/`$subject.*` references resolve at evaluation time type-preserving; unresolvable refs fail the condition (fail closed) — `policy_engine.rs::resolve_condition_value`/`resolve_dynamic_value`, mutation-verified.
- `PolicyRepository` port (CRUD, list_by_scope, decision audit) with SQLite, Postgres, and mem adapters; built-in delete guard enforced in code in all adapters (mem-port-contract check passes).
- Policies table migrations predate this candidate (`000007_platform_entities`, `000028_policy_immutable`); no new migrations added, numbering check passes.
- Attribute extraction is real: subject facts from the auth extractor with insert-only JWT-claim merging; membership/team stores for workspace_ids/team_ids/workspace_role; real store lookups per resource type for owner/team/approval_status/visibility; `env.ip` from `ConnectInfo` socket peer; `env.budget_remaining` from the budget system; `env.main_health` from real merge-queue pause state. Each proven by dedicated live-evaluation tests.
- Evaluation engine: immutable Deny first, priority → scope specificity → Deny-before-Allow ordering, first match wins, default deny — matches spec §Policy Language/§Policy Composition; all five §Policy Examples encoded as verbatim tests.

**Notes for the verifier (not defects):**

- Full `cargo test --all` (integration/e2e binaries) and GitHub CI remain verification/publication gates; this review ran the lib suites plus focused probes per the review-scope constraint (workspace-wide invocations rejected by the executor wrapper).
- `subject.id` is sourced from the auth context's identity string (`auth.agent_id` — JWT display name, agent-token id, or API-key subject), a documented modeling choice consistent with the platform-wide auth model where `agent_id` is the caller identity; `auth.user_id` is not used for `subject.id`. Not a defect against §Attributes ("source: Auth context").

**Verdict: approved.** All three round-1 findings are repaired with real, test-protected work; no new defects found in the repair-round diff.
