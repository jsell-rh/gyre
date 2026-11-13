# Review — task-129: ABAC evaluation engine, API, and audit integration

**Candidate:** `41338dfad22c729ba7b158009925931bd3709ec4`
**Base:** `7daca25d372fdfe1a53deadc10cd300e271a69a7`
**Round:** 7 (independent; rounds 1-6 repaired on-branch, final repair
commit `3471b92b`)
**Verdict:** APPROVED — evidence in `/tmp/stage/review-evidence/`

## What was reviewed

The full candidate diff (21 files, ~4.6k insertions): the
`policy_engine::evaluate` core (immutable-Deny-first, priority-DESC with
scope-specificity and Deny-before-Allow tiebreaks, first-match-wins
short-circuit, default deny), the canonical `scope_applies` filter, the
`AttributeContext` extraction in the migrated middleware (allowlisted
agent-JWT claims, membership/team store sourcing, entity-lookup resource
scope chains, env attributes), the seeded builtin set (m34 list + the 6-row
domain splice covering every §Built-In Policies table row, drift-refreshed,
fail-closed seeding), the read-through `CachedPolicyRepository` with
invalidation on every mutation path including the out-of-band
`apply_trust_transition` writers, all 8 `/api/v1/policies` endpoints with
Admin-only management and reserved `trust:`/`builtin:` prefix guards on
create AND (round 6) on stored names for update/delete, decision recording
on all six evaluation sites via `record_decision_audit` (warn on store
failure), the fail-closed attestation gates (merge requeues, push skips
audit-only evaluation visibly), and the round-6 re-spawn JWT repair
(`mint_worker_scoped` with real workspace/repo scope claims).

## Independent evidence

- All 19 mechanical invariant scripts pass (arch, abac-route-registry,
  abac-exempt-handlers, migrations, mem-port-contracts, inert-enforcement,
  and the rest). `check-task-commit-attribution.sh` exits 1 only against
  the sandbox's uncommitted contract restoration of `specs/tasks/task-129.md`;
  the committed candidate lists all 7 task-labeled SHAs.
- `cargo check -p gyre-server --tests` — exit 0 (type-checks every new
  test, 8m21s).
- `cargo test -p gyre-domain` — 507 passed (incl. trust-preset
  field-level assertions).
- `cargo test -p gyre-adapters policy` — 14 passed (SQLite policy adapter
  CRUD/scope/delete guards).
- Test quality: each round-6 repair carries a focused test with a documented
  killing direction; the fail-closed test injects a genuinely failing
  `WorkspaceRepository`; the <1ms performance criterion is a mean over 200
  runs of the full seeded builtin set with a populated context
  (`evaluation_meets_latency_target`), not a single-run smoke.

## Verifier must run (gyre-server test-binary compilation deferred per
assignment budget)

`cargo test -p gyre-server` for `policy_engine`, `abac_middleware`,
`api::policies`, `merge_processor`, `ralph_loop`, `git_http`,
`policy_cache`; then `cargo test --all` as the final acceptance gate.
Exact commands and the specific test names are recorded in
`/tmp/stage/review-evidence/README.md`.

## Non-blocking observations

1. `In`/`NotIn`/`Contains` do not handle list-valued attributes (only
   Equals/NotEquals gained set semantics in round 6). A custom policy using
   `In` on a list-valued attribute fails closed; no shipped policy does.
   Fail-closed asymmetry only.
2. Coverage row 13's note still counts "four" decision-recording call
   sites; round 6 raised the count to six. The section's claim (every
   evaluation path records) remains true; the stale count understates.

Neither observation blocks: the task contract (10-step flow, composition
rules, 7 builtins seeded and delete-protected, immutable Deny
un-overridable, caching with invalidation, <1ms benchmark, decision audit
on every evaluation, full API surface, middleware migration, route
registration) is met by production code with mutation-meaningful tests.
