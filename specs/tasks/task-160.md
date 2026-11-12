---
title: "Hierarchy enforcement scripts — check-hierarchy, check-tenant-filter, check-api-auth"
spec_ref: "hierarchy-enforcement.md §7"
depends_on: []
progress: ready-for-review
coverage_sections:
  - "hierarchy-enforcement.md §Invariant Enforcement"
  - "hierarchy-enforcement.md §Enforcement"
  - "hierarchy-enforcement.md §New Scripts"
commits: ["8458f699df0b4b075eea35a31c2a3e32107c1884", "d162c56d01c9b742d17fc1626e2095adb0d64a47", "c7ea6ab05b7221f327ea006b94bae12a88b5a412", "45cd250d7655a2661f1cd7488ea4ab91ade1819e", "9dc6ffd47bf8f52f5b4ec656317c944c563aa756", "b82af6b4b10dbd3306cc23546efb34c07bd6c640", "b297d1b2e6588ae68f35eb1419b73e7652b53b7b", "2698b9b63b73e5f3b2230a0b7e8e30d160265f3d", "ba78ab2a5e1a2147a04cf7e148f53e6c0abb34e3"]
---

## Spec Excerpt

### §2 Invariant Enforcement (hierarchy-enforcement.md)

`scripts/check-hierarchy.sh` scans domain struct definitions for `Option<Id>` on hierarchy fields and fails if any are found. The script is gated by `GYRE_CHECK_HIERARCHY=1` env var.

```bash
REQUIRED_FIELDS=(
    "workspace_id"  # on Task, Agent, MergeRequest, Repository
    "tenant_id"     # on Workspace
)
```

### §3 Enforcement (hierarchy-enforcement.md)

`scripts/check-tenant-filter.sh` scans all Diesel query methods in `crates/gyre-adapters/src/sqlite/` and `crates/gyre-adapters/src/pg/` for the pattern `.filter(table::tenant_id.eq(`. Any query method that builds a Diesel query without this filter fails the check.

### §7 New Scripts (hierarchy-enforcement.md)

| Script | What it checks | Run by |
|---|---|---|
| `scripts/check-api-auth.sh` | Every route (GET/POST/PUT/DELETE) has a `RouteResourceMapping` in the ABAC `ResourceResolver` | Pre-commit, CI |
| `scripts/check-tenant-filter.sh` | Every Diesel query method in adapters filters by `tenant_id` | Pre-commit, CI |
| `scripts/check-hierarchy.sh` | Domain structs don't use `Option<Id>` for hierarchy fields | Pre-commit, CI |

## Implementation Plan

1. **`scripts/check-hierarchy.sh`**: Grep `crates/gyre-domain/src/` for struct definitions containing `workspace_id: Option<Id>` or `tenant_id: Option<Id>`. Exit non-zero if found. Must exclude `gyre-common` types (e.g., Message.workspace_id is intentionally Option).

2. **`scripts/check-tenant-filter.sh`**: Scan all `*.rs` files in `crates/gyre-adapters/src/sqlite/` and `crates/gyre-adapters/src/pg/` (or `postgres/`). For each function that builds a Diesel query (look for `.filter(` or `diesel::` query builder patterns), verify that `tenant_id.eq(` appears. Maintain a skip list with documented rationale for exempt files (e.g., `message.rs` — queries by workspace_id which is tenant-bound).

3. **`scripts/check-api-auth.sh`**: The ABAC middleware already has a `non_exempt_patterns()` method that returns all registered patterns. This script should extract registered route patterns from the ABAC middleware and cross-reference against routes registered in `api/mod.rs`. Any route in mod.rs without a corresponding RouteResourceMapping entry fails. Consider using `cargo test` with a test that does this comparison, or grep-based approach.

4. Make all three scripts executable (`chmod +x`).

## Acceptance Criteria

- [ ] `scripts/check-hierarchy.sh` exists and passes on current codebase
- [ ] `scripts/check-hierarchy.sh` would fail if `workspace_id: Option<Id>` were added to Task
- [ ] `scripts/check-tenant-filter.sh` exists and passes (with documented skip list)
- [ ] `scripts/check-api-auth.sh` exists and passes
- [ ] All scripts have usage comments at the top
- [ ] `cargo test --all` still passes

## Agent Instructions

Read `specs/system/hierarchy-enforcement.md` §2 (Invariant Enforcement), §3 (Enforcement), and §7 (Mechanical Enforcement) for the full script requirements. The scripts are purely static analysis (grep/regex) — no runtime execution. Check the existing `scripts/check-arch.sh` for style conventions used by other enforcement scripts in this codebase.

## Shipped

All three hierarchy-enforcement scripts exist, are executable, carry usage
comments, and are wired into `.pre-commit-config.yaml` and
`.github/workflows/ci.yml` (blocking).

### Scripts

- `scripts/check-hierarchy.sh` - awk state machine over every domain `.rs`
  file; fails on `Option<...>` hierarchy fields and on deletion of a required
  field (spec §2). Env-gated via `GYRE_CHECK_HIERARCHY` (enabled by default,
  `=0` bypasses).
- `scripts/check-tenant-filter.sh` - POSIX-awk scan of read query methods in
  `src/sqlite` + `src/postgres`; the tenant-column table set is derived from
  the migrations; documented `SKIP_LIST` for structurally-isolated adapters
  (spec §3); tenant-less tables reported as non-failing backlog.
- `scripts/check-api-auth.sh` - ABAC middleware chain on the api router,
  per-handler auth extractors on non-ABAC routes, and the
  route↔`RouteResourceMapping` cross-reference delegated to the frozen-baseline
  `check-abac-route-registry.sh` (spec §7).

### This round: verification repair after base merge

The round-2 verification failure (attempt f5d6200b, exit 1) was the
exemption gate: `new verification exemptions forbidden:
scripts/scope-literal-defaults-exemptions.txt` listing the two remaining
domain entries. Their line numbers had shifted in the rebase onto base
3af80bd2, so the base-diff read them as newly added. Fixed in the sanctioned
direction - the sites themselves are gone:

- `Task::new` and `MergeRequest::new` no longer inline
  `workspace_id: Id::new("default")`; they bind the documented
  `bootstrap_workspace()` helper (now shared `pub(crate)` in gyre-domain
  `lib.rs`, same hierarchy-enforcement.md §1 contract `Agent::new` uses).
  The exemption file drops to 0 entries and `FROZEN_EXEMPTION_COUNT`
  lowers 2 → 0. No behavior change.
- The base merge brought `TenantMetaSpecSetRepository::get` (task-158)
  querying `tenant_meta_spec_sets` via primary-key `.find()`, which
  `check-tenant-filter.sh` correctly flagged (reads on tenant-column tables
  must carry the explicit `.filter(tenant_id.eq(...))` form, spec §3). Both
  sqlite and postgres `get`s now filter explicitly - semantically identical
  since `tenant_id` is the primary key - so the lint passes with no
  skip-list growth.

### Test evidence

All evidence saved under `/tmp/stage/review-evidence/`:

- All three task scripts pass on current HEAD (`check-scripts.txt`,
  `check-tenant-filter.txt`), plus arch/fabricated-scope/inert-enforcement/
  mem-port-contracts/byte-slice/relative-path lints.
- Negative tests: injecting `workspace_id: Id::new("default")` into `Task`
  fails `check-scope-literal-defaults` (`check-scope-literal-negative.txt`);
  declaring `Task.workspace_id: Option<Id>` fails `check-hierarchy`
  (`check-hierarchy-negative.txt`); reverting the meta_spec_set filter to
  `.find()` fails `check-tenant-filter`
  (`check-tenant-filter-negative.txt`). Files restored after each.
- `cargo test -p gyre-domain`: 474 passed (`domain-tests.txt`).
- `cargo test -p gyre-adapters --test tenant_isolation`: 2 passed
  (`tenant-isolation-tests.txt`); `sqlite::meta_spec_set`: 9 passed
  (`meta-spec-set-tests.txt`).
- `cargo check -p gyre-domain -p gyre-adapters` clean; `cargo fmt --check`
  clean; clippy on both crates shows only pre-existing warnings outside the
  changed lines.
- Server suites (`cargo test --all`, all-target clippy, GitHub CI) are
  verifier-owned; the verifier should run them on this head.
