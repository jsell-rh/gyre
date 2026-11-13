# Review — task-129 (ABAC evaluation engine, API, and audit integration)

Spec: `specs/system/abac-policy-engine.md` §Evaluation Engine–§Dry-Run Evaluation.
Candidate under review: `3bddd641` (branch `pipeline/task-129/360234bd55324aa8a153b58f9eae0d17-1`, base `d9c419a0`).
Product commits: `0dcb1017`, `81ccc284`, `0c148e28`, `9ba4dd40`, `35711565` (all recorded in the task frontmatter).

## Round 5 (2026-11-13) — round-4 scope-applicability repair verified; one evidence-integrity finding remains

Verdict: **needs-revision** (one finding). Evidence: `/tmp/stage/review-evidence/probe-log.md`
(probe commands + exit codes), `deferred-verifier-probes.md` (gyre-server tests the verifier
must run — the executor wrapper blocks server test builds in the review sandbox;
`cargo check -p gyre-server --tests` passes, exit 0).

Probes run: `cargo test -p gyre-domain policy` (pass), `cargo check -p gyre-server` and
`--tests` (pass), `check-arch`, `check-abac-route-registry`, `check-mem-port-contracts`,
`check-in-memory-state-stores`, `check-scope-literal-defaults`, `check-inert-enforcement`,
`check-forwarded-header-trust`, `check-fail-open-ref-resolution`,
`check-lossy-secret-conversion`, `check-fabricated-scope-defaults`,
`check-byte-slice-truncation` (all pass), `check-task-commit-attribution` (passes against
the candidate's committed task file — the worktree failure is the executor's contract
restoration, not the candidate; all five frontmatter SHAs resolve).

Verified fixed from all prior durable findings:

- **R1 (agent-repo-scope hollow + `?subject=` binding):** the builtin is now seeded
  enabled with the §Policy Examples semantics (`subject.type == agent` AND
  `subject.repo_scope not_equals $resource.repo_id`, dynamic reference resolved at
  evaluation time, unresolvable → condition fails closed) at priority 750, above the m34
  agent-scoped-access role Allow (700) so higher-priority-wins cannot override the
  containment. Live test `builtin_agent_repo_scope_denies_out_of_scope_agent_live`
  (own repo 200 / sibling repo 403, denial attributed to `builtin-agent-repo-scope` via
  the decision audit). `/policies/effective` binds the documented `?subject=` param
  (`subject_id` alias) with a binding test.
- **R2 (system-full-access spoof + dead mitigation):** `SYSTEM_ABAC_IDENTITY` is reserved
  on every display_name write path (PUT /users/me, admin creation, SSO provisioning —
  including existing-row login rejection — and SCIM create/update, each with tests); the
  middleware denies any non-system principal presenting the identity before evaluation
  (`spoofed_system_identity_denied_before_evaluation`); the genuine global token now flows
  through evaluation with `subject.id` mapped to the reserved identity and is allowed BY
  `system-full-access` at 1000 > tenant-isolation 950, with the policy-match provenance
  asserted in the decision audit (`system_token_allowed_by_system_full_access_policy`,
  `builtin_tenant_isolation_denies_cross_tenant_live`). The former early bypass is gone.
- **R3 (scope applicability + stale comments):** one canonical `scope_applies` filter now
  lives inside `evaluate()` — `scope_id: Some` requires the addressed resource's
  scope-chain attribute to be present AND equal; `None` means all at that level (keeps
  every seeded builtin, all Tenant/None, global). All evaluation call sites verified to
  carry the caller-resolved scope chain: middleware (path + entity lookup, repo →
  workspace → tenant, no fabricated scopes), merge path (tenant from the repo's
  workspace, never the caller's claim), attestation gate, git-http push ctx, dry-run
  (`resource.tenant_id` accepted), effective endpoint (optional scope query params).
  Engine unit tests (workspace/repo/tenant non-applicability, None-means-all,
  immutable-Deny composition) plus live two-leg middleware tests in both directions with
  decision-audit `matched_policy` assertions and stated killing directions, and a dry-run
  API scope test. Stale auth.rs/git_http.rs bypass comments corrected.

Independently confirmed clean this round: policy-set caching (read-through
`CachedPolicyRepository` wired in both `build_state` and `test_state`; port mutations
invalidate on success; the one out-of-band writer `apply_trust_transition` invalidated
explicitly at both handler call sites, port documents the contract; `find_by_id` passes
through for seeding drift detection); admin-only policy management via the seeded
`builtin:policy-admin-only` Deny at 860 over the Developer blanket Allow at 800 (live
test); built-in delete/update guards in both adapters; agent JWT claim allowlist
(persona/stack_hash/attestation_level/repo_scope only, agent-type-gated, insert-only);
membership boolean fail-closed on store error; immutable-Deny-first with the
`<1ms` latency benchmark over the full seeded set; `merge_jwt_claims` (arbitrary claims)
confined to the dry-run endpoint.

Findings:

- [-] **F1 (evidence integrity / §Audit Integration): coverage row 13 cites decision
  recording on paths that do not record.** The note claims decisions are "persisted on
  EVERY evaluation — middleware requests, merge-time (evaluate_merge_abac), and push-time
  (git_http) all call record_decision". In the candidate, `evaluate_merge_abac`
  (merge_processor.rs:3693, the supervised-trust hold predicate) never calls
  `record_decision` (the only merge-path call site is inside `evaluate_attestation_abac`,
  merge_processor.rs:3917) and the git_http push-time attestation evaluation
  (git_http.rs:1141) never calls it either (tracing::info, "audit-only" Phase 1 — a
  pre-existing task-061 path). Both behaviors are unchanged from base, so this is not a
  regression, but the acceptance criterion "PolicyDecision audit logging on every
  evaluation" and the spec's "Every policy evaluation is logged ... All decisions are
  queryable via the audit API" are not literally met for those two evaluation paths, and
  the coverage matrix justifying the §Audit Integration implemented flip cites call
  sites that do not exist. Repair: either persist `PolicyDecision` records for the
  supervised-trust-hold and push-time evaluations, or correct the coverage note (and the
  shipped claim) to state exactly which evaluation paths record, reconciled with the
  acceptance criterion. Repro: `grep -n record_decision crates/gyre-server/src/merge_processor.rs`
  (only :3917, in evaluate_attestation_abac); `grep -c record_decision
  crates/gyre-server/src/git_http.rs` (0).

Test-inflation check: none found — the round's new tests assert both directions
(out-of-scope non-effect and in-scope effect) through the live router and the decision
audit trail, with killing directions documented in the test comments; the engine unit
tests fail on the pre-fix filter and on an Exists-condition or disabled seed.

Server tests (all compile under `cargo check -p gyre-server --tests`) are enumerated for
the verifier in `/tmp/stage/review-evidence/deferred-verifier-probes.md`; the full
`cargo test --all` gate remains the verifier's to run.
