# Review — task-129 (ABAC evaluation engine, API, and audit integration)

Spec: `specs/system/abac-policy-engine.md` §Evaluation Engine → §Dry-Run Evaluation.
Candidate: `c4d89726cb670009a1d51ab6e4767fbf8d40d807` (base `cd58a166`, branch
`pipeline/task-129/3614455c87eb42bc8618f65211e0b9a4-2`). Verdict: **needs revision**
(1 blocking finding). Prior rounds' repairs (agent-repo-scope containment,
`?subject=` param binding, Admin-only policy management) verified good — the
blocking finding is new damage introduced by this round's seeding change.

## Probes run (evidence: /tmp/stage/review-evidence/)

- `cargo test -p gyre-domain` — 507 + 8 passed, 0 failed (exit 0).
- `cargo check -p gyre-server --tests` — exit 0 (8m01s cold; all new test code type-checks).
- `scripts/check-arch.sh`, `check-abac-route-registry.sh`,
  `check-mem-port-contracts.sh`, `check-inert-enforcement.sh` — all pass.
- gyre-server test execution is blocked by the review-stage build wrapper;
  the verifier must run the suites listed in
  `review-evidence/deferred-verifier-probes.md` (incl. the decisive
  escalation repro).

## Blocking finding — display_name → `system-full-access` privilege escalation

This round's `SEEDED_DOMAIN_BUILTINS` change newly seeds `system-full-access`
(Allow, priority 1000, actions `*`, resource_types `*`, condition
`subject.id == "gyre-system-token"`); at base the domain splice seeded only
`builtin:require-human-spec-approval`, so this policy previously never ran in
production. Chain, every link verified in source:

1. `PUT /api/v1/users/me` sets `display_name` with only a non-empty check
   (`api/users.rs` `update_me`); SSO provisioning sets it directly from the
   caller-controlled `preferred_username` claim (`auth.rs` `resolve_jwt_user`).
2. JWT/API-key user auth sets `agent_id = user.display_name`
   (`auth.rs:1242/1446/1591`).
3. `is_system_principal()` is false for these callers (`user_id` is `Some`),
   so the line-1961 bypass does not fire; the middleware sets
   `subject.id = auth.agent_id` (`abac_middleware.rs:2035`).
4. `system-full-access` matches `subject.id == "gyre-system-token"` at
   priority 1000 — above `tenant-isolation` (950),
   `workspace-membership-required` (940), and every role band — granting
   full tenant-spanning access.

So any authenticated user who renames themselves `gyre-system-token` gets
Allow-everything. The middleware comment (lines 2037-2047) claims the string is
"the reserved string no JWT/API-key subject can produce" — false; the task-099
comment at lines 1958-1960 documents the exact opposite for this auth path.
Compounding: the described mitigation is dead code — `ctx.set("subject.id",
"gyre-system-token")` (2046-2048) is unreachable because the same
`is_system_principal()` predicate already returned at 1961. In the live
middleware the seeded policy can therefore only ever be matched by a spoofed
identity; the genuine global token never reaches evaluation (the 200 in
`builtin_tenant_isolation_denies_cross_tenant_live` for `test-token` comes
from the bypass, not from the policy).

Fix direction (any one, with a killing test): key the policy on a
non-spoofable attribute set only from `is_system_principal()`; or reserve
`gyre-system-token` in `update_me` + SSO provisioning; or drop the seeding and
keep the bypass as the system-principal path.

## Verified good (no findings)

- **Engine** (`policy_engine.rs::evaluate`): immutable-Deny-first, priority
  DESC + scope tiebreaker (repo > workspace > tenant) + Deny-before-Allow at
  equal priority/scope, first-match short-circuit, default deny; dynamic
  `$ref` resolution fails closed (mutation-killed per round-1). Latency target
  covered by `evaluation_meets_latency_target` (mean < 1ms over the full
  seeded set).
- **agent-repo-scope repair** (0c148e28): seeded enabled with the §Policy
  Examples dynamic-reference conditions at priority 750 (above the m34 role
  Allow at 700); live test mints a real worker JWT and proves own-repo 200 /
  other-repo 403 attributed to `builtin-agent-repo-scope`. Seed-shape test
  kills each pre-fix defect individually.
- **Policy cache** (`policy_cache.rs`): read-through `list`/`list_by_scope`,
  invalidation on every port mutation, explicit invalidation at both
  `apply_trust_transition` call sites (`api/workspaces.rs:156,349`), port
  doc comment records the contract, mem test-state mirrors production wiring.
  `find_by_id`/`record_decision`/`list_decisions` correctly pass through.
- **API**: all 8 §API endpoints registered (`api/mod.rs:888-901`) with
  resolver entries; `builtin:policy-admin-only` (860) live-gates
  create/update/delete (dry-run `evaluate` deliberately not admin-gated — the
  spec table only marks create Admin-only); `/policies/effective` binds the
  documented `?subject=` param (alias `subject_id`) and loads real
  subject attributes from user/membership/team stores.
- **Audit**: `record_decision` on every evaluation; denials at warn +
  `AbacDenied` event, allowances at debug; decisions queryable via
  `GET /api/v1/policies/decisions`.
- **Middleware migration**: `abac_middleware` evaluates through
  `policy_engine::evaluate` on the cached set; membership boolean
  (`subject.member_of_resource_workspace`) derived from the real membership
  store and fails closed (set false) on store error.

Generated `web/dist/` was not touched by this diff; no frontend source
changes to review.
