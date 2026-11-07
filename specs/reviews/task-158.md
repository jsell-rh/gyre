# Review — task-158 (Implement tenant-scope meta-spec changes)

Spec: `specs/system/meta-spec-reconciliation.md` §8 (Tenant-Scope Meta-Spec
Changes). Candidate `bac70a024` (base `233664cf8`), scoped to commits
`3c22bfa87`, `381962ab9`, `5ef42f5c3` per the task's `commits:` frontmatter
(plus docs/process commits `7825f6a19`, `bac70a024` recording the same).

Verdict: **not approved — one code finding** (rustfmt violations on
candidate-changed lines; trivial repair). No functional, scope-safety, or
fake-implementation defects found; the two prior review findings are properly
fixed. Rust compile/test execution is deferred to remote verification
(infrastructure restriction, not a code defect — see below).

## Round aa6fdd18 (this review)

### Build restriction (infrastructure, not a defect)

crates.io is unreachable from this review sandbox (curl exit 7 on
index.crates.io / static.crates.io), `CARGO_HOME` has no registry, and no
prebuilt target dir exists — same restriction as the executor's round-6107f63d
sandbox. `cargo test`/`cargo build` could not run. Evidence:
`/tmp/stage/review-evidence/build-restriction-round-aa6fdd18.txt`.

Consequently commit `5ef42f5c3` (fan-out fix + killing test) has never been
compiled or executed. It was verified line-by-line by inspection in this
review — fix logic, call shapes, port signatures, helper availability, test
name uniqueness, rustfmt parse — recorded in
`/tmp/stage/review-evidence/inspection-5ef42f5c3-round-aa6fdd18.txt`.
**Remote verification MUST run** (recorded as deferred checks):

1. `SKIP_WEB_BUILD=1 cargo test -p gyre-server --lib` — full lib suite.
2. Focused: `... --lib global_spec_approval_skips_unreadable_workspace_set`.
3. Mutation kill: restore the `?` propagation in the Global/Workspace approval
   fan-out (`crates/gyre-server/src/api/meta_specs.rs`, `newly_approved`
   else-branch) → that test must FAIL.
4. `python3 scripts/check-clippy-diff.py 233664cf8` (changed-lines clippy).

### Verified sound (by inspection this round; earlier rounds by execution)

- **Prior finding 1 (fan-out 500 after persist) fixed.** In
  `update_meta_spec_registry`, no error propagation remains between
  `state.meta_specs.update(&ms)` and `Ok(Json(ms))`: the Tenant branch calls
  `run_tenant_reconciliation` (summary return, no `Result`), the
  Global/Workspace branch now `match`es `resolve_effective_meta_spec_set` and
  skips the workspace with a `tracing::warn!` — the same fail-closed-skip as
  the tenant controller. A corrupt `meta_spec_sets` row can no longer 500 an
  approval that already persisted while leaving a partially executed fan-out.
- **Prior finding 2 (compiler warning) fixed.** `.expect("seed built-in
  policies")` on the `block_in_place` result; `seed_builtin_policies` returns
  `anyhow::Result<()>` (abac_middleware.rs:744), so it compiles; pattern
  matches siblings at meta_specs.rs:841/:1283.
- **§8 acceptance criteria** all have real production code behind them:
  `MetaSpecScope::Tenant` (domain + 000057 migration widening the scope CHECK
  via a portable three-table rebuild), `TenantMetaSpecSetRepository` port with
  mem/SQLite/PostgreSQL adapters and a separate `tenant_meta_spec_sets`
  table, `GET/PUT /api/v1/tenants/{id}/meta-specs/set` (Admin + tenant
  containment + unknown-tenant 404, ABAC resolver entry present),
  `resolve_effective_meta_spec_set` merging tenant defaults under workspace
  overrides (workspace pin wins; persona-key rebinding drops the tenant pin),
  `run_tenant_reconciliation` (per-workspace waves, override skip,
  fail-closed unreadable-set skip counted in `workspaces_skipped_error`,
  unbound paths never propagate), and the `force` flag (400 on non-Tenant
  scope, 403 cross-tenant admin). `compute_meta_spec_set_sha` now hashes the
  canonical effective-set serialization and is byte-identical for
  tenant-less workspaces whose sets were stored via the API (canonical
  serialization — `put_meta_spec_set` stores `serde_json::to_string(&set)`).
- **Test suite** (executed at `381962ab9`: 1356 passed / 0 failed in
  gyre-server --lib, 359 passed in gyre-adapters --lib, with recorded
  mutation kills for the set-PUT trigger and the fail-closed override gate)
  covers every §8 scenario: propagation, override skip, force bypass,
  unbound paths, blast radius incl. tenant isolation, merge semantics,
  effective-set SHA inheritance, workspace GET effective view, full API
  flows, tenant-set GET/PUT roundtrip/admin/containment/404, force 400/403.
  The new `global_spec_approval_skips_unreadable_workspace_set` is
  inspection-verified only (restriction above).
- **Mechanical checks**: 20/20 green offline at the candidate (arch,
  abac-route-registry, abac-exempt-handlers, migration-versions,
  migration-sql-portability, mem-port-contracts, in-memory-state-stores,
  forged-scope-fields, scope-literal-defaults, dead-message-kinds,
  inert-enforcement, mcp-write-tools, byte-slice-truncation,
  fail-open-ref-resolution, relative-path-defaults, lossy-secret-conversion,
  fabricated-scope-defaults, forwarded-header-trust,
  unbounded-external-http, task-commit-attribution). Evidence:
  `/tmp/stage/review-evidence/mechanical-checks-round-aa6fdd18.txt`.

### Finding (blocks approval)

**F1 (code, minor but CI-breaking on integration): rustfmt violations on
candidate-changed lines.** `python3 scripts/check-rustfmt-diff.py 233664cf8`
(the assigned base) exits 1:

- `crates/gyre-server/src/api/meta_specs.rs`: lines 453, 971, 1542–1544
- `crates/gyre-server/src/api/specs.rs`: lines 567, 1628
- `crates/gyre-server/src/constraint_check.rs`: line 975
- `crates/gyre-server/src/reconciliation.rs`: lines 632–635, 727–730, 1525,
  1620, 2533, 2545

Every flagged line is attributed by `git blame` to `3c22bfa87` (this task's
feature commit) — none is pre-existing debt — and direct `rustfmt --edition
2021` on file copies confirms each would be reformatted (e.g.
`merge_meta_spec_sets`/`diff_tenant_meta_spec_set` signatures fit on one line;
the `emit_tenant_meta_spec_set_updated(...)` call at meta_specs.rs:453-455 is
wrapped non-canonically). The per-commit CI gate
(`check-rustfmt-diff.py HEAD^1`) passes at the pushed head only because the
head commit's parent (`7825f6a19`) is docs-only; merging the candidate into
main as a merge commit makes `HEAD^1` the main baseline, and the same check
fails. This also makes the shipped-notes claim "new lines rustfmt-clean; only
pre-existing debt lines differ" inaccurate for a base-anchored run.
Reproduction and attribution evidence:
`/tmp/stage/review-evidence/rustfmt-base-diff-round-aa6fdd18.txt`.
Repair: rustfmt the flagged regions (no semantic change expected).

### History note

Durable findings from earlier rounds of this task (fail-open override gate,
missing set-PUT trigger test, fan-out 500-after-persist, compiler warning)
were each verified fixed in this candidate; no regression was found in the
fixes.
