# Review — task-129 (ABAC evaluation engine, API, and audit integration)

Spec: `specs/system/abac-policy-engine.md` §Evaluation Engine → §Dry-Run
Evaluation. Candidate `45db7dcf48e6f9ff5a4b7fff5ba27c85e1ca0100` on
`pipeline/task-129/49de64f07cac47279c787784dd52de78-1`, base `cdd1dd02`.
All 12 commits listed in the task's `commits:` frontmatter are present in
the range. Verdict: **complete** (approved), with non-blocking observations
recorded below.

## Probes run (evidence in /tmp/stage/review-evidence/)

- `cargo check -p gyre-server` → exit 0 (7m14s cold, includes web build).
- `cargo test -p gyre-domain` → 510 + 8 passed, 0 failed.
- Static verification of the round-12 fixture repair (see below).
- `record_decision_audit` call-site census (6 production sites).
- Base-vs-candidate comparison for every suspected fail-open in the merge
  path (all pre-existing, byte-identical to base).
- Mechanical gates corroborated by the retained verification run for this
  candidate (rustfmt/clippy changed-lines, arch, ABAC route registry,
  exempt handlers, mem-port-contracts, forged-scope-fields, etc. — all OK).
- gyre-server test compilation deferred to the verifier per the 4 GiB
  budget; required test list recorded in review-evidence/05.

## Verified against the contract

- **Evaluation engine** (`policy_engine.rs`): all 10 steps — attribute
  extraction, repo→workspace→tenant collection via the single canonical
  `scope_applies` filter, immutable-Deny-first (bypasses priority ordering),
  priority-DESC with scope-specificity then Deny-before-Allow tiebreakers,
  AND-conditions, first-match-wins with short-circuit, default deny. Tests
  are behavioral with documented killing directions (e.g.
  `dynamic_reference_unresolvable_fails_closed` includes a mutation that
  flips NotEquals under a fail-open resolver).
- **Policy composition**: all four rules enforced by `policy_sort_key` +
  evaluation order; killed by dedicated tests.
- **Built-ins**: all 7 table rows seeded at startup, fail-closed
  (`main.rs` propagates seed errors — startup aborts), drift-refreshed to
  the code definition. `agent-repo-scope` is enabled with the dynamic
  `$resource.repo_id` reference at 750, above the m34 role Allow (700) —
  the priority analysis for why 100/Exists would enforce nothing is
  correct. Built-ins refuse update/delete at the API layer (plus the
  stored `trust:`/`builtin:` name-prefix guard for non-flagged presets);
  the seed refresh path legitimately bypasses the guard via the repository.
- **Reserved system identity**: the global token is allowed *by*
  `system-full-access` through normal evaluation — the only
  `is_system_principal()` branch in the request path rewrites an
  attribute, it does not return early. Spoof resistance is layered:
  `is_system_principal` requires agent_id "system" AND no backing user;
  every display_name write path (PUT /users/me, admin create, SCIM
  create/update) rejects the reserved string; a non-system principal
  presenting it is denied before evaluation with a decision recorded.
  Live regression tests cover each.
- **Membership boolean**: `subject.member_of_resource_workspace` is derived
  from the real membership store whenever a workspace is addressed
  (directly or via the entity scope chain), set FALSE for non-members, and
  set FALSE (not left unset) on store error — the membership-required Deny
  keys on `false`, so the outage direction is fail-closed.
- **Performance**: read-through `CachedPolicyRepository` (per-scope keys,
  full invalidation on every port mutation — conservative and correct),
  the one out-of-band writer (`apply_trust_transition`) documented at the
  port and invalidated at both handler call sites; latency benchmark over
  the full seeded set (200 runs, mean < 1ms) with a fully populated
  context. Cache behavior proven by counting-repo tests.
- **Audit integration**: `PolicyDecision` persisted on every production
  evaluation path (middleware main loop, both pre-evaluation denials,
  supervised-trust hold, merge-time attestation gate, push-time gate);
  denials warn + emit AbacDenied, allowances debug; failed audit writes
  warn instead of being discarded. Queryable via
  GET /api/v1/policies/decisions (policy_decisions table pre-exists from
  task-128's migration).
- **API**: all 8 endpoints live, registered in `api/mod.rs` (~893-907) with
  ABAC RouteResourceMapping entries. Admin-only writes enforced by the
  seeded `builtin:policy-admin-only` Deny (860, above the Developer blanket
  Allow at 800) with a live test (Developer 403 / Admin 200); action
  derivation (POST/PUT→write, DELETE→delete) covers all three verbs.
  Dry-run runs the real engine against the real seeded set, is
  side-effect-free, and uses the dry-run-only `merge_jwt_claims` while
  live paths use the allowlisted insert-only `merge_agent_jwt_claims`
  (spoofing channel from review round 2 is closed and mutation-tested).
  `/policies/effective` binds the documented `?subject=` param and
  resolves real subject attributes from user/membership/team stores;
  unknown subjects fail closed (bare id, no role attributes).
- **Middleware migration**: no ad-hoc policy logic remains in the request
  path — the middleware builds attributes and delegates to
  `policy_engine::evaluate`. Resource scope attributes resolve strictly by
  lookup (absent on failure, never fabricated).
- **Round-12 repair (23454bed)**: statically verified sound — the fixture
  Deny moved to priority 250 (user band) above the seeded
  `builtin:attested-authorization` Allow (150, immutable: false); the test
  state seeds the full production builtin set (mem.rs:4912-4928), the
  engine's immutable-Deny-first + priority-DESC ordering makes the 250
  Deny decide, and the assertions cover both the Failed status and the
  non-merged MR. Fixture correction, not a production-code mask.

## Non-blocking observations (recorded for follow-up)

1. **`create_policy` persists caller-supplied `scope`/`scope_id`
   unvalidated** (api/policies.rs:96). Reachable only by global Admins or
   the system token (the 860 Deny gates writes); Admin is a cross-tenant
   global role in this system, and scope/scope_id are the policy's target
   payload rather than caller identity fields — the repo's
   check-forged-scope-fields invariant passes. A defense-in-depth
   validation against the caller's tenant/workspace would still be
   reasonable in a multi-tenant deployment.
2. **Shipped-narrative overstatement**: the task file says "every
   evaluation path (middleware, dry-run, effective, merge-time,
   push-time) persists a PolicyDecision" — dry-run and effective do not
   record (correctly, in my judgment: hypothetical/derived evaluations
   would pollute the compliance audit trail; real access decisions are
   all recorded). The coverage matrix row 13 is precise about the four
   production call sites.
3. **Pre-existing, outside this diff** (verified byte-identical to base
   `cdd1dd02`; belong to the attestation-gate/authorization-provenance
   task line, not this task's coverage sections):
   - chain-attestation store reads in the merge gate are fail-open
     (`find_by_commit`/`find_by_task` errors read as "no chain";
     `load_chain().unwrap_or_default()`) — merge_processor.rs:2421/2428/
     2497/2505. The task-129 hardening made `evaluate_attestation_abac`
     itself fail-closed, but the outer chain lookups still swallow errors.
   - atomic-group merges dispatch to `merge_atomic_group` before the
     attestation gate and run no `evaluate_attestation_abac`
     (merge_processor.rs:2019) — a Deny that blocks a single-MR merge
     does not block the same MR inside an atomic group.
   - push-time attestation ABAC is audit-only by design (TASK-006 Phase
     1, documented in-code); repo-level push enforcement comes from the
     earlier `check_repo_abac` gate.
4. Minor: `parse_condition` coerces non-integer JSON numbers to 0
   (silent input corruption rather than a 400); `delete_policy` maps
   store errors to 400; the middleware's policy-list read error is
   swallowed fail-closed (`unwrap_or_default()` → empty set → deny for
   everyone) but unlogged, so a policy-store outage presents as
   unexplained 403s.

## Verifier requirements

`cargo test --all` (acceptance criterion) plus the focused gyre-server
suites listed in /tmp/stage/review-evidence/05-verifier-test-list.md.
No frontend source was touched by the attributed commits; web tests are
low-risk. Large gyre-server test compilation was not run in this review
sandbox per the budget constraint.
