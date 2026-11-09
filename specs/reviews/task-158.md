# Review — task-158 (Implement tenant-scope meta-spec changes, meta-spec-reconciliation.md §8)

Spec: `specs/system/meta-spec-reconciliation.md` §8 "Tenant-Scope Meta-Spec Changes".
Candidate under review: `a57da98bd` (rebase round 38c991eb onto main `fefde2946`; diff scoped to the four feature commits in the task's `commits:` frontmatter, all present on the branch — `3c22bfa8`, `381962ab9`, `5ef42f5c3`, `b7eaa775c` — plus the source-level rebase merge itself).
Verdict: **approved** (round 5 of independent review; this round's focus was the rebase, whose prior sandbox could not compile — all deferred Rust checks were executed here for the first time).

## What ran (this round; evidence under /tmp/stage/review-evidence/)

This sandbox could build (the implementation rounds' crates.io-unreachable restriction did not apply), so every deferred check from the rebase round was executed at the exact candidate head:

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` → **1503 passed, 0 failed**; no compiler warnings.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib` → **389 passed, 0 failed** — includes the tenant set repo roundtrip/overwrite/delete and `tenant_scoped_meta_spec_round_trip` on fresh migrated databases, proving the renumbered `2026-11-07-000059_tenant_meta_spec_sets` migration runs and widens the `meta_specs.scope` CHECK to admit `Tenant`.
- Mutation kills re-verified post-rebase (source restored and re-tested green after each, working tree verified clean):
  - deleting the `run_tenant_reconciliation` call from `put_tenant_meta_spec_set` → `tenant_set_put_propagates_approved_pins_to_inheriting_workspaces` FAILS;
  - restoring the fail-open override gate (`unwrap_or_default` on `load_workspace_set`) → `tenant_reconciliation_fails_closed_on_unreadable_workspace_set` FAILS.
- 19 mechanical invariant checks green (arch, ABAC route registry + exempt handlers, migration versions + SQL portability, mem port contracts, in-memory state stores, forged scope fields, scope literal defaults, fabricated scope defaults, dead message kinds, inert enforcement, MCP write tools, byte-slice truncation, fail-open ref resolution, relative path defaults, lossy secret conversion, forwarded header trust, unbounded external HTTP).
- `check-rustfmt-diff.py fefde2946` green (17 files); `check-task-commit-attribution.sh` green.

## Verified against the acceptance criteria

- **MetaSpecScope Tenant variant** — domain enum + `as_str`/`parse`, SQLite/Postgres adapters, migration 000059 (portable three-table rebuild, children dropped before parent, parents copied before children); creation validates the scope tenant exists (400 missing scope_id, 404 unknown tenant); `scope=Tenant&scope_id=` listing requires caller-tenant containment.
- **Tenant-level set managed via API** — `GET/PUT /api/v1/tenants/{id}/meta-specs/set`, Admin-only + tenant containment + tenant existence (404), persisted through the new `TenantMetaSpecSetRepository` port (mem/SQLite/Postgres, separate table so tenant/workspace id collisions never shadow), registered in the ABAC resolver, api-reference documented.
- **Inheritance with override** — `resolve_effective_meta_spec_set` / `merge_meta_spec_sets` (workspace pin wins; persona-key rebinding drops the tenant pin; merge consistent with `workspace_overrides_path`). Every consumer resolves the effective set: workspace set GET, `compute_meta_spec_set_sha` (byte-identical for tenant-less workspaces — no upgrade drift, verified by the SHA test and the untouched legacy SHA tests), blast radius (`tenant_inheritance` vs `workspace_binding` reasons), preview, and the `meta_spec_changed` fan-out.
- **Approval triggers per-workspace reconciliation** — Tenant-scope branch of the registry approval handler calls `run_tenant_reconciliation`, which lists the tenant's workspaces, skips overriders, and runs a §6 `run_reconciliation` wave per non-overriding workspace (composing with task-157's per-workspace rollout policies — §8 step 3). Killing tests cover both triggers (approval and tenant-set PUT diff; removals are not triggers; unbound paths have empty blast radius; never crosses tenants).
- **Overrides skipped / force bypasses** — override-skip test, force test, and force authorization tests (400 on non-Tenant scopes, 403 cross-tenant force, Admin-only tenant-set endpoints).
- **Fail-closed override gate** — unreadable workspace set (storage error / corrupt JSON) skips the workspace (`workspaces_skipped_error`), never decoded as "no overrides"; the same fail-closed skip covers the Global/Workspace approval fan-out (`global_spec_approval_skips_unreadable_workspace_set`, which passes at this head).

## Observations (non-blocking)

- Tenant-scoped registry **creation** does not enforce caller-tenant containment (an admin of tenant A can mint a Tenant-scoped entry for tenant B). This mirrors the pre-existing platform-admin registry model — Global scope in base has the same cross-tenant reach, the shipped api-reference documents registry create as "Admin only" without containment, and the test suite records the design intent ("registry creation is platform-admin governed; force is the tenant-scoped power"). The genuinely dangerous powers are contained: `force` requires an admin of the scope tenant, and the tenant-set PUT (which writes tenant B's inherited defaults directly) enforces tenant containment. Unforced approval of a cross-tenant entry can only create reconciliation tasks in workspaces whose tenant set already pins the same path name. Recorded as a hardening candidate, not a defect against §8.
- `compute_meta_spec_set_sha` hashes the default empty set (not the empty string) when stored set JSON is corrupt after the row exists — a drift signal for the §10 sweep and a constraint mismatch against any SHA recorded under a readable set; fail-closed in effect. Not a defect.

Prior rounds' findings (fail-open override gate, missing set-PUT killing test, Global/Workspace approval 500-after-persist, rustfmt violations) were each repaired with killing tests, all of which pass and re-kill at this head. Spec §8 was amended through the spec lifecycle to document the shipped binding-model realization; coverage row 12 is `implemented` consistent with the verified behavior.
