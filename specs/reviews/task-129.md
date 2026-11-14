# Review: task-129 — ABAC evaluation engine, API, and audit integration

**Reviewer:** Independent review (round 12, post round-11 rebase repair)
**Spec ref:** abac-policy-engine.md §Evaluation Engine – §Dry-Run Evaluation
**Comparison base:** bfc36c9c03b5baddaec30f204feb18ac5981b7d2
**HEAD reviewed:** e343c029745307432020fadf7906581f8652425e
**Status:** complete — approved

---

## Scope of this round

The candidate is the previously-approved task-129 branch plus the round-11
rebase repair: merge commit 56f8bbc3 resolved the DIRTY state against main
(task-146 analytics, `fail_queue_entry` refactor), followed by the process
commit e343c029. Review focus: (a) the full base→candidate diff against the
task contract, and (b) specifically that the conflict resolution in
`merge_processor.rs` preserved the task-129 ABAC semantics while integrating
main's refactor.

## Diff survey (base → candidate, 22 files, +4856/−320)

- `policy_engine.rs`: 10-step evaluation flow — attribute extraction
  (auth context, allowlisted agent-JWT claims, membership/team stores),
  canonical `scope_applies` filter (repo→workspace→tenant scope chain),
  immutable-Deny-first, priority-DESC sort with scope-specificity and
  Deny-before-Allow tiebreakers, first-match-wins short-circuit, default
  deny. Dynamic `$resource.*` references fail closed (unresolvable →
  condition false). List-valued attributes use set semantics for
  Equals/NotEquals.
- `policy_cache.rs` (new): read-through `CachedPolicyRepository` over the
  `PolicyRepository` port; every port mutation invalidates; the one
  out-of-band writer (`apply_trust_transition`) is invalidated explicitly at
  both handler call sites via the shared `state.policy_cache` handle, and
  the port method documents the contract.
- `abac_middleware.rs`: system-token bypass REMOVED — the global
  GYRE_AUTH_TOKEN is allowed by the seeded `system-full-access` policy
  (priority 1000), with the reserved `gyre-system-token` identity made
  non-spoofable (all display_name write paths reject it; non-system
  principals presenting it are denied before evaluation; SSO provisioning
  rejects it). Role-set extraction (`subject.global_role`), derived
  membership boolean (fail-closed false on store error), concrete
  `resource.id` in the decision audit trail, warn/debug log-level contract.
- `api/policies.rs`: reserved-prefix guards on update/delete (stored name,
  not just newly supplied), real-store subject resolution for
  `/policies/effective`, documented `?subject=` param binding, resource
  scope chain on dry-run.
- `domain/policy.rs`: full §Built-In Policies table — tenant-isolation (950),
  workspace-membership-required (940), persona-human-approval (950),
  agent-repo-scope (750, enabled, `$resource.repo_id` dynamic reference,
  above the 700 role Allow so containment actually wins), plus the existing
  system-full-access / immutable require-human-spec-approval / default-deny.
- `merge_processor.rs` / `git_http.rs`: attestation ABAC gates return
  `Result` and fail closed (requeue) on store-read failures; every
  evaluation path persists a `PolicyDecision` (`record_decision_audit`)
  and emits `AbacDenied` service audit events; warn/debug contract.
- `auth.rs`, `api/users.rs`, `api/scim.rs`: reserved-identity containment
  on every provisioning/display_name path, each with killing tests.

## Round-11 rebase repair verification

- The conflict resolution in `merge_processor.rs` preserves both sides'
  invariants: `evaluate_attestation_abac` still returns `anyhow::Result`
  with requeue-on-`Err` on BOTH lookup paths (branch-head at ~2434 and
  agent-task at ~2489), and the Deny path now flows through main's shared
  `denial` block → `fail_queue_entry(..., "attestation_abac", ...)` — the
  entry still fails before the merge commit is created and now also emits
  the spec-required `merge_queue.processed` failed analytics event.
- Diff 9a01da84 (pre-merge branch head) → candidate shows ZERO changes to
  `policy_engine.rs`, `policy_cache.rs`, `abac_middleware.rs`,
  `api/policies.rs` — the reviewed implementation is untouched by the
  rebase; only `merge_processor.rs` carried conflict resolution.
- No conflict markers anywhere in the tree; `web/` untouched by this task
  (backend-only scope — correct).

## Probes run (evidence in /tmp/stage/review-evidence)

| Probe | Result |
|---|---|
| `cargo check -p gyre-server` (build wrapper) | clean, 6m47s |
| `cargo test -p gyre-domain --lib` | 510 passed, 0 failed |
| `cargo clippy -p gyre-domain` | no warnings in `policy.rs` (all pre-existing elsewhere) |
| `rustfmt --check` on all 7 changed Rust files | clean |
| `scripts/check-arch.sh` | pass |
| `scripts/check-abac-route-registry.sh` (incl. `/api/v1/policies*` mappings) | pass |
| `scripts/check-mem-port-contracts.sh` | pass |
| `scripts/check-inert-enforcement.sh` | pass |
| `scripts/check-forged-scope-fields.sh` | pass |
| `scripts/check-in-memory-state-stores.sh` | pass |
| `scripts/check-scope-literal-defaults.sh` / `-fabricated-` | pass |
| `scripts/check-lossy-secret-conversion.sh` | pass |
| `scripts/check-forwarded-header-trust.sh` | pass |
| `scripts/check-unbounded-external-http.sh` | pass |
| `scripts/check-relative-path-defaults.sh` | pass |
| `scripts/check-fail-open-ref-resolution.sh` | pass |
| `scripts/check-byte-slice-truncation.sh` | pass |

## Acceptance criteria assessment

All 16 criteria are met by real production code with live (in-process HTTP,
real JWT/API-key auth, real stores) tests asserting outcomes AND matched
policy attribution through the same decisions port the audit endpoint
queries — including `system_token_allowed_by_system_full_access_policy`,
`spoofed_system_identity_denied_before_evaluation`,
`builtin_tenant_isolation_denies_cross_tenant_live`,
`builtin_agent_repo_scope_denies_out_of_scope_agent_live`,
`builtin_workspace_membership_required_denies_non_member_live`,
`builtin_persona_human_approval_denies_agent_live`,
`policy_management_is_admin_only_live`,
`spec_table_builtins_seeded_on_startup`,
`evaluation_meets_latency_target` (mean over 200 runs of the exact seeded
set, <1ms). Seeding (`seed_builtin_policies` in main.rs) is fail-closed and
drift-refreshed. Policy decisions persist via a real Diesel insert into the
`policy_decisions` table (migration 000007).

## Deferred to the independent verifier

gyre-server test compilation exceeds this sandbox's 4 GiB cgroup budget
(established round 9; `cargo check` fits, the `--lib` test build does not).
The verifier must run, on the integrated tree:

- `cargo test --all --quiet`
- `cargo test -p gyre-server --lib policy_engine`
- `cargo test -p gyre-server --lib policy_cache`
- `cargo test -p gyre-server --lib abac_middleware`
- `cargo test -p gyre-server --lib attestation_abac`
- `cargo test -p gyre-server --lib merge_queue` (analytics event on the
  attestation-deny failure path)
- `cargo clippy --all-targets` (workspace-wide lint belongs to verification)

## Findings

None blocking. Two non-blocking observations, recorded for completeness:

1. `record_decision_audit` warns (does not fail the request) when the audit
   write fails — the spec requires logging/persistence, and a warn-visible
   dropped write with the decision still enforced is a defensible
   availability trade-off; prior rounds accepted it and the rationale is
   documented at the helper.
2. `PolicyCache.invalidate()` clears all scope keys rather than the mutated
   scope only — conservative direction (can never serve stale), with the
   rationale (tenant `scope_id: None` applies to every scope query)
   documented in the module header.
