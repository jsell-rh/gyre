# Review — task-158 (Implement tenant-scope meta-spec changes)

Spec: `specs/system/meta-spec-reconciliation.md` §8 (as amended by this candidate's spec-lifecycle change to the binding-model realization).
Candidate under review: `8e4a5901668e151b0ad37c51246a326202a854de` (production commit `3c22bfa87` + base merge `7da7bbfff` + task-doc re-record `8e4a59016`).
Verdict: **needs-revision** (2 findings — 1 test gap, 1 low-severity fail-open). Evidence: `/tmp/stage/review-evidence/` (probes.md, diffs, mechanical-checks-rerun.txt, base-failing-scripts.txt).

## Round 1

### What was verified as real

- **Scope/registry**: `MetaSpecScope::Tenant` variant with tenant id in `scope_id`; migration `2026-11-07-000057` widens the `meta_specs.scope` CHECK via the portable three-table rebuild and adds `tenant_meta_spec_sets`; creation validates the tenant exists (400/404 tests); `scope=Tenant&scope_id=` listing enforces caller-tenant containment; SQLite round-trip test on a fresh migrated DB pins the widened CHECK.
- **Tenant set port + adapters**: `TenantMetaSpecSetRepository` with mem/SQLite/Postgres implementations, separate table from workspace sets (id-string shadowing test), wired through `store!` in `build_state` (not a bare in-memory AppState alias — in-memory-state-stores check passes).
- **API**: `GET/PUT /api/v1/tenants/{id}/meta-specs/set` registered in `api/mod.rs` AND the ABAC route resolver, with per-handler Admin + tenant containment (tests: roundtrip, non-Admin 403, cross-tenant 403, unknown tenant 404).
- **Inheritance**: `merge_meta_spec_sets` / `workspace_overrides_path` are mutually consistent by case analysis (path pinned in any section; persona-key rebinding drops the tenant pin); workspace pin wins; consumers of the effective set include the workspace set GET, `compute_meta_spec_set_sha` (byte-identical for tenant-less workspaces — stored JSON is the canonical serialization and the merge re-serializes identically), blast radius (`tenant_inheritance` vs `workspace_binding` reasons), preview, and the `meta_spec_changed` fan-out.
- **Propagation controller**: `run_tenant_reconciliation` lists the tenant's workspaces, gates on tenant-set-bound paths, skips overriders, runs the §6 wave per non-overriding workspace; the approval trigger for Tenant-scoped registry entries is wired in `update_meta_spec_registry` and genuinely tested.
- **Force flag**: Tenant-scope-only (400 otherwise), admin-of-scope-tenant only (403 cross-tenant), bypasses the override skip.

### Test runs (this checkout at the candidate)

- `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib reconciliation::tests::tenant` → 5 passed, 0 failed.
- Focused §8 set (blast radius, merge, SHA, effective-set GET, tenant-set endpoint, force validation) → 11 passed, 0 failed.
- `SKIP_WEB_BUILD=1 cargo test -p gyre-adapters --lib -- sqlite::meta_spec` → 16 passed, 0 failed.
- All AGENTS.md-enforced mechanical checks pass; non-enforced scripts that fail do so identically at the base commit `7da7bbfff` (worktree comparison — no new violations).
- Protected task text (everything before `## Shipped`) is byte-identical to the assigned contract — the previous round's checkbox edits are fully restored.

### Mutation probes (source restored, `git status --short` clean after each)

- Disable the override skip in `run_tenant_reconciliation` → skip-override and approval-flow tests FAIL. ✓ genuinely tested.
- Disable the force bypass → force and approval-flow tests FAIL. ✓ genuinely tested.
- **Remove the tenant-set PUT propagation trigger from `put_tenant_meta_spec_set` entirely → the FULL `cargo test -p gyre-server --lib` suite passes 1350/1350.** ✗ untested trigger (finding 1).

### Findings

1. **[test] Tenant-set PUT propagation trigger has no killing test** (`crates/gyre-server/src/api/meta_specs.rs`, `put_tenant_meta_spec_set`). The candidate's own spec-§8 amendment makes "a tenant meta-spec set update propagates its changed pins" a normative trigger, yet removing the `run_tenant_reconciliation` call leaves the entire server suite green. The only API test that PUTs a tenant set with content binds a *Pending* spec and asserts NO tasks are created — it cannot distinguish trigger-present from trigger-absent. Fix: a test that PUTs a tenant set pinning an **Approved** meta-spec and asserts per-workspace reconciliation tasks for non-overriding workspaces (plus, ideally, a removal-only re-PUT asserting no new tasks, since "removals are not triggers" is also documented and untested).
2. **[code] Fail-open override gate in `run_tenant_reconciliation`** (`crates/gyre-server/src/reconciliation.rs`): `load_workspace_set(state, &ws.id).await.unwrap_or_default()` silently treats a corrupt stored workspace set (or storage error) as an empty set, dropping the workspace's overrides so tenant propagation — including urgent force changes — stomps an overrider. This contradicts `load_workspace_set`'s own doc comment and the repo's standing never-fail-open-a-decision-signal rule; the sibling loads fail closed (`load_tenant_set(...).unwrap_or_default()` → nothing propagates) or propagate errors (`resolve_effective_meta_spec_set`). Reachability is low (API writes are serde-canonical) but the gate is a governance decision signal — fail closed (log + skip the workspace, or propagate).

Both findings are cheap to fix; no structural, scope-safety, or fakery defects were found in the rest of the implementation.
