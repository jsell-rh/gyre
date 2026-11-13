# Review — task-129: ABAC evaluation engine, API, and audit integration (round 6)

**Candidate:** `a0c6eff51ef23b9dcee4259dfac3273604ecc8be` (base `f764bc005c5ff8f442f58556a655fb9902049aa9`)
**Verdict:** NOT approved — 3 substantive findings (1 blocking, 2 serious) plus minor issues.
**Evidence:** `/tmp/stage/review-evidence/` (findings.md, cargo-check-server.log, cargo-test-domain-policy.log, mechanical-check logs).

## What was verified as sound

- **Evaluation engine** (`policy_engine.rs`): 10-step flow, immutable-Deny-first (before priority ordering, cannot be overridden), priority-DESC with scope-specificity and Deny-before-Allow tiebreaks, first-match-wins short-circuit, default deny; scope applicability (`scope_applies`) enforced inside `evaluate` so every path shares it. Spec-example and composition tests are real and have killing directions.
- **Claim discipline**: live subject attributes come from the auth extractor and the real membership/team stores; JWT claim merging is allowlisted to the four agent-claim names and gated on `subject.type == agent` (dry-run-only `merge_jwt_claims` has exactly one caller, the dry-run endpoint).
- **Reserved system identity**: mapped to `gyre-system-token` only for the genuine system principal; non-system principals presenting it are denied pre-evaluation; every display_name write path rejects the string. The old base-era middleware bypass (`is_system_principal → next.run`) was removed — the token is now allowed *by* `system-full-access` through evaluation.
- **Builtins**: all 7 table rows seeded fail-closed at startup, drift-refreshed; delete guard in all three adapters; `builtin:policy-admin-only` (860) makes policy writes Admin-only, live-tested through real middleware.
- **Caching** (`policy_cache.rs`): read-through per-scope sets, invalidate-on-successful-mutation, explicit invalidation for the one out-of-band writer (`apply_trust_transition`), `<1ms` engine benchmark over the full seeded set.
- **Decision recording** on all four production call sites (middleware, supervised-trust hold, merge-time attestation, push-time attestation), with round-5 tests that fail if `record_decision` is removed.
- Probes: `cargo check -p gyre-server --tests` clean (9m40s); `cargo test -p gyre-domain policy` 6/6; check-abac-route-registry, check-arch, check-inert-enforcement, check-mem-port-contracts, check-forged-scope-fields all pass; exemption file unchanged at frozen count.

## Findings

1. **(Blocking) agent-repo-scope containment is not enforced for re-spawned agent sessions.** `relaunch_agent_session` (ralph_loop.rs:625-641 — the Phase 7 re-spawn that runs on gate failure/loop continuation) mints the fresh agent JWT via `mint_with_workload`, which hardcodes `repo_id: None` (auth.rs:266-296). The middleware sources `subject.repo_scope` only from the JWT claim, with no fallback to the agent entity's `repo_id` spawn scope. With the attribute absent the containment Deny (750) fails its condition and `agent-scoped-access` (700) grants read/write/complete/approve tenant-wide. Every containment test mints the first-spawn shape (`mint_worker_scoped`); none covers the claim-less re-spawn shape. The §Built-In Policies invariant is therefore unenforced on a core production path while coverage row 11 claims live enforcement. Fix: entity fallback for agent subjects, or scoped re-spawn mint.
2. **(Serious) `evaluate_attestation_abac` fails open on workspace-store error** (merge_processor.rs:3901-3906, `_ => None`): `resource.tenant_id` unset on error → tenant-scoped Denies inapplicable while the Tenant/None attested Allow still grants; the result is enforced (Deny fails the merge entry, :2300-2318). The sibling `evaluate_merge_abac` propagates store errors — this path needs the same treatment.
3. **(Serious) trust: policies are mutable/deletable by id** (api/policies.rs:139-247): update guards only `built_in`/`immutable` (trust policies are neither, gyre-domain/policy.rs:491-492) and the reserved-prefix guard fires only on a new name; delete checks only `built_in`. An Admin can silently disable/delete `trust:require-human-mr-review`, desyncing `workspace.trust_level` from enforcement and bypassing the atomic `apply_trust_transition` path — defeating the server-managed-prefix invariant this task itself established on create.
4. **(Minor)** audit log-level contract (denials warn / allowances debug, §Audit Integration) unmet on the attestation/push paths (info both ways, no AbacDenied event on the enforced merge-time denial); `PolicyDecision.resource_id` records the route pattern rather than the addressed entity id; `record_decision` errors swallowed silently (`let _ =`); `global_role` fabricated `ReadOnly` default + `first()` role selection; no priority-band validation or `?limit` cap on the decisions endpoint.
5. **(Observations, pre-existing / outside this diff — not counted in the verdict):** tenant-isolation/membership builtins are structurally inert on collection routes (`GET /api/v1/tenants`, `/api/v1/repos`, `/api/v1/activity` return cross-tenant data to any ReadOnly user; handlers unchanged by this candidate and identical at base); the task-134 scoped gate-token allowlist runs after the unresolved/exempt early returns (identical at base); dry-run/effective evaluations deliberately not recorded (documented in api-reference.md).

## Verifier must run

```
cargo test -p gyre-server --lib policy_engine
cargo test -p gyre-server --lib abac_middleware
cargo test -p gyre-server --lib policy_cache
cargo test -p gyre-server --lib evaluate_merge_abac_records_decision_to_audit_trail push_attestation_abac_records_decision_to_audit_trail
cargo test -p gyre-server --lib api::policies
cargo test --all
```
