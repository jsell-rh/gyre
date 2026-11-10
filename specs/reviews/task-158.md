# Review — task-158 (Implement tenant-scope meta-spec changes)

Spec: `specs/system/meta-spec-reconciliation.md` §8 "Tenant-Scope Meta-Spec Changes".
Candidate under review: `f61745fec5afc2fa33d4680e23321a67e34fbbbf` (assigned base `b5a872087899e8bc4a18092ab56990a26e9ca9af`).
Task commits: `3c22bfa8`, `381962ab`, `5ef42f5c`, `b7eaa775` plus rebase/merge/process commits recorded in the task's `commits:` frontmatter (`check-task-commit-attribution` green).
Verdict: **approved** — all acceptance criteria verified against production code; no findings.

## Probes run (in this sandbox, at candidate `f61745fec`, cargo reachable)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` → **1635 passed, 0 failed** (includes the full §8 suite and every killing test from prior repair rounds; matches the merge-round record).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib` → **414 passed, 0 failed** (renumbered migration `2026-11-07-000059_tenant_meta_spec_sets` runs on a fresh DB alongside main's 000056–000058; tenant-set repo roundtrip/overwrite/delete/independence; Tenant-scoped meta-spec round-trip + scope filter).
- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib tenant` → 65 passed (§8 block enumerated by name below).
- `SKIP_WEB_BUILD=1 cargo check -p gyre-cli` → clean (the wrapper rejects workspace-wide invocations; the four touched crates — common/ports/domain/adapters/server — all compile via the test builds, gyre-cli confirmed separately).
- Mutation kills (source edited temporarily, restored, tree verified clean afterwards — see `/tmp/stage/review-evidence/mutation*.txt`):
  1. Fail-open override gate restored in `run_tenant_reconciliation` (`match` → `unwrap_or_default()`) → `tenant_reconciliation_fails_closed_on_unreadable_workspace_set` **FAILED**. Kill confirmed.
  2. Set-PUT propagation trigger disabled (`if false && !changed_paths.is_empty()` in `put_tenant_meta_spec_set`) → `tenant_set_put_propagates_approved_pins_to_inheriting_workspaces` **FAILED**. Kill confirmed.
  3. Error propagation restored in the Global/Workspace approval fan-out (fail-closed skip → `return Err(...)`) → `global_spec_approval_skips_unreadable_workspace_set` **FAILED**. Kill confirmed.
  4. Override check disabled (`workspace_overrides_path` → constant `false`) → `tenant_reconciliation_skips_overriding_workspace` **FAILED**. Kill confirmed.
- 19 mechanical invariant checks green (arch, abac-route-registry, abac-exempt-handlers, migration-versions, migration-sql-portability, mem-port-contracts, in-memory-state-stores, forged-scope-fields, scope-literal-defaults, dead-message-kinds, inert-enforcement, mcp-write-tools, byte-slice-truncation, fail-open-ref-resolution, relative-path-defaults, lossy-secret-conversion, fabricated-scope-defaults, forwarded-header-trust, unbounded-external-http).
- `python3 scripts/check-rustfmt-diff.py b5a872087899e8bc4a18092ab56990a26e9ca9af` → clean (18 Rust files).
- `bash scripts/check-task-commit-attribution.sh` → OK.

## Acceptance criteria verification

- **MetaSpecScope includes Tenant variant** — `gyre-domain/src/meta_spec.rs`: `Tenant` arm with `"Tenant"` serialization/deserialization; scope entity id carried in `scope_id` (mirrors the Workspace design, so the enum stays a plain string column). Migration `000059` widens the `meta_specs.scope` CHECK via a portable three-table rebuild (children dropped before parent, parents copied before children; no SQLite/PG-only SQL — portability check green).
- **Tenant-level meta-spec sets managed via API** — `GET/PUT /api/v1/tenants/{id}/meta-specs/set` registered in `api/mod.rs:663` and in the ABAC resolver (`abac_middleware.rs:416`, resource `meta_spec`); per-handler authorization is Admin role + tenant containment + tenant existence (404). New `TenantMetaSpecSetRepository` port (get/upsert/delete by tenant id) with mem, SQLite, and PostgreSQL adapters in a dedicated `tenant_meta_spec_sets` table (a tenant and workspace sharing an id string cannot shadow each other). Tests: roundtrip, Admin-only 403, containment 403, unknown tenant 404 — all pass.
- **Inheritance with override support** — `resolve_effective_meta_spec_set` / `merge_meta_spec_sets` (reconciliation.rs): workspace pin wins on conflict; persona keys rebound by the workspace drop the tenant pin; the merge is consistent with `workspace_overrides_path` ("not overridden" always means "inherits the tenant pin"). Every consumer resolves the effective set: workspace set GET, `compute_meta_spec_set_sha` (byte-identical serialization for tenant-less workspaces — no upgrade drift on recorded SHAs), blast-radius endpoint (reason `tenant_inheritance` vs `workspace_binding`), preview blast radius, and the `meta_spec_changed` fan-out. Tests `effective_set_merges_tenant_defaults_with_overrides`, `effective_set_sha_reflects_inheritance`, workspace-GET-returns-effective-set all pass.
- **Tenant approval triggers per-workspace reconciliation** — `update_meta_spec_registry` newly-approved block: Tenant scope → `run_tenant_reconciliation(tenant, [path], force)`; Global/Workspace → effective-set fan-out with fail-closed skip (mutation 3 proves the skip test kills). Tests `tenant_spec_approval_triggers_per_workspace_reconciliation`, `tenant_reconciliation_creates_tasks_for_inheriting_workspaces` pass.
- **Overriding workspaces skipped** — `run_tenant_reconciliation` checks `workspace_overrides_path` per changed path (mutation 4 proves the test kills); unreadable workspace sets (storage error / corrupt JSON) are skipped fail-closed with a `workspaces_skipped_error` counter (mutation 1 proves the killing test). Unbound paths do not propagate (`tenant_reconciliation_ignores_unbound_paths`).
- **Force flag bypasses workspace rollout policies** — `force` on the tenant-set PUT and on Tenant-scoped registry approval; validation: 400 on non-Tenant scopes, 403 for an admin outside the scope tenant, internal error on a Tenant-scoped entry missing `scope_id`. Tests `tenant_reconciliation_force_bypasses_workspace_override`, `force_rejected_for_non_tenant_scoped_spec`, `force_requires_admin_of_the_scope_tenant` pass.
- **Tests cover inheritance, override, and force** — verified by the enumerated §8 suite plus four independent mutation kills; none of the killing tests is self-confirming.

## Merge-repair specifics (round 8f042330, the newest material changes)

- `mem.rs` AppState wiring keeps main's shared `meta_spec_binding_store` (so main's delete-guard contract test sees bindings) and adds the separate `tenant_meta_spec_sets` field — both features coexist; full suite green confirms.
- `prompt_assembly.rs` — the new `Tenant` arm injects required Tenant-scoped meta-specs of the task workspace's tenant into band 1; caller tenant resolved via `workspaces.find_by_id` with warn-and-skip on lookup failure (no fabricated tenant identity). Without this fix a required Tenant-scoped entry would silently never inject — the merge resolution is correct and covered by the passing prompt-assembly suite.
- `api-reference.md` rows correctly merge main's registry shape with the Tenant scope + force semantics.

## Minor observations (non-blocking, no action required)

- The blast-radius read endpoint decodes corrupt stored sets with `.ok()` (treated as absent). This is the pre-existing pattern at base (base `meta_specs.rs:460-462/670-672`) extended to the tenant set, on a read-only diagnostic endpoint, failing in the conservative direction for reporting. Not a new defect; the enforcement paths (`run_tenant_reconciliation`, approval fan-out, SHA computation) are all fail-closed.
- `run_tenant_reconciliation`'s `load_tenant_set(...).unwrap_or_default()` treats an unreadable tenant set as unbound (nothing propagates) — safe direction, and the PUT trigger reloads a set it just wrote successfully.

Test-inflation check: none found. Every §8 behavior named in the task plan has a test that was proven killing by mutation in this review or a prior round (re-verified here), and the full suites pass at the exact candidate.

Evidence: `/tmp/stage/review-evidence/` (test outputs, mutation kills, mechanical checks, rustfmt, cli check).
