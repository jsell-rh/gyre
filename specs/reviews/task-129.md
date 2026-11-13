# Review: TASK-129 — Implement ABAC evaluation engine, API, and audit integration

**Reviewer:** Independent review agent
**Round:** R7 (of this candidate lineage; rounds 1–6 repaired in-branch)
**Candidate:** `44706d28dbda0dc536d1a3cf4aadb864b4be1e80`
**Assigned base:** `f7f5c59f22682d40e6f995b137dfa3ea9bfd3ad3` (merge-base with candidate: `7daca25d`)
**Verdict:** `needs-revision` (one mechanical-invariant regression; implementation substance approved)

---

## Scope and method

Task commits inspected (merges of already-contained upstream SHAs are tree
no-ops): `0dcb1017`, `81ccc284`, `0c148e28`, `9ba4dd40`, `35711565`,
`0dc466e7`, `3471b92b`, `1bb51ffe` (+ process bookkeeping commits).

Read: `specs/system/abac-policy-engine.md` §Evaluation Engine → §Dry-Run
Evaluation against `policy_engine.rs`, `policy_cache.rs`, `abac_middleware.rs`
(extraction/seed/evaluation path), `api/policies.rs`, `api/mod.rs` (route
registration + RouteResourceMapping), `auth.rs` (system principal),
`git_http.rs` / `merge_processor.rs` (push/merge-time evaluation + audit
recording), `gyre-domain/src/policy.rs` (builtin set, SYSTEM_ABAC_IDENTITY),
`gyre-adapters` policy repository + `policy_decisions` migration.

Probes run (evidence in `/tmp/stage/review-evidence/`):

- 19 gated mechanical scripts (`check-arch`, `check-abac-route-registry`,
  `check-abac-exempt-handlers`, `check-mem-port-contracts`,
  `check-migration-versions`, `check-inert-enforcement`, …): **all PASS**.
- 39 advisory scripts compared candidate-vs-base: **identical results** — no
  new violations.
- `cargo test -p gyre-domain policy`: 6/6 PASS.
- `cargo test -p gyre-adapters policy`: 14/14 PASS (SQLite CRUD, scope
  filtering, built-in delete guard, prefix+scope deletes).
- `cargo check -p gyre-server`: PASS (clean; build.rs ran `npm ci` + web build
  successfully).
- `cargo fmt --check` (server/domain/adapters): PASS.
- `bash scripts/check-abac-context-parity.sh`: **FAIL on candidate (3
  violations), PASS at merge-base `7daca25d`** — the finding below.

Server test *execution* is deferred to the verifier per the build wrapper
(4 GiB budget). Required verifier probes:

```
cargo test --all                                   # acceptance criterion
cargo test -p gyre-server --lib policy_engine      # composition, immutable-deny-first,
                                                   # scope applicability, latency target
cargo test -p gyre-server --lib policy_cache       # cache + invalidation
cargo test -p gyre-server --lib abac_middleware    # seeding, live builtin enforcement,
                                                   # admin-only policy mgmt, spoofed identity
cargo test -p gyre-server --lib api::policies      # CRUD, dry-run, decisions, effective
cargo test -p gyre-server --lib auth               # is_system_principal discrimination
cargo test -p gyre-server --lib merge_processor    # trust-hold + decision recording
cargo test -p gyre-server --lib git_http           # push-time attestation ABAC
```

## Findings

- [-] **F1 (blocking, mechanical invariant regression): `check-abac-context-parity`
  fails on the candidate (3 violations) after passing at the merge-base.**
  The lint is a pre-commit hook (`.pre-commit-config.yaml:392`,
  `abac-context-parity`) and its violations are the exact class the repo
  freezes ("never add new entries"). Two sites, both introduced by this
  task's commits:
  1. `crates/gyre-server/src/abac_middleware.rs:1934` — the round-6
     `path_resource_id` helper builds a scratch `AttributeContext` (used only
     to extract the addressed resource id for pre-evaluation denial audit
     records) without the `// abac-context:ok` exemption comment; flagged for
     both `subject.type` and `subject.tenant_id`. No functional defect — it
     never evaluates policies — but it must carry the exemption marker.
  2. `crates/gyre-server/src/abac_middleware.rs:2108` — the canonical
     evaluation context itself is flagged missing `subject.tenant_id` because
     task-129's inserted documentation/role-set comment blocks pushed the
     `ctx.set("subject.tenant_id", &auth.tenant_id)` call (now ~line 2160)
     beyond the lint's 30-line scan window. The attribute IS set (verified by
     reading the code and by `builtin_tenant_isolation_denies_cross_tenant_live`),
     so this is a lint-window false positive — but the enforced gate still
     fails. Fix: hoist the `subject.tenant_id` set above the `global_role`
     block (it has no dependency on it), and annotate the scratch context.
  Reproduce: `bash scripts/check-abac-context-parity.sh` → exit 1
  (3 violations); at `7daca25d` → exit 0. History: 1 violation since the
  initial task commit `0dcb1017`, 3 since round-6 `3471b92b`.

## Substantive assessment (supports the implementation)

- **Evaluation engine** (`policy_engine::evaluate`): 10-step flow, immutable
  Deny first (before priority ordering, cannot be overridden), priority-DESC
  with scope-specificity then Deny-before-Allow tiebreaks, first-match-wins
  with short-circuit, default deny. Mutation-killing tests present
  (`immutable_deny_blocks_even_when_high_priority_allow_matches`,
  `deny_overrides_allow_at_same_priority_and_scope`, scope-applicability
  suite, dynamic-reference fail-closed suite with an outcome-flipping
  killing case).
- **Composition**: `policy_sort_key` = (priority, scope rank, deny rank) —
  matches all four §Policy Composition rules.
- **Built-ins**: all 7 §Built-In Policies table rows seeded at startup
  (`main.rs:55` → `seed_builtin_policies`, fail-closed, drift-refreshed).
  `system-full-access` matches the reserved `subject.id
  == "gyre-system-token"` (not type); reservation enforced on every
  display_name write path (profile/admin/SSO) plus pre-evaluation denial of
  spoofed carriers (`spoofed_system_identity_denied_before_evaluation`).
  `agent-repo-scope` seeded ENABLED with the dynamic-reference conditions at
  priority 750 > the role Allow at 700; live end-to-end test mints a real
  worker JWT and asserts 200 in-scope / 403 out-of-scope with the decision
  attributed to `builtin-agent-repo-scope`. Immutable
  `builtin:require-human-spec-approval` at 999.
- **Performance**: read-through `CachedPolicyRepository` (per-scope keys,
  success-invalidated mutations, explicit invalidation for the one
  out-of-band writer `apply_trust_transition`); latency benchmark
  `evaluation_meets_latency_target` (mean over 200 runs of the full seeded
  set < 1ms) — deferred to verifier.
- **Audit integration**: `PolicyDecision` persisted on every evaluation path
  (HTTP middleware, merge trust-hold, merge attestation gate, git-http push
  gate) via `record_decision_audit` (warns on write failure); warn/debug log
  contract; queryable via `GET /api/v1/policies/decisions`.
- **API**: all 8 endpoints registered (`api/mod.rs:893-906`) with
  RouteResourceMapping entries; Admin-only management via seeded
  `builtin:policy-admin-only` Deny (860) over the Developer write Allow
  (800), live-tested; built-in/reserved-prefix guards on update/delete
  (including round-6 stored-name guard for `trust:` presets); dry-run runs
  the real engine against the real seeded set; `/policies/effective`
  resolves the subject from real stores and binds the documented `?subject=`
  param.
- **Middleware migration**: the request path builds the full attribute
  context (auth extractor, allowlisted agent-claim merge, path/entity
  extraction, membership-derived role set + membership boolean, env
  attributes) and calls `policy_engine::evaluate` — no parallel legacy
  evaluation.

## Non-blocking observations (not introduced by this candidate)

- `abac_middleware.rs:2344` `state.policies.list().await.unwrap_or_default()`
  silently degrades a policy-store failure to an empty set → default deny.
  Fail-closed (safe direction) and pre-existing at the merge-base (base line
  2154); the round-6 repair propagated this error class on the merge/push
  paths with context. Worth aligning for visibility, not a regression.
- `policy_cache.rs:418,452,456` — three `let _ = repo.list().await.unwrap();`
  hits in test code trip the (unenforced) `check-silent-result-discard.sh`;
  that script already fails massively at base and is not in CI or pre-commit.
- `check-assertionless-tests.sh` / `check-self-confirming-tests.sh` exit 2 on
  both base and candidate (pre-existing script/environment issue, zero
  violations printed).
