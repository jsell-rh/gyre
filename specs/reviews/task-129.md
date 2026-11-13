# Review: task-129 — ABAC evaluation engine, API, and audit integration

**Candidate:** `ba8ae373c1bae37d74c364ff2bf69b787bfc2071`
**Base:** `6f5a631c9fc09664a3e766cd10536f8b56d9ce2f`
**Reviewer round:** 9 (independent; review rounds 1–8 recorded in the branch history)
**Verdict:** APPROVED

## Scope

Diff `6f5a631c..ba8ae373`: `gyre-server` (policy_engine.rs, policy_cache.rs [new],
abac_middleware.rs, api/policies.rs, api/scim.rs, api/users.rs, api/workspaces.rs,
auth.rs, git_http.rs, merge_processor.rs, ralph_loop.rs, lib.rs, mem.rs),
`gyre-domain/src/policy.rs`, `gyre-ports/src/workspace.rs`, docs/api-reference.md,
specs/coverage/system/abac-policy-engine.md, specs/system/abac-policy-engine.md.
No `web/` source changes and no `scripts/` or exemption-file changes in the diff.

## Independent probes (evidence in /tmp/stage/review-evidence)

| Probe | Result |
|---|---|
| `bash scripts/check-abac-context-parity.sh` (round-7 durable finding) | exit 0 — RESOLVED (scratch-context `// abac-context:ok` at abac_middleware.rs:1948; subject.tenant_id hoisted above the lint window) |
| All 19 other gated mechanical scripts (arch, abac-route-registry, abac-exempt-handlers (79 handlers), mem-port-contracts, inert-enforcement, forged-scope-fields, scope-literal-defaults, fabricated-scope-defaults, unbounded-external-http, forwarded-header-trust, in-memory-state-stores, lossy-secret-conversion, fail-open-ref-resolution, byte-slice-truncation, relative-path-defaults, migration-versions, migration-sql-portability, dead-message-kinds, mcp-write-tools) | all exit 0 |
| `cargo test -p gyre-domain policy` | 6/6 pass |
| `cargo test -p gyre-adapters policy` | 14/14 pass |
| `cargo check -p gyre-server` | clean (only build-script informational messages; no rustc warnings) |

## Findings against the acceptance criteria

- **10-step evaluation flow / composition / first-match-wins / short-circuit** —
  `policy_engine::evaluate` implements immutable-Deny-first (bypasses priority
  ordering), then priority-DESC with scope-specificity (repo>ws>tenant) and
  deny-before-allow tiebreakers (`policy_sort_key`), first-match short-circuit,
  default deny. Killing-direction unit tests cover each rule
  (`immutable_deny_blocks_even_when_high_priority_allow_matches`,
  `deny_overrides_allow_at_same_priority_and_scope`,
  `higher_priority_deny_beats_lower_priority_allow`,
  `workspace_scoped_policy_does_not_apply_out_of_scope`, …).
- **Canonical scope applicability** — `scope_applies` filters scoped policies
  against the resource scope chain (repo → workspace → tenant, entity-resolved,
  never caller-claimed); `scope_id: None` = all entities at that level; absent
  scope attributes fail closed (no fabricated scope). Shared by every
  evaluation path (middleware, dry-run, effective, merge-time, push-time).
- **7 built-ins seeded on startup** — m34 set + domain splice covers every
  §Built-In Policies table row; seeding is idempotent, drift-refreshed, and
  FAILS CLOSED on store errors (`builtin_policy_seeding_fails_closed_on_store_error`).
  Built-ins not deletable (guard in BOTH adapters; live-asserted in
  `spec_table_builtins_seeded_on_startup`); `builtin:require-human-spec-approval`
  is immutable and evaluated before all priority policies.
- **Reserved system identity** — `SYSTEM_ABAC_IDENTITY` ("gyre-system-token")
  is non-assignable on every display_name write path (SSO provisioning, SCIM
  create/update, users API) and denied pre-evaluation for non-system principals
  that still present it; the genuine global token is allowed BY
  `system-full-access` through normal evaluation (no middleware bypass — the
  old bypass was removed, matching the spec amendment).
- **Performance** — read-through `CachedPolicyRepository` (list/list_by_scope
  cached; all mutations invalidate on success); the one out-of-band policy
  writer (`apply_trust_transition`) is invalidated explicitly at both handler
  call sites and the port documents the cache contract; membership lookups
  precomputed in the auth context; `<1ms` benchmark
  (`evaluation_meets_latency_target`, mean of 200 runs over the full seeded
  set) present. Cache tests prove reads are served from cache and mutations
  invalidate (counting-repo probes).
- **Audit integration** — `PolicyDecision` persisted on every evaluation path
  (middleware, merge-time supervised-trust hold, merge-time attestation gate,
  push-time attestation) via `record_decision_audit` (warns on write failure);
  denied at warn + `AbacDenied` audit event, allowed at debug; queryable via
  `GET /api/v1/policies/decisions` (SQLite `policy_decisions` table + mem
  adapter); decisions carry concrete resource ids.
- **API** — all 8 endpoints registered in `api/mod.rs` (~893–906) with
  `RouteResourceMapping` entries (route-registry check passes). Create is
  Admin-only via the seeded `builtin:policy-admin-only` Deny (860) with a live
  killing test (`policy_management_is_admin_only_live`: Developer 403, Admin
  200). Reserved `trust:`/`builtin:` prefixes rejected on create AND on
  update/delete by stored name (round-6 desync fix). Dry-run honors the scope
  filter; effective-permissions binds the documented `?subject=` param and
  loads real subject attributes from the stores.
- **Middleware migration** — `abac_middleware` evaluates through the new
  engine; claim merging is agent-only + allowlisted + insert-only (spoofing
  tests); `subject.global_role` uses set semantics with no fabricated
  ReadOnly fallback (roleless-caller killing test); membership boolean fails
  closed on store errors; merge/push attestation gates fail closed
  (requeue / visible skip) on store errors.
- **Other diffs reviewed** — re-spawn JWT carries worker-scope claims
  (`mint_worker_scoped`, round-6 containment fix with killing test);
  `git_http` push-time attestation evaluation extracted into a testable unit
  with propagated store errors; no stubs/`todo!`/`bail!` stand-ins introduced.

## Non-blocking observations (recorded, not defects)

1. `abac_middleware.rs:~2358`: `state.policies.list().await.unwrap_or_default()`
   degrades a policy-store read failure to an empty set → deny-all. This is
   fail-CLOSED (an empty set can only default-deny), so it is not an
   enforcement defect, but the failure is unlogged — an observability gap
   given the repo's visible-failure discipline elsewhere.
2. Pre-existing (present at base, unchanged by this candidate): two migrations
   share the 6-digit sequence `000061` (`2026-11-08-000061_spec_links`,
   `2026-11-12-000061_repo_access_overrides`,
   `2026-11-13-000061_repo_budget_and_compute_minutes`). Diesel versions by
   full directory name and `check-migration-versions.sh` passes; noting for
   the record only.
3. Generated-output observation: `web/dist/` untouched by this candidate (no
   authored frontend changes in scope); the deterministic verifier owns the
   bundle rebuild.

## Verifier must run (deferred per build-budget rules)

- `cargo test -p gyre-server policy_engine`
- `cargo test -p gyre-server policy_cache`
- `cargo test -p gyre-server abac_middleware`
- `cargo test -p gyre-server api::policies`
- `cargo test -p gyre-server auth`
- `cargo test -p gyre-server api::scim` and `cargo test -p gyre-server api::users`
- `cargo test -p gyre-server ralph_loop`
- `cargo test -p gyre-server git_http`
- `cargo test -p gyre-server merge_processor`
- `cargo test --all` (task acceptance criterion)

Independent evidence (mechanical checks, domain/adapters tests, cargo check)
supports the task contract; the prior durable finding is verified fixed.
