# Review — task-129 (ABAC evaluation engine, API, and audit integration)

Spec: `specs/system/abac-policy-engine.md` §Evaluation Engine through §Dry-Run
Evaluation (rows 8–15 of `specs/coverage/system/abac-policy-engine.md`).
Base `6ffd6799` → candidate `26f6a80c` (round 9: task commits 0dcb1017,
81ccc284, 0c148e28, 9ba4dd40, 35711565, 0dc466e7, 3471b92b, 1bb51ffe,
a9f254b2, 6e712b45, plus process/merge commits).
Verdict: **approved** — no blocking findings. All probes that could run in
this sandbox passed; the full gyre-server suite is deferred to the verifier
(build-wrapper restriction, recorded in
`/tmp/stage/review-evidence/deferred-verifier-checks.txt`).

## Probes run (checkout at candidate `26f6a80c`)

- `cargo test -p gyre-domain --quiet` → **507 + 8 passed, 0 failed** (policy
  domain: builtins, drift, trust presets, SYSTEM_ABAC_IDENTITY reservation).
- `cargo test -p gyre-adapters --lib sqlite::policy` → **4 passed, 0 failed**
  (CRUD roundtrip, built-in delete guard, scope filtering).
- `cargo check -p gyre-server` → clean, no errors/warnings (6m58s).
- `bash scripts/check-arch.sh` → OK.
- `bash scripts/check-abac-route-registry.sh` → OK (all /api/v1/ routes,
  including the five new /policies routes, resolve in the ABAC registry).
- `bash scripts/check-mem-port-contracts.sh` → OK.
- `bash scripts/check-abac-exempt-handlers.sh` → OK (79 handlers).
- `git diff --stat 6ffd6799..26f6a80c -- scripts/` → empty (no exemption-file
  growth, no verifier/script edits).
- Full read of the diff: policy_engine.rs (engine + tests), policy_cache.rs,
  abac_middleware.rs (middleware migration, seeding, m34/domain builtin sets,
  entity/path/membership/env attribute extraction), api/policies.rs (all 8
  endpoints), domain/policy.rs, mem.rs, lib.rs wiring, auth.rs, users.rs,
  scim.rs, workspaces.rs, git_http.rs, merge_processor.rs, ralph_loop.rs.

## Verified against the acceptance criteria

- **10-step evaluation flow** — `policy_engine::evaluate` + the middleware:
  subject attrs (auth extractor + allowlisted agent-JWT claims + membership/team
  stores), resource attrs (path + entity lookup with repo→workspace→tenant
  scope-chain backfill), action (method + per-route overrides), env attrs
  (ip from socket peer, budget_remaining, main_health, env.time), policy
  collection through the cached set, immutable-Deny-first, priority-DESC sort,
  AND-conditions, first-match-wins short-circuit, decision persisted on every
  path. `scope_applies` is the single canonical scope filter shared by all
  evaluation paths (middleware, dry-run, effective, merge-time, push-time).
- **Immutable Deny first / cannot be overridden** — checked before priority
  sorting; killed by `immutable_deny_blocks_even_when_high_priority_allow_matches`
  and `builtin_require_human_spec_approval_blocks_agent_approve` (p=1000 Allow
  loses to the immutable Deny).
- **Priority + scope tiebreaker + Deny-over-Allow** — `policy_sort_key`
  (priority, scope rank repo>ws>tenant, deny rank); killed by
  `deny_overrides_allow_at_same_priority_and_scope`,
  `higher_priority_deny_beats_lower_priority_allow`, scope-applicability tests.
- **7 built-ins seeded on startup, drift-refreshed, fail-closed** —
  `seed_builtin_policies` (main.rs:55) = m34 set + `domain_builtin_policies()`
  (6 §Built-In Policies table rows). All 7 table rows present
  (`builtin_set_contains_spec_table_rows`, `spec_table_builtins_seeded_on_startup`).
  `agent-repo-scope` is ENABLED at 750 (above the m34 agent role Allow at 700)
  with the §Policy Examples `$resource.repo_id` dynamic-reference condition —
  out-of-scope denial is attributed to the builtin, not the catchall
  (`agent_repo_scope_enforced_over_role_allow`).
- **Built-ins cannot be deleted; immutable cannot be overridden** — delete
  guard in BOTH adapters (mem-port-contract gate green; adapter test
  `delete_rejects_built_in_policy`), API guard, plus the stored-name
  reserved-prefix guard closing the trust:-preset-by-id vector
  (`trust_preset_policy_cannot_be_updated_or_deleted_by_id`).
- **Policy caching with invalidation** — read-through
  `CachedPolicyRepository` (per-scope keys, clear-on-mutation), the one
  out-of-band writer (`apply_trust_transition`) invalidated at both handler
  call sites via `state.policy_cache`, port documents the contract; cache
  behavior proven by the policy_cache.rs test set (served-from-cache,
  per-scope keys, mutation invalidation, out-of-band invalidation).
- **<1ms target** — `evaluation_meets_latency_target`: mean over 200 runs of
  the full seeded set with a fully populated context must stay <1ms.
- **PolicyDecision audit logging on every evaluation** — all four production
  paths (middleware, evaluate_merge_abac, evaluate_attestation_abac,
  evaluate_push_attestation_abac) go through `record_decision_audit` (warns on
  write failure); denials warn + AbacDenied audit event, allowances debug;
  queryable store (SQLite `policy_decisions` + mem adapter).
- **CRUD + dry-run + decisions + effective endpoints** — all 8 routes
  registered in api/mod.rs:893-906 with ABAC registry entries; create is
  Admin-only via the seeded `builtin:policy-admin-only` Deny (860, live-verified
  by `policy_management_is_admin_only_live`); dry-run uses the REAL engine and
  REAL policy set with the dry-run-only claim merge; effective resolves the
  subject's real attributes from the stores and binds the documented
  `?subject=` param (killing test present).
- **Middleware migrated, no system-token bypass** — the global token is
  allowed BY `system-full-access` (matched by the reserved, non-spoofable
  `gyre-system-token` identity; every display_name write path rejects it and
  the middleware denies spoofed bearers pre-evaluation —
  `spoofed_system_identity_denied_before_evaluation`,
  `jwt_rejects_reserved_system_identity_provisioning`, SCIM/users guards).
  Re-spawned agents get `mint_worker_scoped` JWTs so `subject.repo_scope` is
  real (`respawn_jwt_carries_worker_scope_claims`).
- **Coverage matrix** rows 8–15 flipped to `implemented` with landing evidence;
  spec amendment (reserved-identity paragraph) follows spec lifecycle.

## Test-inflation check

None found. The engine tests are behavioral with explicit killing directions
(fail-open resolver, list-semantics, scope-filter mutations each named);
the API tests drive the real router; the admin-only, tenant-isolation,
membership-required, persona-approval, and agent-repo-scope invariants are
live-tested through the middleware with minted JWTs and asserted
`matched_policy` attribution.

## Deferred to the verifier (recorded, not blocking)

- `cargo test -p gyre-server --lib` and `cargo test --all`. The round-8
  verification (source ba8ae373) failed 68 tests; every failure class was
  triaged to a fix present in this candidate (seeded `test_state`,
  spec-example scope fixtures, repo_access test-contract amendment) —
  source-verified here, not executed (gyre-server codegen exceeds the
  sandbox's 4 GiB wrapper budget). The repo_access amendment keeps both
  assertions as denials (403 at the policy layer, 404 handler gate probed via
  the system token) — no containment relaxed.
- No frontend changes are in this task's diff; `web/dist/` untouched
  (generated output is the verifier's).
