# Review — task-158 (meta-spec-reconciliation §8 — Tenant-Scope Meta-Spec Changes)

Spec: `specs/system/meta-spec-reconciliation.md` §8. Commits under review
(base `cfeefbda975211a6ac24a17e5920be6f8b23008b` → candidate
`96c3304cea690fb09005032503f7b002ef96add8`, product surface:
`3c22bfa8`, `381962ab`, `5ef42f5c`, `b7eaa775`, `17fc6bf7` — all recorded in
the task frontmatter, attribution gate green). Verdict: **complete**.

## Acceptance criteria vs. implementation (source-verified)

- **MetaSpecScope includes Tenant variant** — real: `gyre-domain/src/meta_spec.rs`
  adds `Tenant` (+`as_str`/`parse`), scope entity in `scope_id` mirroring the
  Workspace design. Migration `2026-11-07-000059` widens the `meta_specs.scope`
  CHECK via a portable three-table rebuild (parent copied before children, so
  FK checks pass on SQLite and PG) and creates `tenant_meta_spec_sets`.
  Verified executing: `cargo test -p gyre-adapters --lib tenant` → 27 passed,
  incl. `tenant_scoped_meta_spec_round_trip` (fresh migrated SQLite DB) and
  the five new tenant-set repo tests. Migration versioning gate green (the
  date-qualified Diesel versions don't collide with 2026-11-08-000059; neither
  later migration touches meta_spec tables, so ordering is safe).
- **Tenant-level meta-spec sets managed via API** — real:
  `GET/PUT /api/v1/tenants/{id}/meta-specs/set` with Admin-role + caller-tenant
  containment + tenant existence checks in-handler, route registered in the
  ABAC resolver (resource `meta_spec`), persistence through a new
  `TenantMetaSpecSetRepository` port with mem/SQLite/PostgreSQL adapters
  (production wiring via the `store!` macro selects the DB adapters; not a
  mem-only stand-in — `check-in-memory-state-stores` green).
- **Workspace inheritance with override support** — real:
  `resolve_effective_meta_spec_set` merges tenant defaults with the
  workspace's own pins (workspace pin wins on conflict; persona-role rebinding
  drops the tenant pin). `merge_meta_spec_sets` and `workspace_overrides_path`
  are consistent by construction (verified case-by-case: ws-pins-path,
  ws-rebinds-role, cross-section pins). All consumers resolve the effective
  set: workspace set GET, `compute_meta_spec_set_sha` (byte-identical
  serialization for tenant-less workspaces, so pre-upgrade SHAs stay valid),
  blast radius, preview blast radius, `meta_spec_changed` fan-out, and §10
  conformance sweep.
- **Tenant approval triggers per-workspace reconciliation** — real:
  `update_meta_spec_registry`'s newly-approved branch dispatches Tenant-scoped
  specs to `run_tenant_reconciliation`, which lists the tenant's workspaces
  (`workspaces.list_by_tenant`), skips overriders, and runs a §6/§7
  `run_reconciliation` wave per non-overriding workspace (rollout policy
  applied per workspace). Second trigger: tenant-set PUT diff
  (`diff_tenant_meta_spec_set`; removals are not triggers — pinned by test).
- **Overriding workspaces skipped** — real and fail-closed: a workspace whose
  own set cannot be read (storage error/corrupt JSON) is skipped and counted
  (`workspaces_skipped_error`), never decoded as "no overrides" — the
  fail-open variant is kill-tested with force=true. The Global/Workspace
  approval fan-out got the same fail-closed-skip repair in `5ef42f5c3`
  (previously `?` could 500 a persisted approval mid-fan-out), kill-tested by
  `global_spec_approval_skips_unreadable_workspace_set`.
- **Tenant admin force flag bypasses workspace rollout policies** — real:
  `force` on the tenant-set PUT and on Tenant-scoped registry approval;
  400 on non-Tenant scopes, 403 for an admin outside the scope tenant
  (both pinned by tests). Force cannot stomp a workspace with an unreadable
  set (fail-closed test).
- **Tests cover inheritance, override, and force** — yes: eleven §8 tests in
  `reconciliation.rs` plus eight API/registry tests in `api/meta_specs.rs`,
  several driving the real router (`api_router().oneshot`). No tests removed
  or weakened (test counts only grew; 0 removed `#[tokio::test]` lines).
  Kill properties verified by reading (see review-evidence/summary.md).

Spec lifecycle: §8 was amended (commit `71dd8628f` lineage) to replace the
never-shipped `POST /api/v1/tenant/meta-spec-rollout` wave-plan sketch with
the shipped binding model (tenant set + inheritance + override skip + force).
This matches the assigned task's implementation plan (which specifies exactly
this model) and the goal's rule that outdated specs are amended, not
worked-around. Coverage row 12 → `implemented` with accurate anchors;
api-reference rows updated.

## Probes run this round (review-stage constraints honored)

- `cargo check -p gyre-server` and `cargo check -p gyre-server --lib --tests`
  → clean (all new tests type-check on the current tree).
- `cargo test -p gyre-adapters --lib tenant` → 27 passed, 0 failed.
- Changed-line clippy (lib targets of gyre-server/-adapters/-ports,
  intersected with the base diff): **zero warnings** — the prior round's
  `double_must_use` / `needless_borrow` failures (durable finding df2f95d9)
  are fixed by `17fc6bf7a`.
- `check-rustfmt-diff.py` (changed lines), `check-arch.sh`,
  `check-migration-versions.sh`, `check-abac-route-registry.sh`,
  `check-abac-exempt-handlers.sh`, `check-mem-port-contracts.sh`,
  `check-in-memory-state-stores.sh`, `check-migration-sql-portability.sh`,
  `check-task-commit-attribution.sh` (at candidate state) and the remaining
  invariant gates → all green.
- Full logs and the deferred-check list: `/tmp/stage/review-evidence/`.

## Deferred to remote verification (wrapper/4 GiB policy)

1. `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` — full suite, esp. the
   §8 tests listed in review-evidence/summary.md. Note
   `global_spec_approval_skips_unreadable_workspace_set` (from `5ef42f5c3`)
   has no recorded execution yet; it compiles (probe 2) and its mechanism
   (corrupt-set upsert through the port) is the same one the executed
   reconciliation.rs fail-closed test uses.
2. `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib` (full; tenant subset
   executed here).
3. `python3 scripts/check-clippy-diff.py cfeefbda975211a6ac24a17e5920be6f8b23008b`
   (workspace `--all-targets` clippy; changed lib lines already verified clean
   per-package).

## Non-blocking observations (recorded, no action required)

- `compute_meta_spec_set_sha` logs "treating as unbound" for corrupt set JSON
  but actually hashes the empty effective set (non-empty SHA). Behavior is
  consistent on both the input and agent sides of the §9 constraint (they
  compare equal, same as the old raw-bytes hash of a corrupt blob) and
  fail-closed in the §10 sweep (drift) — only the log wording is imprecise.
- `run_tenant_reconciliation` loads the tenant set with a silent
  `.unwrap_or_default()` (storage error → nothing propagates — safe
  direction, but unlike the workspace-set path there is no warn log).
- The blast-radius endpoint decodes corrupt sets as absent (fail-open on the
  advisory report only; all propagation gates fail closed).

## Verdict

Approved. Every acceptance criterion is backed by real, port-backed
production code with per-handler authorization; the adapters/migration layer
is proven by executed tests against fresh SQLite databases; type checks,
rustfmt-diff, changed-line clippy, and every repo mechanical gate pass at the
candidate. Server-suite execution is deferred to the verifier per the
review-stage build policy, with the exact test list recorded.
